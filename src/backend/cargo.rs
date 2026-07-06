use std::collections::BTreeMap;
use std::path::PathBuf;
use std::{fmt::Debug, fs, sync::Arc};

use async_trait::async_trait;
use color_eyre::Section;
use eyre::{Context, bail, eyre};
use url::Url;
use walkdir::WalkDir;

use crate::Result;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::get_filename_from_url;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::GITHUB_TOKEN;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::http::HTTP;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions, Toolset};

#[derive(Debug)]
pub struct CargoBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct CargoOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> CargoOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn bin(&self) -> Option<String> {
        self.values.platform_string("bin")
    }

    fn bin_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("bin", target)
    }

    fn locked(&self) -> bool {
        self.values
            .raw()
            .get_string("locked")
            .is_none_or(|v| v.to_lowercase() != "false")
    }

    fn features(&self) -> Option<String> {
        self.values.raw().get_string("features")
    }

    fn default_features_disabled(&self) -> bool {
        self.values
            .raw()
            .get_string("default-features")
            .is_some_and(|v| v.to_lowercase() == "false")
    }

    fn crate_arg(&self) -> Option<String> {
        self.values.raw().get_string("crate")
    }

    fn lockfile_options(&self, target: &PlatformTarget) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        for key in ["features", "default-features", "crate", "locked"] {
            if let Some(value) = self.values.raw().get_string(key) {
                result.insert(key.to_string(), value.to_string());
            }
        }
        if let Some(bin) = self.bin_for_target(target) {
            result.insert("bin".to_string(), bin);
        }
        result
    }
}

#[derive(Debug)]
enum BinstallStatus {
    Enabled(PathBuf),
    Disabled,
    Unavailable,
    UnsupportedOptions(Vec<&'static str>),
}

#[async_trait]
impl Backend for CargoBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Cargo
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["rust"])
    }

    fn get_optional_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["cargo-binstall", "sccache"])
    }

    /// Cargo installs packages from crates.io using version specs (e.g., ripgrep@14.0.0).
    /// It doesn't support installing from direct URLs, so lockfile URLs are not applicable.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        if self.git_url().is_some() {
            // TODO: maybe fetch tags/branches from git?
            return Ok(vec![VersionInfo {
                version: "HEAD".into(),
                ..Default::default()
            }]);
        }

        // Use crates.io API which includes created_at timestamps
        let url = format!(
            "https://crates.io/api/v1/crates/{}/versions",
            self.tool_name()
        );
        let response: CratesIoVersionsResponse = HTTP_FETCH.json(&url).await?;

        let versions = response
            .versions
            .into_iter()
            .filter(|v| !v.yanked)
            .map(|v| VersionInfo {
                version: v.num,
                created_at: Some(v.created_at),
                ..Default::default()
            })
            .rev() // API returns newest first, we want oldest first
            .collect();

        Ok(versions)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        // Check if cargo is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "cargo",
            &["rust", "cargo"],
            "To use cargo packages with mise, you need to install Rust first:\n\
              mise use rust@latest\n\n\
            Or install Rust via https://rustup.rs/",
        )
        .await;

        let config = ctx.config.clone();
        let install_arg = format!("{}@{}", self.tool_name(), tv.version);
        let registry_name = &Settings::get().cargo.registry_name;

        let cmd = CmdLineRunner::new("cargo").arg("install");
        let mut cmd = if let Some(url) = self.git_url() {
            let mut cmd = cmd.arg(format!("--git={url}"));
            if let Some(rev) = tv.version.strip_prefix("rev:") {
                cmd = cmd.arg(format!("--rev={rev}"));
            } else if let Some(branch) = tv.version.strip_prefix("branch:") {
                cmd = cmd.arg(format!("--branch={branch}"));
            } else if let Some(tag) = tv.version.strip_prefix("tag:") {
                cmd = cmd.arg(format!("--tag={tag}"));
            } else if tv.version != "HEAD" {
                Err(eyre!("Invalid cargo git version: {}", tv.version).note(
                    r#"You can specify "rev:", "branch:", or "tag:", e.g.:
      * mise use cargo:eza-community/eza@tag:v0.18.0
      * mise use cargo:eza-community/eza@branch:main"#,
                ))?;
            }
            cmd
        } else {
            match self.binstall_status(&config, Some(&ctx.ts), &tv).await {
                BinstallStatus::Enabled(cargo_binstall) => {
                    let mut cmd = CmdLineRunner::new(cargo_binstall).arg("-y");
                    if let Some(token) = &*GITHUB_TOKEN {
                        cmd = cmd.env("GITHUB_TOKEN", token)
                    }
                    cmd.arg(install_arg)
                }
                BinstallStatus::UnsupportedOptions(options)
                    if Settings::get().cargo.binstall_only =>
                {
                    let options = format_tool_options(&options);
                    bail!(
                        "cargo-binstall cannot honor cargo install-only tool option(s): {options}\n\
                        hint: Remove the option(s), or disable cargo.binstall_only to allow cargo install"
                    );
                }
                BinstallStatus::Disabled if Settings::get().cargo.binstall_only => {
                    bail!("cargo-binstall is disabled, but cargo.binstall_only is set");
                }
                BinstallStatus::Unavailable
                    if Settings::get().experimental
                        && Settings::get().cargo.binstall_native
                        && !Settings::get().cargo.binstall_only
                        && self.native_binstall(ctx, &tv).await? =>
                {
                    return Ok(tv.clone());
                }
                _ if Settings::get().cargo.binstall_only => {
                    bail!("cargo-binstall is not available, but cargo.binstall_only is set");
                }
                BinstallStatus::UnsupportedOptions(options) => {
                    let options = format_tool_options(&options);
                    info!(
                        "not using cargo-binstall because cargo install-only tool option(s) are specified: {options}"
                    );
                    cmd.arg(install_arg)
                }
                _ => cmd.arg(install_arg),
            }
        };

        let request_options = tv.request.options();
        let opts = CargoOptions::new(&request_options);
        if let Some(bin) = opts.bin() {
            cmd = cmd.arg(format!("--bin={bin}"));
        }
        if opts.locked() {
            cmd = cmd.arg("--locked");
        }
        if let Some(features) = opts.features() {
            cmd = cmd.arg(format!("--features={features}"));
        }
        if opts.default_features_disabled() {
            cmd = cmd.arg("--no-default-features");
        }
        if let Some(c) = opts.crate_arg() {
            cmd = cmd.arg(c);
        }
        if let Some(registry_name) = registry_name {
            cmd = cmd.arg(format!("--registry={registry_name}"));
        }

        cmd.arg("--root")
            .arg(tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(ctx.ts.env_with_path_without_tools(&ctx.config).await?)
            .envs(tv.install_env())
            .prepend_path(ctx.ts.list_paths(&ctx.config).await)?
            .prepend_path(
                self.dependency_toolset(&ctx.config)
                    .await?
                    .list_paths(&ctx.config)
                    .await,
            )?
            .execute()?;

        Ok(tv.clone())
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let opts = request.options();
        Ok(CargoOptions::new(&opts).lockfile_options(target))
    }
}

