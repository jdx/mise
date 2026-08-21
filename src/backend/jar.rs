use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{
    ensure_plain_bin_name, get_filename_from_url, template_string, verify_artifact,
};
use crate::backend::version_list;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::{file, hash};
use async_trait::async_trait;
use eyre::Result;
use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;

pub const EXPERIMENTAL: bool = true;

/// Default Maven repository for coordinate-style tools (`jar:group/artifact`).
const DEFAULT_REPOSITORY: &str = "https://repo1.maven.org/maven2";

/// maven-metadata.xml lists releases as `<version>x.y.z</version>` entries in
/// ascending publish order, which matches the ordering contract of
/// `_list_remote_versions`.
const MAVEN_METADATA_VERSION_REGEX: &str = "<version>([^<]+)</version>";

/// Installs runnable JAR files and generates `java -jar` launcher scripts so
/// jar-based tools behave like any other mise-managed binary.
///
/// Two addressing modes:
/// - Maven coordinates: `jar:com.squareup.wire/wire-compiler` downloads from a
///   Maven repository (Maven Central by default) and lists versions from
///   maven-metadata.xml.
/// - Direct URL: any tool name plus a templated `url` option, for jars only
///   published as e.g. GitHub release assets.
#[derive(Debug)]
pub struct JarBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct JarOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> JarOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn raw(&self) -> &'a ToolVersionOptions {
        self.values.raw()
    }

    fn url(&self) -> Option<String> {
        self.values.platform_string("url")
    }

    fn checksum(&self) -> Option<String> {
        self.values.platform_string("checksum")
    }

    fn repository(&self) -> Option<String> {
        self.values.platform_string("repository")
    }

    fn classifier(&self) -> Option<String> {
        self.values.platform_string("classifier")
    }

    fn bin(&self) -> Option<String> {
        self.values.platform_string("bin")
    }

    fn java_args(&self) -> Option<String> {
        self.values.platform_string("java_args")
    }

    fn version_list_url(&self) -> Option<&'a str> {
        self.values.str("version_list_url")
    }

    fn version_regex(&self) -> Option<&'a str> {
        self.values.str("version_regex")
    }

    fn version_json_path(&self) -> Option<&'a str> {
        self.values.str("version_json_path")
    }

    fn version_expr(&self) -> Option<&'a str> {
        self.values.str("version_expr")
    }
}

/// Maven coordinates parsed from a `jar:group/artifact` tool name.
#[derive(Debug, PartialEq)]
struct MavenCoords {
    group_id: String,
    artifact_id: String,
}

impl MavenCoords {
    /// Parse `group/artifact` from a tool name. Returns `None` for names
    /// without exactly one `/` — those are only valid in URL mode.
    fn parse(tool_name: &str) -> Option<Self> {
        let (group_id, artifact_id) = tool_name.split_once('/')?;
        if group_id.is_empty() || artifact_id.is_empty() || artifact_id.contains('/') {
            return None;
        }
        Some(Self {
            group_id: group_id.to_string(),
            artifact_id: artifact_id.to_string(),
        })
    }

    /// Base URL of this artifact's directory in a Maven repository.
    fn artifact_dir(&self, repository: &str) -> String {
        format!(
            "{}/{}/{}",
            repository.trim_end_matches('/'),
            self.group_id.replace('.', "/"),
            self.artifact_id
        )
    }

    fn metadata_url(&self, repository: &str) -> String {
        format!("{}/maven-metadata.xml", self.artifact_dir(repository))
    }

    fn jar_url(&self, repository: &str, version: &str, classifier: Option<&str>) -> String {
        let classifier = classifier.map(|c| format!("-{c}")).unwrap_or_default();
        format!(
            "{}/{version}/{}-{version}{classifier}.jar",
            self.artifact_dir(repository),
            self.artifact_id
        )
    }
}

impl JarBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn maven_coords(&self) -> Option<MavenCoords> {
        MavenCoords::parse(&self.ba.tool_name)
    }

    fn repository(&self, opts: &JarOptions<'_>) -> String {
        opts.repository()
            .unwrap_or_else(|| DEFAULT_REPOSITORY.to_string())
    }

    /// Resolve the artifact URL for a version. An explicit `url` option wins;
    /// otherwise Maven coordinates from the tool name are required.
    fn artifact_url(&self, opts: &JarOptions<'_>, tv: &ToolVersion) -> Result<String> {
        if let Some(url_template) = opts.url() {
            return Ok(template_string(&url_template, tv));
        }
        if let Some(coords) = self.maven_coords() {
            return Ok(coords.jar_url(
                &self.repository(opts),
                &tv.version,
                opts.classifier().as_deref(),
            ));
        }
        Err(eyre::eyre!(
            "jar backend requires Maven coordinates (`jar:group/artifact`, e.g. \
             `jar:com.squareup.wire/wire-compiler`) or a `url` option"
        ))
    }

    /// Name of the generated launcher (and the installed `lib/<bin>.jar`).
    /// Defaults to the Maven artifactId, or the tool name in URL mode.
    fn bin_name(&self, opts: &JarOptions<'_>) -> Result<String> {
        if let Some(bin) = opts.bin() {
            ensure_plain_bin_name("bin", &bin)?;
            return Ok(bin);
        }
        Ok(self
            .maven_coords()
            .map(|c| c.artifact_id)
            .unwrap_or_else(|| self.ba.tool_name.clone()))
    }

    /// POSIX sh launcher. `java_args` is inserted verbatim so it can reference
    /// environment variables; per-invocation JVM flags go through `JAVA_OPTS`.
    fn unix_launcher(&self, tv: &ToolVersion, bin_name: &str, java_args: &str) -> String {
        let java_args = if java_args.is_empty() {
            String::new()
        } else {
            format!("{java_args} ")
        };
        format!(
            r#"#!/bin/sh
# Generated by mise for {full}@{version}. Do not edit.
jar="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)/lib/{bin_name}.jar"
if [ -n "${{JAVA_HOME:-}}" ] && [ -x "${{JAVA_HOME}}/bin/java" ]; then
  java_bin="${{JAVA_HOME}}/bin/java"
else
  java_bin=java
fi
# shellcheck disable=SC2086
exec "$java_bin" {java_args}${{JAVA_OPTS:-}} -jar "$jar" "$@"
"#,
            full = self.ba.full(),
            version = tv.version,
        )
    }

    /// Windows cmd launcher, mirroring the sh launcher's JAVA_HOME/JAVA_OPTS
    /// behavior.
    fn windows_launcher(&self, tv: &ToolVersion, bin_name: &str, java_args: &str) -> String {
        let java_args = if java_args.is_empty() {
            String::new()
        } else {
            format!("{java_args} ")
        };
        format!(
            "@echo off\r\n\
             rem Generated by mise for {full}@{version}. Do not edit.\r\n\
             setlocal\r\n\
             set \"JAR=%~dp0..\\lib\\{bin_name}.jar\"\r\n\
             if defined JAVA_HOME (set \"JAVA_BIN=%JAVA_HOME%\\bin\\java.exe\") else (set \"JAVA_BIN=java\")\r\n\
             \"%JAVA_BIN%\" {java_args}%JAVA_OPTS% -jar \"%JAR%\" %*\r\n",
            full = self.ba.full(),
            version = tv.version,
        )
    }

    /// Lay out the install directory: `lib/<bin>.jar` plus launcher scripts in
    /// `bin/`. The jar gets a stable, versionless name so scripts and configs
    /// can reference it without embedding the version.
    fn install_jar(&self, tv: &ToolVersion, opts: &JarOptions<'_>, jar_path: &Path) -> Result<()> {
        let bin_name = self.bin_name(opts)?;
        let install_path = tv.install_path();
        let lib_dir = install_path.join("lib");
        let bin_dir = install_path.join("bin");
        file::create_dir_all(&lib_dir)?;
        file::create_dir_all(&bin_dir)?;

        file::copy(jar_path, lib_dir.join(format!("{bin_name}.jar")))?;

        let java_args = opts.java_args().unwrap_or_default();
        let launcher = bin_dir.join(&bin_name);
        file::write(&launcher, self.unix_launcher(tv, &bin_name, &java_args))?;
        file::make_executable(&launcher)?;
        file::write(
            bin_dir.join(format!("{bin_name}.cmd")),
            self.windows_launcher(tv, &bin_name, &java_args),
        )?;
        Ok(())
    }

    /// Verify against a lockfile checksum, or generate one when lockfiles are
    /// enabled. Jars are platform-independent, but lock entries are keyed by
    /// platform for consistency with other backends.
    fn verify_lock_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file_path: &Path,
    ) -> Result<()> {
        let filename = file_path.file_name().unwrap().to_string_lossy();
        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();

        if let Some(checksum) = &platform_info.checksum {
            ctx.pr.set_message(format!("checksum {filename}"));
            let (algo, check) = checksum
                .split_once(':')
                .ok_or_else(|| eyre::eyre!("Invalid checksum format: {checksum}"))?;
            hash::ensure_checksum(file_path, check, Some(ctx.pr.as_ref()), algo)?;
        } else if Settings::get().lockfile_enabled() {
            ctx.pr.set_message(format!("generate checksum {filename}"));
            let h = hash::file_hash_blake3(file_path, Some(ctx.pr.as_ref()))?;
            platform_info.checksum = Some(format!("blake3:{h}"));
        }

        if let Some(expected_size) = platform_info.size {
            let actual_size = file_path.metadata()?.len();
            if actual_size != expected_size {
                return Err(eyre::eyre!(
                    "Size mismatch for {filename}: expected {expected_size}, got {actual_size}"
                ));
            }
        } else if Settings::get().lockfile_enabled() {
            platform_info.size = Some(file_path.metadata()?.len());
        }

        Ok(())
    }
}