/// Returns install-time-only option keys for Cargo backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "features".into(),
        "default-features".into(),
        "bin".into(),
        "crate".into(),
        "locked".into(),
    ]
}

impl CargoBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn cargo_install_required_options(opts: &ToolVersionOptions) -> Vec<&'static str> {
        let mut options = vec![];
        if opts
            .get_string("features")
            .is_some_and(|features| !features.trim().is_empty())
        {
            options.push("features");
        }
        if opts
            .get_string("default-features")
            .is_some_and(|default_features| default_features.to_lowercase() == "false")
        {
            options.push("default-features");
        }
        options
    }

    async fn binstall_status(
        &self,
        config: &Arc<Config>,
        ts: Option<&Toolset>,
        tv: &ToolVersion,
    ) -> BinstallStatus {
        if !Settings::get().cargo.binstall {
            return BinstallStatus::Disabled;
        }
        let opts = tv.request.options();
        let cargo_install_required_options = Self::cargo_install_required_options(&opts);
        if !cargo_install_required_options.is_empty() {
            return BinstallStatus::UnsupportedOptions(cargo_install_required_options);
        }
        if let Some(cargo_binstall) = self
            .dependency_path_for_install(config, ts, "cargo-binstall")
            .await
        {
            return BinstallStatus::Enabled(cargo_binstall);
        }
        BinstallStatus::Unavailable
    }

    async fn native_binstall(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<bool> {
        match self.native_binstall_result(ctx, tv).await {
            Ok(true) => Ok(true),
            Ok(false) => Ok(false),
            Err(err) => {
                debug!("native cargo binary install unavailable: {err:#}");
                Ok(false)
            }
        }
    }

    async fn native_binstall_result(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<bool> {
        let request_options = tv.request.options();
        let opts = CargoOptions::new(&request_options);
        let version = tv.version.as_str();
        let crate_name = opts.crate_arg().unwrap_or_else(|| self.tool_name());
        let package = CratesIoPackage::fetch(&crate_name, version).await?;
        let target = cargo_target_triple(&PlatformTarget::from_current())
            .ok_or_else(|| eyre!("unsupported platform for native cargo binary install"))?;

        let manifest = download_crate_manifest(&package, tv, ctx).await?;
        let Some(binstall) = NativeBinstallMetadata::from_manifest(&manifest, &target)? else {
            return Ok(false);
        };

        let bins = match opts.bin() {
            Some(bin) => vec![bin],
            None if !package.bin_names.is_empty() => package.bin_names.clone(),
            None => vec![crate_name.clone()],
        };
        let pkg_fmt = binstall.pkg_fmt.unwrap_or_else(|| "tgz".to_string());
        let archive_format = archive_format(&pkg_fmt)?;
        let binary_ext = if cfg!(windows) { ".exe" } else { "" };
        let archive_url = expand_binstall_template(
            &binstall.pkg_url,
            &BinstallTemplateVars {
                repo: package.repository.as_deref().unwrap_or_default(),
                name: &crate_name,
                version,
                target: &target,
                archive_format: &archive_format,
                archive_suffix: &format!(".{archive_format}"),
                bin: bins.first().map(String::as_str).unwrap_or(&crate_name),
                binary_ext,
            },
        );

        let archive_path = tv.download_path().join(get_filename_from_url(&archive_url));
        ctx.pr.set_message(format!(
            "download {}",
            archive_path.file_name().unwrap().to_string_lossy()
        ));
        HTTP.download_file(&archive_url, &archive_path, Some(ctx.pr.as_ref()))
            .await?;

        let extract_dir = tempfile::Builder::new()
            .prefix("mise-cargo-binstall-")
            .tempdir()?;
        ctx.pr.next_operation();
        file::extract_archive(
            &archive_path,
            extract_dir.path(),
            ExtractionFormat::from_file_name(&archive_path.to_string_lossy()),
            &ExtractOptions {
                strip_components: 0,
                pr: Some(ctx.pr.as_ref()),
                preserve_mtime: true,
            },
        )?;

        let bin_root = tv.install_path().join("bin");
        file::create_dir_all(&bin_root)?;
        for bin in &bins {
            let src = find_native_bin(
                extract_dir.path(),
                binstall.bin_dir.as_deref(),
                bin,
                binary_ext,
            )?;
            let dest = bin_root.join(format!("{bin}{binary_ext}"));
            fs::copy(&src, &dest).wrap_err_with(|| {
                format!(
                    "failed to copy native cargo binary {} to {}",
                    file::display_path(&src),
                    file::display_path(&dest)
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&dest)?.permissions();
                perms.set_mode(perms.mode() | 0o755);
                fs::set_permissions(&dest, perms)?;
            }
        }
        info!("installed {crate_name}@{version} from native cargo binary artifact");
        Ok(true)
    }

    /// if the name is a git repo, return the git url
    fn git_url(&self) -> Option<Url> {
        if let Ok(url) = Url::parse(&self.tool_name()) {
            Some(url)
        } else if let Some((user, repo)) = self.tool_name().split_once('/') {
            format!("https://github.com/{user}/{repo}.git").parse().ok()
        } else {
            None
        }
    }
}

fn format_tool_options(options: &[&'static str]) -> String {
    options
        .iter()
        .map(|option| format!("`{option}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoPackageResponse {
    version: CratesIoPackage,
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoPackage {
    #[serde(rename = "crate")]
    name: String,
    num: String,
    dl_path: String,
    repository: Option<String>,
    #[serde(default)]
    bin_names: Vec<String>,
}

impl CratesIoPackage {
    async fn fetch(crate_name: &str, version: &str) -> Result<Self> {
        let url = format!("https://crates.io/api/v1/crates/{crate_name}/{version}");
        let response: CratesIoPackageResponse = HTTP_FETCH.json(&url).await?;
        Ok(response.version)
    }

    fn download_url(&self) -> String {
        format!("https://crates.io{}", self.dl_path)
    }
}

#[derive(Debug, Default)]
struct NativeBinstallMetadata {
    pkg_url: String,
    bin_dir: Option<String>,
    pkg_fmt: Option<String>,
}

impl NativeBinstallMetadata {
    fn from_manifest(manifest: &toml::Value, target: &str) -> Result<Option<Self>> {
        let Some(root) = manifest
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("binstall"))
            .and_then(toml::Value::as_table)
        else {
            return Ok(None);
        };

        let mut merged = root.clone();
        if let Some(target_override) = root
            .get("overrides")
            .and_then(|o| o.get(target))
            .and_then(toml::Value::as_table)
        {
            for (key, value) in target_override {
                merged.insert(key.clone(), value.clone());
            }
        }

        let Some(pkg_url) = merged.get("pkg-url").and_then(toml::Value::as_str) else {
            return Ok(None);
        };

        Ok(Some(Self {
            pkg_url: pkg_url.to_string(),
            bin_dir: merged
                .get("bin-dir")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            pkg_fmt: merged
                .get("pkg-fmt")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
        }))
    }
}

struct BinstallTemplateVars<'a> {
    repo: &'a str,
    name: &'a str,
    version: &'a str,
    target: &'a str,
    archive_format: &'a str,
    archive_suffix: &'a str,
    bin: &'a str,
    binary_ext: &'a str,
}

async fn download_crate_manifest(
    package: &CratesIoPackage,
    tv: &ToolVersion,
    ctx: &InstallContext,
) -> Result<toml::Value> {
    let crate_path = tv
        .download_path()
        .join(format!("{}-{}.crate", package.name, package.num));
    HTTP.download_file(package.download_url(), &crate_path, None)
        .await?;

    let temp_dir = tempfile::Builder::new()
        .prefix("mise-cargo-crate-")
        .tempdir()?;
    file::extract_archive(
        &crate_path,
        temp_dir.path(),
        ExtractionFormat::TarGz,
        &ExtractOptions {
            strip_components: 0,
            pr: Some(ctx.pr.as_ref()),
            preserve_mtime: true,
        },
    )?;

    let manifest_path = temp_dir
        .path()
        .join(format!("{}-{}/Cargo.toml", package.name, package.num));
    let manifest = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("failed to read {}", file::display_path(&manifest_path)))?;
    Ok(toml::from_str(&manifest)?)
}

fn cargo_target_triple(target: &PlatformTarget) -> Option<String> {
    let libc = target.libc().unwrap_or("gnu");
    match (target.os_name(), target.arch_name(), libc) {
        ("macos", "x64", _) => Some("x86_64-apple-darwin".to_string()),
        ("macos", "arm64", _) => Some("aarch64-apple-darwin".to_string()),
        ("windows", "x64", _) => Some("x86_64-pc-windows-msvc".to_string()),
        ("windows", "arm64", _) => Some("aarch64-pc-windows-msvc".to_string()),
        ("linux", "x64", "musl") => Some("x86_64-unknown-linux-musl".to_string()),
        ("linux", "x64", _) => Some("x86_64-unknown-linux-gnu".to_string()),
        ("linux", "arm64", "musl") => Some("aarch64-unknown-linux-musl".to_string()),
        ("linux", "arm64", _) => Some("aarch64-unknown-linux-gnu".to_string()),
        ("linux", "armv7", _) => Some("armv7-unknown-linux-gnueabihf".to_string()),
        _ => None,
    }
}

fn archive_format(pkg_fmt: &str) -> Result<String> {
    match pkg_fmt {
        "tgz" => Ok("tar.gz".to_string()),
        "txz" => Ok("tar.xz".to_string()),
        "tbz" | "tbz2" => Ok("tar.bz2".to_string()),
        "zip" | "tar" => Ok(pkg_fmt.to_string()),
        other => Err(eyre!(
            "unsupported native cargo binary package format: {other}"
        )),
    }
}

fn expand_binstall_template(template: &str, vars: &BinstallTemplateVars<'_>) -> String {
    let replacements = [
        ("repo", vars.repo),
        ("name", vars.name),
        ("version", vars.version),
        ("target", vars.target),
        ("archive-format", vars.archive_format),
        ("archive-suffix", vars.archive_suffix),
        ("bin", vars.bin),
        ("binary-ext", vars.binary_ext),
    ];
    let mut out = template.to_string();
    for (key, value) in replacements {
        out = out.replace(&format!("{{ {key} }}"), value);
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

fn expand_bin_dir(template: &str, bin: &str, binary_ext: &str) -> String {
    expand_binstall_template(
        template,
        &BinstallTemplateVars {
            repo: "",
            name: "",
            version: "",
            target: "",
            archive_format: "",
            archive_suffix: "",
            bin,
            binary_ext,
        },
    )
}

fn find_native_bin(
    extract_dir: &std::path::Path,
    bin_dir: Option<&str>,
    bin: &str,
    binary_ext: &str,
) -> Result<PathBuf> {
    if let Some(bin_dir) = bin_dir {
        let candidate = extract_dir.join(expand_bin_dir(bin_dir, bin, binary_ext));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    let file_name = format!("{bin}{binary_ext}");
    WalkDir::new(extract_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.file_type().is_file() && entry.file_name() == file_name.as_str())
        .map(|entry| entry.into_path())
        .ok_or_else(|| eyre!("native cargo binary artifact did not contain {file_name}"))
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoVersionsResponse {
    versions: Vec<CratesIoVersion>,
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoVersion {
    num: String,
    yanked: bool,
    created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::toolset::parse_tool_options;

    #[test]
    fn test_lockfile_options_uses_target_platform_bin() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("bin".into(), toml::Value::String("base-bin".into()));
        let mut platforms = toml::Table::new();
        let mut linux = toml::Table::new();
        linux.insert("bin".into(), toml::Value::String("linux-bin".into()));
        platforms.insert("linux-x64".into(), toml::Value::Table(linux));
        opts.opts
            .insert("platforms".into(), toml::Value::Table(platforms));

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let lock_opts = CargoOptions::new(&opts).lockfile_options(&target);

        assert_eq!(lock_opts.get("bin").map(String::as_str), Some("linux-bin"));
    }

    #[test]
    fn test_lockfile_options_include_crate_and_locked() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("crate".into(), toml::Value::String("demo".into()));
        opts.opts
            .insert("locked".into(), toml::Value::Boolean(false));

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let lock_opts = CargoOptions::new(&opts).lockfile_options(&target);

        assert_eq!(lock_opts.get("crate").map(String::as_str), Some("demo"));
        assert_eq!(lock_opts.get("locked").map(String::as_str), Some("false"));
    }

    #[test]
    fn cargo_install_required_options_skips_feature_options() {
        let opts = parse_tool_options("features=add,default-features=false");

        assert_eq!(
            CargoBackend::cargo_install_required_options(&opts),
            vec!["features", "default-features"]
        );
    }

    #[test]
    fn cargo_install_required_options_allows_binstall_supported_options() {
        let opts =
            parse_tool_options("bin=cargo-add,crate=cargo-edit,locked=false,default-features=true");

        assert_eq!(
            CargoBackend::cargo_install_required_options(&opts),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn cargo_install_required_options_skips_toml_bool_default_features() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("default-features".into(), toml::Value::Boolean(false));

        assert_eq!(
            CargoBackend::cargo_install_required_options(&opts),
            vec!["default-features"]
        );
    }

    #[test]
    fn native_binstall_metadata_merges_target_overrides() {
        let manifest = toml::Value::Table(toml::toml! {
            [package.metadata.binstall]
            pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }.{ archive-format }"
            bin-dir = "{ bin }{ binary-ext }"
            pkg-fmt = "tgz"

            [package.metadata.binstall.overrides.x86_64-apple-darwin]
            pkg-fmt = "zip"
        });

        let metadata =
            NativeBinstallMetadata::from_manifest(&manifest, "x86_64-apple-darwin").unwrap();
        let metadata = metadata.unwrap();

        assert_eq!(metadata.pkg_fmt.as_deref(), Some("zip"));
        assert_eq!(metadata.bin_dir.as_deref(), Some("{ bin }{ binary-ext }"));
    }

    #[test]
    fn expand_binstall_template_handles_spaced_and_compact_vars() {
        let rendered = expand_binstall_template(
            "{ repo }/{name}-{ version }-{ target }{ archive-suffix }",
            &BinstallTemplateVars {
                repo: "https://example.com",
                name: "demo",
                version: "1.2.3",
                target: "x86_64-unknown-linux-gnu",
                archive_format: "tar.gz",
                archive_suffix: ".tar.gz",
                bin: "demo",
                binary_ext: "",
            },
        );

        assert_eq!(
            rendered,
            "https://example.com/demo-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn archive_format_normalizes_binstall_aliases() {
        assert_eq!(archive_format("tgz").unwrap(), "tar.gz");
        assert_eq!(archive_format("txz").unwrap(), "tar.xz");
        assert_eq!(archive_format("zip").unwrap(), "zip");
        assert!(archive_format("pkg").is_err());
    }
}