/// Returns install-time-only option keys for the jar backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "url".into(),
        "checksum".into(),
        "repository".into(),
        "classifier".into(),
        "bin".into(),
        "java_args".into(),
        "version_list_url".into(),
        "version_regex".into(),
        "version_json_path".into(),
        "version_expr".into(),
    ]
}

#[async_trait]
impl Backend for JarBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Jar
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["java"])
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &[
            "repository",
            "version_list_url",
            "version_regex",
            "version_json_path",
            "version_expr",
        ]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = JarOptions::new(&raw_opts);

        let versions = if let Some(url) = opts.version_list_url() {
            version_list::fetch_versions(
                url,
                opts.version_regex(),
                opts.version_json_path(),
                opts.version_expr(),
            )
            .await?
        } else if let Some(coords) = self.maven_coords() {
            version_list::fetch_versions(
                &coords.metadata_url(&self.repository(&opts)),
                Some(MAVEN_METADATA_VERSION_REGEX),
                None,
                None,
            )
            .await?
        } else {
            // URL mode without a version listing source: versions are whatever
            // the config pins, same as the http backend.
            vec![]
        };

        Ok(versions
            .into_iter()
            .map(|v| VersionInfo {
                version: v,
                ..Default::default()
            })
            .collect())
    }

    /// Jar artifacts are platform-independent, so `mise lock` resolves the same
    /// URL and checksum for every target platform.
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let raw_opts = tv.request.options();
        let opts = JarOptions::new(&raw_opts);
        let url = self.artifact_url(&opts, tv)?;
        Ok(PlatformInfo {
            url: Some(url),
            checksum: opts.checksum(),
            ..Default::default()
        })
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let raw_opts = tv.request.options();
        let opts = JarOptions::new(&raw_opts);

        let url = self.artifact_url(&opts, &tv)?;
        let filename = get_filename_from_url(&url);
        let file_path = tv.download_path().join(&filename);

        tv.lock_platforms
            .entry(self.get_platform_key())
            .or_default()
            .url = Some(url.clone());

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &file_path, Some(ctx.pr.as_ref()))
            .await?;

        verify_artifact(&tv, &file_path, opts.raw(), Some(ctx.pr.as_ref()))?;
        self.verify_lock_checksum(ctx, &mut tv, &file_path)?;

        ctx.pr.set_message("install jar".into());
        self.install_jar(&tv, &opts, &file_path)?;

        Ok(tv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendResolution;
    use crate::toolset::{ToolRequest, ToolSource, parse_tool_options};

    fn jar_backend(tool_name: &str) -> JarBackend {
        JarBackend {
            ba: Arc::new(BackendArg::new_raw(
                format!("jar-{}", tool_name.replace('/', "-")),
                Some(format!("jar:{tool_name}")),
                tool_name.to_string(),
                None,
                BackendResolution::new(true),
            )),
        }
    }

    fn jar_test_tv(tool_name: &str, version: &str) -> ToolVersion {
        let backend = Arc::new(BackendArg::new_raw(
            format!("jar-{}", tool_name.replace('/', "-")),
            Some(format!("jar:{tool_name}")),
            tool_name.to_string(),
            None,
            BackendResolution::new(true),
        ));
        let request = ToolRequest::Version {
            backend,
            version: version.to_string(),
            options: ToolVersionOptions::default(),
            source: ToolSource::Argument,
        };
        ToolVersion::new(request, version.to_string())
    }

    #[test]
    fn maven_coords_parse() {
        assert_eq!(
            MavenCoords::parse("com.squareup.wire/wire-compiler"),
            Some(MavenCoords {
                group_id: "com.squareup.wire".to_string(),
                artifact_id: "wire-compiler".to_string(),
            })
        );
        assert_eq!(MavenCoords::parse("elasticmq"), None);
        assert_eq!(MavenCoords::parse("a/b/c"), None);
        assert_eq!(MavenCoords::parse("/artifact"), None);
        assert_eq!(MavenCoords::parse("group/"), None);
    }

    #[test]
    fn maven_urls() {
        let coords = MavenCoords::parse("com.squareup.wire/wire-compiler").unwrap();
        assert_eq!(
            coords.metadata_url(DEFAULT_REPOSITORY),
            "https://repo1.maven.org/maven2/com/squareup/wire/wire-compiler/maven-metadata.xml"
        );
        assert_eq!(
            coords.jar_url(DEFAULT_REPOSITORY, "5.5.0", None),
            "https://repo1.maven.org/maven2/com/squareup/wire/wire-compiler/5.5.0/wire-compiler-5.5.0.jar"
        );
        assert_eq!(
            coords.jar_url(DEFAULT_REPOSITORY, "5.5.0", Some("jar-with-dependencies")),
            "https://repo1.maven.org/maven2/com/squareup/wire/wire-compiler/5.5.0/wire-compiler-5.5.0-jar-with-dependencies.jar"
        );
        // Trailing slash on a custom repository is tolerated.
        assert_eq!(
            coords.jar_url("https://example.com/releases/", "1.0", None),
            "https://example.com/releases/com/squareup/wire/wire-compiler/1.0/wire-compiler-1.0.jar"
        );
    }

    #[test]
    fn artifact_url_prefers_url_option_and_templates_version() {
        let backend = jar_backend("elasticmq");
        let tv = jar_test_tv("elasticmq", "1.7.1");
        let raw_opts = parse_tool_options(
            r#"url="https://github.com/softwaremill/elasticmq/releases/download/v{{version}}/elasticmq-server-all-{{version}}.jar""#,
        );
        let opts = JarOptions::new(&raw_opts);
        assert_eq!(
            backend.artifact_url(&opts, &tv).unwrap(),
            "https://github.com/softwaremill/elasticmq/releases/download/v1.7.1/elasticmq-server-all-1.7.1.jar"
        );
    }

    #[test]
    fn artifact_url_from_maven_coords() {
        let backend = jar_backend("com.facebook/ktfmt");
        let tv = jar_test_tv("com.facebook/ktfmt", "0.64");
        let raw_opts = parse_tool_options(r#"classifier="with-dependencies""#);
        let opts = JarOptions::new(&raw_opts);
        assert_eq!(
            backend.artifact_url(&opts, &tv).unwrap(),
            "https://repo1.maven.org/maven2/com/facebook/ktfmt/0.64/ktfmt-0.64-with-dependencies.jar"
        );
    }

    #[test]
    fn artifact_url_requires_coords_or_url() {
        let backend = jar_backend("elasticmq");
        let tv = jar_test_tv("elasticmq", "1.7.1");
        let raw_opts = ToolVersionOptions::default();
        let opts = JarOptions::new(&raw_opts);
        let err = backend.artifact_url(&opts, &tv).unwrap_err();
        assert!(err.to_string().contains("Maven coordinates"), "{err}");
    }

    #[test]
    fn bin_name_defaults_and_override() {
        let maven = jar_backend("com.squareup.wire/wire-compiler");
        let no_opts = ToolVersionOptions::default();
        assert_eq!(
            maven.bin_name(&JarOptions::new(&no_opts)).unwrap(),
            "wire-compiler"
        );

        let url_mode = jar_backend("elasticmq");
        assert_eq!(
            url_mode.bin_name(&JarOptions::new(&no_opts)).unwrap(),
            "elasticmq"
        );

        let raw_opts = parse_tool_options(r#"bin="wire""#);
        assert_eq!(maven.bin_name(&JarOptions::new(&raw_opts)).unwrap(), "wire");

        // Launcher names are joined onto bin/, so path traversal is rejected.
        let evil = parse_tool_options(r#"bin="../evil""#);
        assert!(maven.bin_name(&JarOptions::new(&evil)).is_err());
    }

    #[test]
    fn unix_launcher_contents() {
        let backend = jar_backend("com.squareup.wire/wire-compiler");
        let tv = jar_test_tv("com.squareup.wire/wire-compiler", "5.5.0");
        let script = backend.unix_launcher(&tv, "wire-compiler", "");
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("lib/wire-compiler.jar"));
        assert!(script.contains(r#"-jar "$jar" "$@""#));
        assert!(script.contains("JAVA_HOME"));
        // No java_args: no doubled spaces before JAVA_OPTS.
        assert!(script.contains(r#"exec "$java_bin" ${JAVA_OPTS:-} -jar"#));

        let script = backend.unix_launcher(&tv, "wire-compiler", "-Xmx512m");
        assert!(script.contains(r#"exec "$java_bin" -Xmx512m ${JAVA_OPTS:-} -jar"#));
    }

    #[test]
    fn windows_launcher_contents() {
        let backend = jar_backend("com.facebook/ktfmt");
        let tv = jar_test_tv("com.facebook/ktfmt", "0.64");
        let script = backend.windows_launcher(&tv, "ktfmt", "-Xmx512m");
        assert!(script.starts_with("@echo off\r\n"));
        assert!(script.contains(r"lib\ktfmt.jar"));
        assert!(script.contains(r#"-Xmx512m %JAVA_OPTS% -jar "%JAR%" %*"#));
    }
}
