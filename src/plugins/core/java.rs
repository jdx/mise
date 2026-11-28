use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Display;
use std::fs::{self};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::args::BackendArg;
use crate::backend::options::BackendOptions;
use crate::backend::static_helpers::lock_checksum;
use crate::backend::{
    Backend, VersionInfo, normalize_idiomatic_contents, platform_target::PlatformTarget,
};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings, SettingsExt};
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::platform::OS;
use crate::platform::Platform;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{file, plugins};
use async_trait::async_trait;
use color_eyre::eyre::Result;
use indoc::formatdoc;
use itertools::Itertools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::LazyLock as Lazy;
use versions::Versioning;
use xx::regex;

static VERSION_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|-test|snapshot|SNAPSHOT|master)"
    )
        .unwrap()
});

#[derive(Debug)]
pub(super) struct JavaPlugin {
    ba: Arc<BackendArg>,
    java_metadata_ea_cache: CacheManager<HashMap<String, JavaMetadata>>,
    java_metadata_ga_cache: CacheManager<HashMap<String, JavaMetadata>>,
    java_metadata_target_cache:
        tokio::sync::Mutex<HashMap<(String, String), HashMap<String, JavaMetadata>>>,
}

#[derive(Debug, Clone, Copy)]
struct JavaOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> JavaOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn release_type(&self) -> &'a str {
        self.values.str("release_type").unwrap_or("ga")
    }

    fn lockfile_options(
        &self,
        requested_version: &str,
        shorthand_vendor: &str,
    ) -> BTreeMap<String, String> {
        let mut opts = BTreeMap::new();
        let release_type = self.release_type();
        if release_type != "ga" {
            opts.insert("release_type".to_string(), release_type.to_string());
        }
        if is_shorthand_java_request(requested_version) {
            opts.insert("shorthand_vendor".to_string(), shorthand_vendor.to_string());
        }
        opts
    }
}

fn is_shorthand_java_request(requested_version: &str) -> bool {
    !requested_version.contains('-')
}

impl JavaPlugin {
    pub(super) fn new() -> Self {
        Self::from_arg(plugins::core::new_backend_arg("java"))
    }

    /// Build a plugin for `ba`. The shared core instance has no options, so a request
    /// with inline options such as `java[release_type=ea]` needs its own instance for
    /// version listing to see them.
    pub(super) fn from_arg(ba: BackendArg) -> Self {
        let settings = Settings::get();
        let ba = Arc::new(ba);
        Self {
            java_metadata_ea_cache: CacheManagerBuilder::new(
                ba.cache_path().join("java_metadata_ea.msgpack.z"),
            )
            .with_fresh_duration(settings.fetch_remote_versions_cache())
            .build(),
            java_metadata_ga_cache: CacheManagerBuilder::new(
                ba.cache_path().join("java_metadata_ga.msgpack.z"),
            )
            .with_fresh_duration(settings.fetch_remote_versions_cache())
            .build(),
            java_metadata_target_cache: tokio::sync::Mutex::new(HashMap::new()),
            ba,
        }
    }

    /// Metadata keyed by full version string (`<vendor>-<version>`). The cache does not
    /// depend on `java.shorthand_vendor`; shorthand versions are added when listing.
    async fn fetch_java_metadata(
        &self,
        release_type: &str,
    ) -> Result<&HashMap<String, JavaMetadata>> {
        let cache = if release_type == "ea" {
            &self.java_metadata_ea_cache
        } else {
            &self.java_metadata_ga_cache
        };
        let release_type = release_type.to_string();
        cache
            .get_or_try_init_async(async || {
                let platform = current_java_platform();
                let mut metadata = HashMap::new();

                for m in self
                    .download_java_metadata(&release_type, &platform)
                    .await?
                {
                    Self::insert_java_metadata(&mut metadata, m, &platform);
                }

                Ok(metadata)
            })
            .await
    }

    async fn fetch_java_metadata_for_target(
        &self,
        release_type: &str,
        target: &PlatformTarget,
    ) -> Result<HashMap<String, JavaMetadata>> {
        if target.platform == current_java_platform() {
            return Ok(self.fetch_java_metadata(release_type).await?.clone());
        }

        let cache_key = (release_type.to_string(), target.to_key());
        if let Some(metadata) = self
            .java_metadata_target_cache
            .lock()
            .await
            .get(&cache_key)
            .cloned()
        {
            return Ok(metadata);
        }

        let mut metadata = HashMap::new();

        for m in self
            .download_java_metadata(release_type, &target.platform)
            .await?
        {
            Self::insert_java_metadata(&mut metadata, m, &target.platform);
        }

        self.java_metadata_target_cache
            .lock()
            .await
            .insert(cache_key, metadata.clone());

        Ok(metadata)
    }

    fn insert_java_metadata(
        metadata: &mut HashMap<String, JavaMetadata>,
        m: JavaMetadata,
        platform: &Platform,
    ) {
        metadata.insert(m.to_version_string(platform), m);
    }

    fn java_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/java")
    }

    fn test_java(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        CmdLineRunner::new(self.java_bin(tv))
            .with_pr(pr)
            // `java -version` writes its normal version banner to stderr. Treat
            // it as ordinary process output so an interactive install can fold
            // it into the tool's progress row instead of printing three lines.
            .stderr_as_stdout()
            .env("JAVA_HOME", tv.install_path())
            .env_values(tv.install_env())
            .arg("-version")
            .execute()
    }

    async fn download(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pr: &dyn SingleReport,
        m: &JavaMetadata,
    ) -> Result<PathBuf> {
        let filename = m.url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        HTTP.download_file_stored(
            &m.url,
            &tarball_path,
            lock_checksum(tv, &self.get_platform_key(), &m.url).as_deref(),
            Some(pr),
        )
        .await?;

        let platform_key = self.get_platform_key();
        if !tv.lock_platforms.contains_key(&platform_key) {
            let platform_info = tv.lock_platforms.entry(platform_key).or_default();
            platform_info.url = Some(m.url.clone());
            if m.checksum.is_some() {
                platform_info.checksum = m.checksum.clone();
            }
        }
        self.verify_checksum(ctx, tv, &tarball_path)?;

        Ok(tarball_path)
    }

    fn install(
        &self,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        tarball_path: &Path,
        m: &JavaMetadata,
    ) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        pr.set_message(format!("extract {filename}"));
        let format = m
            .file_type
            .as_deref()
            .and_then(ExtractionFormat::from_ext)
            .unwrap_or_else(|| ExtractionFormat::from_file_name(&filename));
        file::extract_archive(
            tarball_path,
            &tv.download_path(),
            format,
            &ExtractOptions {
                pr: Some(pr),
                ..Default::default()
            },
        )?;
        self.move_to_install_path(tv, m)
    }

    fn move_to_install_path(&self, tv: &ToolVersion, m: &JavaMetadata) -> Result<()> {
        let basedir = tv
            .download_path()
            .read_dir()?
            .find(|e| e.as_ref().unwrap().file_type().unwrap().is_dir())
            .unwrap()?
            .path();
        let contents_dir = basedir.join("Contents");
        let contents_home_dir = contents_dir.join("Home");
        let source_dir = if cfg!(target_os = "macos") && contents_home_dir.is_dir() {
            contents_home_dir
        } else {
            basedir
        };
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path())?;
        for entry in fs::read_dir(source_dir)? {
            let entry = entry?;
            let dest = tv.install_path().join(entry.file_name());
            trace!("moving {:?} to {:?}", entry.path(), &dest);
            file::move_file(entry.path(), dest)?;
        }

        if cfg!(target_os = "macos") {
            self.handle_macos_integration(&contents_dir, tv, m)?;
        }

        Ok(())
    }

    fn handle_macos_integration(
        &self,
        contents_dir: &Path,
        tv: &ToolVersion,
        m: &JavaMetadata,
    ) -> Result<()> {
        // move Contents dir to install path for macOS, if it exists
        if contents_dir.exists() {
            file::create_dir_all(tv.install_path().join("Contents"))?;
            for entry in fs::read_dir(contents_dir)? {
                let entry = entry?;
                // skip Home dir, so we can symlink it later
                if entry.file_name() == "Home" {
                    continue;
                }
                let dest = tv.install_path().join("Contents").join(entry.file_name());
                trace!("moving {:?} to {:?}", entry.path(), &dest);
                file::move_file(entry.path(), dest)?;
            }
            file::make_symlink(
                tv.install_path().as_path(),
                &tv.install_path().join("Contents").join("Home"),
            )?;
        }

        // if vendor is Zulu, symlink zulu-{major_version}.jdk/Contents to install path for macOS
        if m.vendor.as_str() == "zulu" {
            let major_version = m.version.split('.').next().unwrap();
            let contents_symlink_path = tv.install_path().join("Contents");
            let zulu_contents_path = tv
                .install_path()
                .join(format!("zulu-{major_version}.jdk"))
                .join("Contents");
            if zulu_contents_path.exists() && !contents_symlink_path.exists() {
                file::make_symlink(zulu_contents_path.as_path(), &contents_symlink_path)?;
            }
        }

        if tv.install_path().join("Contents").exists() {
            info!(
                "{}",
                formatdoc! {r#"
                To enable macOS integration, run the following commands:
                sudo mkdir /Library/Java/JavaVirtualMachines/{version}.jdk
                sudo ln -s {path}/Contents /Library/Java/JavaVirtualMachines/{version}.jdk/Contents
                "#,
                    version = tv.version,
                    path = tv.install_path().display(),
                }
            );
        }
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("java -version".into());
        self.test_java(tv, pr)
    }

    fn tv_release_type(&self, tv: &ToolVersion) -> String {
        let raw_opts = tv.request.options();
        JavaOptions::new(&raw_opts).release_type().to_string()
    }

    async fn tv_to_metadata(&self, tv: &ToolVersion) -> Result<&JavaMetadata> {
        let release_type = self.tv_release_type(tv);
        let metadata = self.fetch_java_metadata(&release_type).await?;
        let locked_url = tv
            .lock_platforms
            .get(&self.get_platform_key())
            .and_then(|p| p.url.as_deref());
        let v = java_metadata_key(
            metadata,
            &tv.version,
            &Settings::get().java.shorthand_vendor,
            |m| locked_url == Some(m.url.as_str()) || is_openjdk_install(&tv.install_path()),
        );
        find_java_metadata(metadata, &v, &tv.version, &current_java_platform())
    }

    async fn download_java_metadata(
        &self,
        release_type: &str,
        platform: &Platform,
    ) -> Result<Vec<JavaMetadata>> {
        let url = format!(
            "https://mise-java.jdx.dev/jvm/{}/{}/{}.json",
            release_type,
            java_os(platform),
            java_arch(platform)
        );

        let metadata = HTTP_FETCH
            .json::<Vec<JavaMetadata>, _>(url)
            .await?
            .into_iter()
            .filter(|m| {
                m.file_type
                    .as_ref()
                    .is_some_and(|file_type| java_file_type_supported(platform, file_type))
            })
            .collect();
        Ok(metadata)
    }

    async fn list_remote_versions_for_options(
        &self,
        opts: &ToolVersionOptions,
    ) -> Result<Vec<VersionInfo>> {
        let release_type = JavaOptions::new(opts).release_type().to_string();
        let metadata = self.fetch_java_metadata(&release_type).await?;
        let versions = with_shorthand_versions(metadata, &Settings::get().java.shorthand_vendor)
            .into_iter()
            .sorted_by_cached_key(|(v, m)| {
                let is_shorthand = regex!(r"^\d").is_match(v);
                let vendor = &m.vendor;
                let is_jdk = match is_shorthand {
                    true => true,
                    false => m
                        .image_type
                        .as_ref()
                        .is_some_and(|image_type| image_type == "jdk"),
                };
                let features = 10 - m.features.as_ref().map_or(0, |f| f.len());
                let version = Versioning::new(&m.version);
                // Extract build suffix after a '+', '.' if present. If not present, treat as 0.
                let build_num = m
                    .version
                    .rsplit_once('+')
                    .or_else(|| m.version.rsplit_once('.'))
                    .and_then(|(_, tail)| {
                        // take leading digits of tail
                        let digits: String =
                            tail.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if digits.is_empty() {
                            None
                        } else {
                            u64::from_str(&digits).ok()
                        }
                    })
                    .unwrap_or(0u64);
                // Prefer base vendors (no dashes) over specialized variants like
                // "liberica-nik". Fewer dashes → more canonical → sorts later.
                let vendor_dashes = -(vendor.chars().filter(|c| *c == '-').count() as i32);
                (
                    is_shorthand,
                    vendor_dashes,
                    vendor,
                    is_jdk,
                    features,
                    version,
                    build_num,
                    v.to_string(),
                )
            })
            .map(|(v, m)| VersionInfo {
                version: v.to_string(),
                created_at: m.created_at.clone(),
                // The regex is a denylist heuristic, not a total grammar:
                // a match proves "prerelease", a miss proves nothing.
                prerelease: VERSION_REGEX.is_match(v).then_some(true),
                ..Default::default()
            })
            .unique_by(|v| v.version.clone())
            .collect();

        Ok(versions)
    }
}

#[async_trait]
impl Backend for JavaPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn option_spec(&self) -> Option<crate::backend::OptionSpec> {
        Some(crate::backend::OptionSpec::warn_only(&[
            "tool_core_java_options",
        ]))
    }

    fn include_prereleases(&self, _opts: &ToolVersionOptions) -> bool {
        // Java selects early-access metadata with `release_type = "ea"`; the
        // unrelated generic `prerelease` option has never applied here.
        false
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &["release_type"]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        self.list_remote_versions_for_options(&raw_opts).await
    }

    /// Override to bypass the shared remote_versions cache since Java has
    /// separate caches for GA and EA release types in `fetch_java_metadata`.
    /// The underlying fetch already handles freshness, so the `_refresh` flag
    /// is irrelevant.
    async fn list_remote_versions_with_info_and_options(
        &self,
        _config: &Arc<Config>,
        _listing_opts: &ToolVersionOptions,
        selection_opts: &ToolVersionOptions,
        _refresh: bool,
        _has_local_version_listing_override: bool,
    ) -> Result<Vec<VersionInfo>> {
        self.list_remote_versions_for_options(selection_opts).await
    }

    /// "temurin-21.0.13" and "21.0.13" are prefixes of builds such as "21.0.13+11", so
    /// only the catalog can tell whether a dotted version names one build.
    fn dotted_version_names_a_release(&self, _version: &str) -> bool {
        false
    }

    /// Versions converted from `.sdkmanrc` that name a build by something other than its
    /// catalog version. See [`sdkman_catalog_match`].
    async fn resolve_exact_version(
        &self,
        _config: &Arc<Config>,
        version: &str,
    ) -> eyre::Result<Option<String>> {
        let Some(query) = SdkmanQuery::parse(version) else {
            return Ok(None);
        };
        let metadata = self.fetch_java_metadata("ga").await?;
        let platform = current_java_platform();
        let matches = sdkman_catalog_match(metadata, &query, &platform);
        // The catalog listing's order, which picks the same build `ls-remote` lists last.
        let listed = self
            .list_remote_versions_for_options(&ToolVersionOptions::default())
            .await?;
        match listed.into_iter().rfind(|v| matches.contains(&v.version)) {
            Some(v) => Ok(Some(v.version)),
            None => Err(eyre::eyre!(
                "no {} build of Java {} found for {}",
                query.prefix,
                query.version,
                platform.to_key()
            )),
        }
    }

    fn list_installed_versions_matching(&self, query: &str) -> Vec<String> {
        let versions = self.list_installed_versions();
        if let Some(query) = SdkmanQuery::parse(query) {
            // A build in a shared install dir is listed too, so read its
            // `release` file from where it is, as `ToolVersion::install_path` does.
            let installs_path = self.ba().installs_path();
            let tool_dir_name = self.ba().tool_dir_name();
            return sdkman_installed_match(versions, &query, |version| {
                crate::env::find_in_shared_installs(
                    installs_path.join(version),
                    &tool_dir_name,
                    version,
                )
            });
        }
        // Java doesn't support the `prerelease` opt-in; always filter.
        self.fuzzy_match_filter(versions, query, true)
    }

    async fn list_versions_matching(
        &self,
        config: &Arc<Config>,
        query: &str,
    ) -> eyre::Result<Vec<String>> {
        let versions = self.list_remote_versions(config).await?;
        Ok(self.fuzzy_match_filter(versions, query, true))
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        let aliases = BTreeMap::from([("lts".into(), "25".into())]);
        Ok(aliases)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        Ok(JavaOptions::new(&raw_opts)
            .lockfile_options(&request.version(), &Settings::get().java.shorthand_vendor))
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let release_type = self.tv_release_type(tv);
        let metadata = self
            .fetch_java_metadata_for_target(&release_type, target)
            .await?;
        let locked_url = tv
            .lock_platforms
            .get(&target.to_key())
            .and_then(|p| p.url.as_deref());
        let version = java_metadata_key(
            &metadata,
            &tv.version,
            &Settings::get().java.shorthand_vendor,
            |m| locked_url == Some(m.url.as_str()) || is_openjdk_install(&tv.install_path()),
        );
        let m = find_java_metadata(&metadata, &version, &tv.version, &target.platform)?;

        Ok(PlatformInfo {
            checksum: m.checksum.clone(),
            size: None,
            url: Some(m.url.clone()),
            url_api: None,
            conda_deps: None,
            ..Default::default()
        })
    }

    async fn _parse_idiomatic_file(&self, path: &Path) -> Result<Vec<String>> {
        let contents = file::read_to_string(path)?;
        // The `.sdkmanrc` branch matches on the start of a line without going through
        // `normalize_idiomatic_contents`, so a leading mark defeats `starts_with("java")` and the
        // fallback yields an empty version.
        let contents = file::strip_utf8_bom(&contents);
        if path.file_name() == Some(".sdkmanrc".as_ref()) {
            let version = contents
                .lines()
                .find(|l| l.starts_with("java"))
                .unwrap_or("java=")
                .split_once('=')
                .unwrap_or_default()
                .1;
            Ok(vec![sdkman_java_version(version.trim())])
        } else {
            Ok(normalize_idiomatic_contents(contents)
                .lines()
                .map(|s| s.to_string())
                .collect())
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        // Check if URL already exists in lockfile platforms first
        let platform_key = self.get_platform_key();
        let (metadata, tarball_path) =
            if let Some(platform_info) = tv.lock_platforms.get(&platform_key) {
                if let Some(url) = platform_info.url.clone() {
                    // Use the filename from the URL, not the platform key
                    let filename = url.split('/').next_back().unwrap();
                    debug!("Using existing URL from lockfile for {}: {}", filename, url);
                    let tarball_path = tv.download_path().join(filename);

                    // If the file does not exist, download using the lockfile URL
                    if !tarball_path.exists() {
                        debug!("File not found, downloading from cached URL: {}", url);
                        // Download using the lockfile URL, not JavaMetadata
                        let pinned = lock_checksum(&tv, &platform_key, &url);
                        HTTP.download_file_stored(
                            &url,
                            &tarball_path,
                            pinned.as_deref(),
                            Some(ctx.pr.as_ref()),
                        )
                        .await?;
                    } else {
                        ctx.pr.set_message(format!("cached {filename}"));
                    }
                    // A file that was already there is checked too, and one that fails is not
                    // kept: with always_keep_download the next attempt would find it here.
                    if let Err(err) = self.verify_checksum(ctx, &mut tv, &tarball_path) {
                        file::remove_file(&tarball_path)?;
                        return Err(err);
                    }

                    // The lockfile already names the build, so the catalog is not consulted.
                    (
                        JavaMetadata::from_locked(
                            &tv.version,
                            &url,
                            &Settings::get().java.shorthand_vendor,
                        ),
                        tarball_path,
                    )
                } else {
                    // No URL in lockfile, fall back to metadata
                    let metadata = self.tv_to_metadata(&tv).await?;
                    let tarball_path = self
                        .download(ctx, &mut tv, ctx.pr.as_ref(), metadata)
                        .await?;
                    (metadata.clone(), tarball_path)
                }
            } else {
                let metadata = self.tv_to_metadata(&tv).await?;
                let tarball_path = self
                    .download(ctx, &mut tv, ctx.pr.as_ref(), metadata)
                    .await?;
                (metadata.clone(), tarball_path)
            };

        ctx.pr.next_operation();
        self.install(&tv, ctx.pr.as_ref(), &tarball_path, &metadata)?;
        ctx.pr.next_operation();
        self.verify(&tv, ctx.pr.as_ref())?;

        Ok(tv)
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let map = BTreeMap::from([(
            "JAVA_HOME".into(),
            tv.install_path().to_string_lossy().into(),
        )]);
        Ok(map)
    }

    /// A `-musl` query matches versions without the suffix, which the shared
    /// matcher would not select.
    fn fuzzy_match_stays_within_shared_matcher(&self) -> bool {
        false
    }

    fn fuzzy_match_filter(
        &self,
        versions: Vec<String>,
        query: &str,
        filter_prereleases: bool,
    ) -> Vec<String> {
        // remove -musl feature in favour of alpine-linux OS
        let query = if Platform::current().libc() == Some("musl") && query.contains("-musl") {
            query.replace("-musl", "")
        } else {
            query.to_string()
        };
        let is_vendor_prefix = query != "latest" && query.ends_with('-');
        let query_escaped = regex::escape(&query);
        let query = match query.as_str() {
            "latest" => "[0-9].*",
            // else; use escaped query
            _ => &query_escaped,
        };
        // Same semantics as Backend::fuzzy_match_filter:
        // - "1.2" should match "1.2.3" but not "1.20"
        // - vendor prefixes like "temurin-" should match "temurin-25..."
        let query_regex = if is_vendor_prefix {
            Regex::new(&format!("^{query}.*$"))
        } else {
            Regex::new(&format!("^{query}([+\\-.].+)?$"))
        };
        // The escaped query always parses; one too long to compile names no
        // version, so it matches nothing.
        let Ok(query_regex) = query_regex else {
            return vec![];
        };

        versions
            .into_iter()
            .filter(|v| {
                if query == v {
                    return true;
                }
                if filter_prereleases && VERSION_REGEX.is_match(v) {
                    return false;
                }
                query_regex.is_match(v)
            })
            .collect()
    }
}

/// The mise version for a Java id from `.sdkmanrc`. SDKMAN writes an id as
/// `<version>[-<variant>][+<build>]-<vendor>`, such as `21.0.13.1-fx+5-librca`, and each vendor
/// pads or spells its versions differently from the mise catalog. The result names the same build:
/// the catalog version itself, or a prefix only that build continues (`temurin-21.0.13+9` for
/// `21.0.13+9.0.LTS`). Builds the catalog does not list by the version SDKMAN uses (Zulu, Liberica
/// NIK, JetBrains Runtime) get a version that [`SdkmanQuery`] resolves against the catalog. An id
/// with no exact counterpart yields a version that resolves to nothing, so installing it fails
/// instead of picking another build.
fn sdkman_java_version(id: &str) -> String {
    let Some((version, vendor)) = id.rsplit_once('-') else {
        return id.to_string();
    };
    let (version, build) = match version.split_once('+') {
        Some((version, build)) => (version, Some(build)),
        None => (version, None),
    };
    let (core, variant) = match version.split_once('-') {
        Some((core, variant)) => (core, Some(variant)),
        None => (version, None),
    };
    let vendor = match vendor {
        "amzn" => "corretto",
        "albba" => "dragonwell",
        "graal" => "oracle-graalvm",
        "graalce" => "graalvm-community",
        "jbr" => "jetbrains",
        "librca" => "liberica",
        "nik" => "liberica-nik",
        "open" => "openjdk",
        "ms" => "microsoft",
        "sapmchn" => "sapmachine",
        "sem" => "semeru-openj9",
        "tem" => "temurin",
        _ => vendor, // either same vendor name or unsupported
    };
    let prefix = match variant {
        Some("fx") => format!("{vendor}-javafx"),
        Some(variant) => format!("{vendor}-{variant}"),
        None => vendor.to_string(),
    };
    let parts = core.split('.').collect::<Vec<_>>();
    // SDKMAN pads most versions to four components; the catalog drops a `.0` fourth one.
    let unpadded = match parts[..] {
        [major, minor, patch, "0"] => format!("{major}.{minor}.{patch}"),
        _ => core.to_string(),
    };
    let with_build = |version: &str| match build {
        Some(build) => format!("{version}+{build}"),
        None => version.to_string(),
    };
    let version = match vendor {
        // `21.0.13+9` continues only into `21.0.13+9.0.LTS`, not into a hotfix such as
        // `21.0.13+101`.
        "temurin" | "liberica-nik" => with_build(&unpadded),
        // Liberica names Java 8 releases `8u514+5`.
        "liberica" => match unpadded.strip_prefix("8.0.") {
            Some(update) => with_build(&format!("8u{update}")),
            None => with_build(&unpadded),
        },
        "corretto" => corretto_version(&parts, build),
        "oracle-graalvm" | "oracle" | "openjdk" | "sapmachine" | "microsoft"
        | "graalvm-community" => unpadded,
        // SDKMAN drops the dot of the build number: `+1163125` is `-b1163.125`.
        "jetbrains" => match build {
            Some(build) => format!("{unpadded}-b{build}"),
            None => unpadded,
        },
        // Semeru releases have a fourth component the catalog keeps: `17.0.20` is `17.0.20.0`.
        "semeru-openj9" if build.is_none() && parts.len() == 3 => format!("{core}.0"),
        "kona" => match build {
            Some(build) => format!("{core}.b{build}"),
            None => core.to_string(),
        },
        "mandrel" => mandrel_version(&parts),
        "zulu" => zulu_java_version(&parts, build),
        _ => with_build(core),
    };
    format!("{prefix}-{version}")
}

/// Corretto versions append the build and revision: SDKMAN's `26.0.2.0+11.1` is `26.0.2.11.1`.
/// Java 8 releases are `8.<update>.<two-digit build>.<revision>`, which SDKMAN writes as
/// `8.514.1.1` or `8.0.512.0+8.1`.
fn corretto_version(parts: &[&str], build: Option<&str>) -> String {
    let pad = |build: &str| format!("{build:0>2}");
    match (parts, build) {
        (["8", "0", update, "0"], Some(build)) => match build.split_once('.') {
            Some((build, revision)) => format!("8.{update}.{}.{revision}", pad(build)),
            None => format!("8.{update}.{}", pad(build)),
        },
        (["8", update, build, revision], None) => format!("8.{update}.{}.{revision}", pad(build)),
        ([major, minor, patch, "0"], Some(build)) => format!("{major}.{minor}.{patch}.{build}"),
        _ => {
            let core = parts.join(".");
            match build {
                Some(build) => format!("{core}+{build}"),
                None => core,
            }
        }
    }
}

/// Mandrel releases are `<version>-Final+java<major>`. SDKMAN writes `25.0.4.1`, or
/// `25.0.2.r25` with the Java major.
fn mandrel_version(parts: &[&str]) -> String {
    match parts.split_last() {
        Some((java, version)) if java.starts_with('r') => {
            let mut version = version.to_vec();
            version.resize(4, "0");
            format!("{}-Final+java{}", version.join("."), &java[1..])
        }
        _ => format!("{}-Final", parts.join(".")),
    }
}

/// The Java version a Zulu id names, which the catalog lists as `java_version`. SDKMAN pads it
/// to four components and writes Azul's `21.0.13.0.3` builds as `21.0.13.0+3.1`; a CRaC id has
/// a fifth component for its own revision.
fn zulu_java_version(parts: &[&str], build: Option<&str>) -> String {
    match parts {
        [major, minor, patch, "0"] => match build.and_then(|b| b.split_once('.')) {
            Some((update, _)) => format!("{major}.{minor}.{patch}.0.{update}"),
            None => format!("{major}.{minor}.{patch}"),
        },
        [version @ .., _revision] if parts.len() == 5 => version.join("."),
        _ => parts.join("."),
    }
}

/// A version converted from `.sdkmanrc` that names a build by something other than its catalog
/// version, which only the catalog can turn into a version mise can install. Each of these
/// matches no catalog version as written.
struct SdkmanQuery<'a> {
    /// The vendor and features, such as `zulu-javafx`.
    prefix: &'a str,
    version: &'a str,
    kind: SdkmanQueryKind,
}

enum SdkmanQueryKind {
    /// A Zulu build by its Java version, such as `zulu-javafx-8.0.512`. Zulu's own versions
    /// never have a `0` second component (`8.98.0.111`).
    ZuluJavaVersion,
    /// A Liberica NIK build without the JDK it bundles, such as `liberica-nik-25.0.5.1+1` for
    /// `liberica-nik-openjdk25-25.0.5.1+1`, or without a build number. SDKMAN can give the
    /// bundled JDK major instead, as `.r25` in `liberica-nik-25.0.4.r25`.
    NikVersion,
    /// A JetBrains Runtime build whose build number lost its dot, such as
    /// `jetbrains-21.0.11-b1163125` for `jetbrains-21.0.11-b1163.125`.
    JetbrainsBuild,
    /// A Semeru release by its padded version, such as `semeru-openj9-22.0.1.0`, which older
    /// releases list as `22.0.1+8_openj9-0.45.0`.
    SemeruRelease,
}

impl<'a> SdkmanQuery<'a> {
    fn parse(query: &'a str) -> Option<Self> {
        let (prefix, version) = split_vendor_prefix(query)?;
        let kind = if prefix == "zulu" || prefix.starts_with("zulu-") {
            let parts = version.split('.').collect::<Vec<_>>();
            if parts.len() < 3 || parts[1] != "0" {
                return None;
            }
            SdkmanQueryKind::ZuluJavaVersion
        } else if matches!(prefix, "liberica-nik" | "liberica-nik-javafx") {
            SdkmanQueryKind::NikVersion
        } else if prefix == "jetbrains"
            && version
                .rsplit_once("-b")
                .is_some_and(|(_, build)| build.chars().all(|c| c.is_ascii_digit()))
        {
            SdkmanQueryKind::JetbrainsBuild
        } else if prefix == "semeru-openj9" && semeru_release(version).is_some() {
            SdkmanQueryKind::SemeruRelease
        } else {
            return None;
        };
        Some(Self {
            prefix,
            version,
            kind,
        })
    }
}

/// The Java version of a padded Semeru version: `22.0.1` for `22.0.1.0`.
fn semeru_release(version: &str) -> Option<&str> {
    version
        .strip_suffix(".0")
        .filter(|release| release.split('.').count() == 3)
}

impl SdkmanQuery<'_> {
    /// Whether the build `prefix-version` is the one this query names. `java_version` gives
    /// the build's Java version, which only a Zulu query reads.
    fn matches(
        &self,
        prefix: &str,
        version: &str,
        java_version: impl FnOnce() -> Option<String>,
    ) -> bool {
        match self.kind {
            SdkmanQueryKind::ZuluJavaVersion => {
                prefix == self.prefix && java_version().as_deref() == Some(self.version)
            }
            SdkmanQueryKind::NikVersion => {
                let (wanted, wanted_jdk) = nik_query_version(self.version);
                prefix.rsplit_once("-openjdk").is_some_and(|(nik, jdk)| {
                    nik == self.prefix
                        && jdk.chars().all(|c| c.is_ascii_digit())
                        && wanted_jdk.is_none_or(|wanted_jdk| jdk == wanted_jdk)
                }) && (version == wanted
                    || (!wanted.contains('+') && version.starts_with(&format!("{wanted}+"))))
            }
            SdkmanQueryKind::JetbrainsBuild => {
                prefix == self.prefix
                    && version.rsplit_once("-b").is_some_and(|(v, b)| {
                        format!("{v}-b{}", b.replace('.', "")) == self.version
                    })
            }
            SdkmanQueryKind::SemeruRelease => {
                prefix == self.prefix
                    && (version == self.version
                        || semeru_release(self.version)
                            .is_some_and(|release| version.starts_with(&format!("{release}+"))))
            }
        }
    }
}

/// The Liberica NIK version a query names, and the major of the JDK it bundles when SDKMAN
/// gave one: `25.0.4.r25` is `("25.0.4", Some("25"))`.
fn nik_query_version(version: &str) -> (String, Option<&str>) {
    let (core, build) = match version.split_once('+') {
        Some((core, build)) => (core, Some(build)),
        None => (version, None),
    };
    let Some((nik, jdk)) = core
        .rsplit_once(".r")
        .filter(|(_, jdk)| !jdk.is_empty() && jdk.chars().all(|c| c.is_ascii_digit()))
    else {
        return (version.to_string(), None);
    };
    let nik = match nik.split('.').collect::<Vec<_>>()[..] {
        [major, minor, patch, "0"] => format!("{major}.{minor}.{patch}"),
        _ => nik.to_string(),
    };
    let nik = match build {
        Some(build) => format!("{nik}+{build}"),
        None => nik,
    };
    (nik, Some(jdk))
}

/// The catalog versions `query` names on `platform`.
fn sdkman_catalog_match(
    metadata: &HashMap<String, JavaMetadata>,
    query: &SdkmanQuery,
    platform: &Platform,
) -> Vec<String> {
    metadata
        .values()
        .filter(|m| {
            let key = m.to_version_string(platform);
            key.strip_suffix(&format!("-{}", m.version))
                .is_some_and(|prefix| {
                    query.matches(prefix, &m.version, || Some(m.java_version.clone()))
                })
        })
        .map(|m| m.to_version_string(platform))
        .collect()
}

/// The installed versions `query` names, so a build installed from `.sdkmanrc` is found
/// again without the catalog. A Zulu build's Java version comes from its `release` file.
fn sdkman_installed_match(
    installed: Vec<String>,
    query: &SdkmanQuery,
    install_path: impl Fn(&str) -> PathBuf,
) -> Vec<String> {
    installed
        .into_iter()
        .filter(|installed| {
            let Some((prefix, version)) = split_vendor_prefix(installed) else {
                return false;
            };
            query.matches(prefix, version, || {
                release_java_version(&install_path(installed))
            })
        })
        .collect()
}

/// `zulu-javafx-8.98.0.111` as `("zulu-javafx", "8.98.0.111")`: the vendor and features end
/// at the first `-` followed by a digit.
fn split_vendor_prefix(version: &str) -> Option<(&str, &str)> {
    let at = version
        .match_indices('-')
        .find(|(i, _)| version[i + 1..].starts_with(|c: char| c.is_ascii_digit()))?
        .0;
    Some((&version[..at], &version[at + 1..]))
}

/// The `JAVA_VERSION` of the JDK in `install_path`, as the catalog writes it: `8.0.514` for
/// Java 8's `1.8.0_514`.
fn release_java_version(install_path: &Path) -> Option<String> {
    let release = fs::read_to_string(install_path.join("release")).ok()?;
    let version = release
        .lines()
        .find_map(|line| line.trim().strip_prefix("JAVA_VERSION="))?
        .trim_matches('"');
    Some(match version.strip_prefix("1.") {
        Some(legacy) => legacy.replacen('_', ".", 1),
        None => version.to_string(),
    })
}

/// Every metadata entry under its full version string, plus a shorthand entry like
/// "17.0.2" for each release from `shorthand_vendor`, which `java_metadata_key` turns
/// back into "<vendor>-17.0.2" on install. Entries are visited in key order so the
/// shorthand points at the canonical "<vendor>-<version>" entry when one exists: digits
/// sort before the letters of a feature or image type such as "-jre-".
fn with_shorthand_versions<'a>(
    metadata: &'a HashMap<String, JavaMetadata>,
    shorthand_vendor: &str,
) -> Vec<(&'a str, &'a JavaMetadata)> {
    let mut shorthand: BTreeMap<&str, &JavaMetadata> = BTreeMap::new();
    for (_, m) in metadata
        .iter()
        .filter(|(_, m)| m.vendor == shorthand_vendor)
        .sorted_by_key(|(k, _)| *k)
    {
        shorthand.entry(m.version.as_str()).or_insert(m);
    }
    metadata
        .iter()
        .map(|(k, m)| (k.as_str(), m))
        .chain(shorthand)
        .collect()
}

/// The metadata key for `version`: a vendor-prefixed version as is, and a shorthand
/// version such as "21.0.4+7" with the `shorthand_vendor` prefix.
///
/// Shorthand versions meant OpenJDK builds before the default vendor became Temurin.
/// A shorthand version that `shorthand_vendor` does not publish, such as "21.0.2"
/// (Temurin calls it "21.0.2+13"), maps to the OpenJDK build only when `is_openjdk_build`
/// confirms that this version already is that build: installed from it, or locked to
/// its URL. A version being resolved fresh, such as `java@12.0.2`, which Temurin does not
/// publish, never falls back, so installing agrees with `ls-remote`.
fn java_metadata_key(
    metadata: &HashMap<String, JavaMetadata>,
    version: &str,
    shorthand_vendor: &str,
    is_openjdk_build: impl FnOnce(&JavaMetadata) -> bool,
) -> String {
    if !regex!(r"^\d").is_match(version) {
        return version.to_string();
    }
    let key = format!("{shorthand_vendor}-{version}");
    if metadata.contains_key(&key) {
        return key;
    }
    let openjdk_key = format!("openjdk-{version}");
    match metadata.get(&openjdk_key) {
        Some(m) if is_openjdk_build(m) => openjdk_key,
        _ => key,
    }
}

/// Whether `install_path` holds a jdk.java.net OpenJDK build, going by the
/// `IMPLEMENTOR` line of the JDK's `release` file.
fn is_openjdk_install(install_path: &Path) -> bool {
    fs::read_to_string(install_path.join("release")).is_ok_and(|release| {
        release
            .lines()
            .any(|line| line.trim() == r#"IMPLEMENTOR="Oracle Corporation""#)
    })
}

fn find_java_metadata<'a>(
    metadata: &'a HashMap<String, JavaMetadata>,
    version: &str,
    requested_version: &str,
    platform: &Platform,
) -> Result<&'a JavaMetadata> {
    // A version with no build for this platform is an unsupported target, so `mise lock`
    // skips the platform instead of failing the whole run.
    metadata.get(version).ok_or_else(|| {
        crate::errors::Error::UnsupportedTarget(format!(
            "no metadata found for version {} on {}",
            requested_version,
            platform.to_key()
        ))
        .into()
    })
}

fn java_os(platform: &Platform) -> &str {
    if platform.is_macos() {
        "macosx"
    } else if platform.os == "freebsd" {
        "linux"
    } else if platform.is_linux() && platform.libc() == Some("musl") {
        "alpine-linux"
    } else {
        &platform.os
    }
}

fn java_arch(platform: &Platform) -> &str {
    match platform.arch.as_str() {
        "x64" => "x86_64",
        "arm64" => "aarch64",
        "arm" => "arm32-vfp-hflt",
        other => other,
    }
}

fn java_file_type_supported(platform: &Platform, file_type: &str) -> bool {
    if platform.is_windows() {
        file_type == "zip"
    } else {
        matches!(file_type, "tar.gz" | "tar.xz")
    }
}

fn current_java_platform() -> Platform {
    let settings = Settings::get();
    // Preserve Java's existing host behavior: downloads are selected from the
    // actual runtime OS, while settings.arch can override architecture. Explicit
    // cross-platform lock generation uses PlatformTarget instead.
    let qualifier = if OS.as_str() == "linux" && Platform::current().libc() == Some("musl") {
        Some("musl".to_string())
    } else {
        None
    };
    Platform {
        os: OS.to_string(),
        arch: settings.arch().to_string(),
        qualifier,
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct JavaMetadata {
    // architecture: String,
    checksum: Option<String>,
    // checksum_url: Option<String>,
    created_at: Option<String>,
    features: Option<Vec<String>>,
    file_type: Option<String>,
    // filename: String,
    image_type: Option<String>,
    java_version: String,
    jvm_impl: String,
    // os: String,
    // release_type: String,
    // size: Option<i32>,
    url: String,
    vendor: String,
    version: String,
}

impl Display for JavaMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_version_string(&current_java_platform()))
    }
}

impl JavaMetadata {
    /// The parts of a catalog entry that installing needs, rebuilt from the lockfile so
    /// an install of a locked build does not fetch the catalog. `version` is the locked
    /// version such as "zulu-21.0.1", `url` the locked download URL, and `shorthand_vendor`
    /// the vendor a numeric version like "21.0.1" stands for.
    fn from_locked(version: &str, url: &str, shorthand_vendor: &str) -> Self {
        let mut parts = version.split('-');
        let vendor = match parts.next() {
            Some(v) if !v.starts_with(|c: char| c.is_ascii_digit()) => v.to_string(),
            _ => shorthand_vendor.to_string(),
        };
        Self {
            url: url.to_string(),
            vendor,
            version: version
                .split('-')
                .find(|p| p.starts_with(|c: char| c.is_ascii_digit()))
                .unwrap_or_default()
                .to_string(),
            ..Default::default()
        }
    }

    fn to_version_string(&self, platform: &Platform) -> String {
        let mut v = vec![self.vendor.clone()];
        if self
            .image_type
            .as_ref()
            .is_some_and(|image_type| image_type == "jre")
        {
            v.push(self.image_type.clone().unwrap());
        } else if self.image_type.is_none() {
            v.push("unknown".to_string());
        }
        if let Some(features) = &self.features {
            for f in features {
                if platform.libc() == Some("musl") && f == "musl" {
                    continue;
                }
                if JAVA_FEATURES.contains(f) {
                    v.push(f.clone());
                }
            }
        }
        if self.jvm_impl == "openj9" {
            v.push(self.jvm_impl.clone());
        }
        if self.vendor == "liberica-nik" {
            let major = self
                .java_version
                .split('.')
                .next()
                .unwrap_or(&self.java_version);
            v.push(format!("openjdk{}", major));
        }
        v.push(self.version.clone());
        v.join("-")
    }
}

// only care about these features
static JAVA_FEATURES: Lazy<HashSet<String>> = Lazy::new(|| {
    HashSet::from(
        [
            "crac",
            "innovation",
            "javafx",
            "jcef",
            "leyden",
            "lite",
            "musl",
        ]
        .map(|s| s.to_string()),
    )
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_locked_metadata_keeps_vendor_and_version() {
        let url = "https://cdn.azul.com/zulu/bin/zulu21.30.15-ca-jdk21.0.1-macosx_aarch64.tar.gz";
        let m = JavaMetadata::from_locked("zulu-21.0.1", url, "temurin");
        assert_eq!((m.vendor.as_str(), m.version.as_str()), ("zulu", "21.0.1"));
        assert_eq!(m.url, url);
        let m = JavaMetadata::from_locked("21.0.1", url, "zulu");
        assert_eq!((m.vendor.as_str(), m.version.as_str()), ("zulu", "21.0.1"));
    }

    #[test]
    fn java_missing_metadata_includes_version_and_platform() {
        let metadata = HashMap::new();
        for key in [
            "windows-arm64",
            "windows-x64",
            "macos-arm64",
            "linux-x64-musl",
        ] {
            let platform = Platform::parse(key).unwrap();
            let error = find_java_metadata(&metadata, "zulu-8", "zulu-8", &platform).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("no metadata found for version zulu-8 on {key}")
            );
        }
    }

    #[test]
    fn java_missing_metadata_is_an_unsupported_target() {
        let platform = Platform::parse("linux-arm64-musl").unwrap();
        let error = find_java_metadata(
            &HashMap::new(),
            "oracle-graalvm-25",
            "oracle-graalvm-25",
            &platform,
        )
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<crate::errors::Error>(),
            Some(crate::errors::Error::UnsupportedTarget(_))
        ));
    }

    #[test]
    fn java_missing_metadata_preserves_requested_version() {
        let platform = Platform::parse("windows-arm64").unwrap();
        let error = find_java_metadata(&HashMap::new(), "openjdk-8", "8", &platform).unwrap_err();
        assert_eq!(
            error.to_string(),
            "no metadata found for version 8 on windows-arm64"
        );
    }

    #[test]
    fn java_metadata_lookup_preserves_matching_entry() {
        let platform = Platform::parse("windows-x64").unwrap();
        let metadata = HashMap::from([("openjdk-8".to_string(), JavaMetadata::default())]);
        let found = find_java_metadata(&metadata, "openjdk-8", "8", &platform).unwrap();
        assert!(std::ptr::eq(found, &metadata["openjdk-8"]));
    }

    #[test]
    fn java_metadata_key_falls_back_only_for_existing_openjdk_builds() {
        let metadata = HashMap::from([
            ("openjdk-12.0.2".to_string(), JavaMetadata::default()),
            ("openjdk-21.0.2".to_string(), JavaMetadata::default()),
            ("temurin-21.0.2+13".to_string(), JavaMetadata::default()),
            ("openjdk-25".to_string(), JavaMetadata::default()),
            ("temurin-25".to_string(), JavaMetadata::default()),
        ]);
        let key =
            |version, is_openjdk| java_metadata_key(&metadata, version, "temurin", |_| is_openjdk);
        // an OpenJDK shorthand version installed or locked before the default became Temurin
        assert_eq!(key("21.0.2", true), "openjdk-21.0.2");
        // a fresh request for a version Temurin does not publish finds nothing
        assert_eq!(key("21.0.2", false), "temurin-21.0.2");
        assert_eq!(key("12.0.2", false), "temurin-12.0.2");
        assert_eq!(key("21.0.2+13", true), "temurin-21.0.2+13");
        // the shorthand vendor wins when both vendors publish the version
        assert_eq!(key("25", true), "temurin-25");
        assert_eq!(key("21.0.3", true), "temurin-21.0.3");
        assert_eq!(key("zulu-21.0.2", true), "zulu-21.0.2");
        assert_eq!(
            java_metadata_key(&metadata, "21.0.2", "openjdk", |_| false),
            "openjdk-21.0.2"
        );
    }

    #[test]
    fn openjdk_install_is_read_from_the_release_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_openjdk_install(dir.path()));
        fs::write(
            dir.path().join("release"),
            "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"21.0.2\"\n",
        )
        .unwrap();
        assert!(!is_openjdk_install(dir.path()));
        fs::write(
            dir.path().join("release"),
            "IMPLEMENTOR=\"Oracle Corporation\"\nJAVA_VERSION=\"21.0.2\"\n",
        )
        .unwrap();
        assert!(is_openjdk_install(dir.path()));
    }

    fn opts_with_release_type(release_type: &str) -> ToolVersionOptions {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "release_type".to_string(),
            toml::Value::String(release_type.to_string()),
        );
        opts
    }

    #[test]
    fn java_options_reads_release_type() {
        let default_vendor = Settings::get().java.shorthand_vendor.clone();
        let default_opts = ToolVersionOptions::default();
        assert_eq!(JavaOptions::new(&default_opts).release_type(), "ga");
        assert_eq!(
            JavaOptions::new(&default_opts).lockfile_options("17", &default_vendor),
            BTreeMap::from([("shorthand_vendor".to_string(), default_vendor.clone())])
        );

        let opts = opts_with_release_type("ea");
        assert_eq!(JavaOptions::new(&opts).release_type(), "ea");
        assert_eq!(
            JavaOptions::new(&opts).lockfile_options("17", &default_vendor),
            BTreeMap::from([
                ("release_type".to_string(), "ea".to_string()),
                ("shorthand_vendor".to_string(), default_vendor.clone())
            ])
        );
    }

    #[test]
    fn java_lockfile_options_include_shorthand_vendor() {
        let opts = ToolVersionOptions::default();

        assert_eq!(
            JavaOptions::new(&opts).lockfile_options("17", "temurin"),
            BTreeMap::from([("shorthand_vendor".to_string(), "temurin".to_string())])
        );
        assert_eq!(
            JavaOptions::new(&opts).lockfile_options("lts", "temurin"),
            BTreeMap::from([("shorthand_vendor".to_string(), "temurin".to_string())])
        );
        assert_eq!(
            JavaOptions::new(&opts).lockfile_options("17", "openjdk"),
            BTreeMap::from([("shorthand_vendor".to_string(), "openjdk".to_string())])
        );
        assert!(
            JavaOptions::new(&opts)
                .lockfile_options("temurin-17", "temurin")
                .is_empty()
        );
    }

    fn java_metadata(vendor: &str, image_type: &str, version: &str) -> JavaMetadata {
        JavaMetadata {
            vendor: vendor.to_string(),
            image_type: Some(image_type.to_string()),
            version: version.to_string(),
            ..Default::default()
        }
    }

    /// The cached metadata must not depend on `java.shorthand_vendor`, or changing the
    /// vendor keeps listing the previous vendor's shorthand versions until the cache expires.
    #[test]
    fn shorthand_versions_follow_the_current_vendor() {
        let platform = Platform::parse("linux-x64").unwrap();
        let mut metadata = HashMap::new();
        for m in [
            java_metadata("openjdk", "jdk", "21.0.2"),
            java_metadata("openjdk", "jre", "21.0.2"),
            java_metadata("temurin", "jdk", "21.0.5+11"),
        ] {
            JavaPlugin::insert_java_metadata(&mut metadata, m, &platform);
        }
        assert_eq!(
            metadata.keys().sorted().collect_vec(),
            ["openjdk-21.0.2", "openjdk-jre-21.0.2", "temurin-21.0.5+11"]
        );

        let shorthand = |vendor| {
            with_shorthand_versions(&metadata, vendor)
                .into_iter()
                .filter(|(v, _)| v.starts_with(|c: char| c.is_ascii_digit()))
                .map(|(v, m)| (v, m.vendor.as_str(), m.image_type.as_deref()))
                .collect_vec()
        };
        assert_eq!(shorthand("openjdk"), [("21.0.2", "openjdk", Some("jdk"))]);
        assert_eq!(
            shorthand("temurin"),
            [("21.0.5+11", "temurin", Some("jdk"))]
        );
    }

    /// Core plugins are shared instances without options, so `java[release_type=ea]`
    /// needs its own instance for version listing to select early-access builds.
    #[test]
    fn inline_release_type_reaches_the_java_backend() {
        let backend =
            crate::backend::arg_to_backend(BackendArg::from("java[release_type=ea]")).unwrap();
        let opts = backend.ba().resolve_opts_with_layers(None, None, None);
        assert_eq!(JavaOptions::new(opts.effective()).release_type(), "ea");

        let shared = crate::backend::arg_to_backend(BackendArg::from("java")).unwrap();
        assert!(shared.ba().explicit_opts().is_none());
        assert_eq!(backend.id(), shared.id());
        assert_eq!(backend.ba().installs_path(), shared.ba().installs_path());
        assert_eq!(backend.ba().cache_path(), shared.ba().cache_path());

        // An alias such as `[tool_alias] myjava = "core:java"` keeps its options too,
        // and still uses the shared instance's paths.
        let aliased = crate::backend::arg_to_backend(BackendArg::new_raw(
            "myjava".to_string(),
            Some("core:java".to_string()),
            "myjava".to_string(),
            Some(crate::toolset::parse_tool_options("release_type=ea")),
            crate::args::BackendResolution::new(true),
        ))
        .unwrap();
        let opts = aliased.ba().resolve_opts_with_layers(None, None, None);
        assert_eq!(JavaOptions::new(opts.effective()).release_type(), "ea");
        assert_eq!(aliased.id(), shared.id());
        assert_eq!(aliased.ba().installs_path(), shared.ba().installs_path());
        assert_eq!(aliased.ba().cache_path(), shared.ba().cache_path());
    }

    fn match_versions(versions: &[&str], query: &str) -> Vec<String> {
        JavaPlugin::new().fuzzy_match_filter(
            versions.iter().map(|v| v.to_string()).collect(),
            query,
            true,
        )
    }

    /// A version too long to compile into a pattern matches nothing instead of
    /// stopping the resolution of every tool.
    #[test]
    fn an_overlong_query_matches_nothing() {
        let long = "1".repeat(400_000);
        assert!(match_versions(&["21.0.1"], &long).is_empty());
        assert!(match_versions(&["temurin-21.0.1"], &format!("temurin-{long}-")).is_empty());
    }

    #[tokio::test]
    async fn sdkmanrc_ids_name_the_same_build() {
        let dir = tempfile::tempdir().unwrap();
        let parse = async |contents: &str| {
            let path = dir.path().join(".sdkmanrc");
            std::fs::write(&path, contents).unwrap();
            JavaPlugin::new()
                ._parse_idiomatic_file(&path)
                .await
                .unwrap()
        };
        for (id, version) in [
            // Temurin: a `.0` fourth component is padding; the build stays
            ("26.0.2.1+1-tem", "temurin-26.0.2.1+1"),
            ("21.0.2+13-tem", "temurin-21.0.2+13"),
            ("21.0.13.0+9-tem", "temurin-21.0.13+9"),
            ("8.0.512.0+9-tem", "temurin-8.0.512+9"),
            // Liberica keeps the build and the JavaFX or CRaC variant
            ("21.0.13.1+5-librca", "liberica-21.0.13.1+5"),
            ("21.0.13.1-fx+5-librca", "liberica-javafx-21.0.13.1+5"),
            ("21.0.13.1-crac+6-librca", "liberica-crac-21.0.13.1+6"),
            ("8.0.514.0-fx+5-librca", "liberica-javafx-8u514+5"),
            ("24.0.1.0-fx+1-nik", "liberica-nik-javafx-24.0.1+1"),
            ("25.0.4.r25-nik", "liberica-nik-25.0.4.r25"),
            ("22.3.5.r17-nik", "liberica-nik-22.3.5.r17"),
            ("26.0.2.0+11.1-amzn", "corretto-26.0.2.11.1"),
            ("8.514.1.1-amzn", "corretto-8.514.01.1"),
            ("8.0.512.0+8.1-amzn", "corretto-8.512.08.1"),
            ("25.0.5.1-graal", "oracle-graalvm-25.0.5.1"),
            ("24.0.0.0-graal", "oracle-graalvm-24.0.0"),
            ("26.0.0.0+35-open", "openjdk-26.0.0"),
            ("26.0.2+1-sapmchn", "sapmachine-26.0.2"),
            ("17.0.20-sem", "semeru-openj9-17.0.20.0"),
            ("25.0.5.1+635102-jbr", "jetbrains-25.0.5.1-b635102"),
            ("21.0.11.0+1163125-jbr", "jetbrains-21.0.11-b1163125"),
            ("25.0.5.1+1-kona", "kona-25.0.5.1.b1"),
            ("25.0.4.1-mandrel", "mandrel-25.0.4.1-Final"),
            ("25.0.2.r25-mandrel", "mandrel-25.0.2.0-Final+java25"),
            // Zulu ids carry the Java version, which the catalog resolves
            ("8.0.512.0-fx+9-zulu", "zulu-javafx-8.0.512"),
            ("21.0.13.1+2-zulu", "zulu-21.0.13.1"),
            ("21.0.13.0-fx+3.1-zulu", "zulu-javafx-21.0.13.0.3"),
            ("27.0.1.1.1-crac+1-zulu", "zulu-crac-27.0.1.1"),
            ("21.0.2-zulu", "zulu-21.0.2"),
            ("21.0.2", "21.0.2"),
        ] {
            assert_eq!(parse(&format!("java={id}\n")).await, [version], "{id}");
        }
    }

    #[test]
    fn sdkman_queries_match_installed_builds() {
        let dir = tempfile::tempdir().unwrap();
        let installed = [
            ("zulu-javafx-8.98.0.111", Some("1.8.0_512")),
            ("zulu-21.54.109.0", Some("21.0.13.0.3")),
            ("liberica-nik-openjdk25-25.0.5.1+1", None),
            ("jetbrains-21.0.11-b1163.125", None),
            ("semeru-openj9-22.0.1+8_openj9-0.45.0", None),
        ];
        for (version, java_version) in installed {
            let path = dir.path().join(version);
            fs::create_dir_all(&path).unwrap();
            if let Some(java_version) = java_version {
                fs::write(
                    path.join("release"),
                    format!(
                        "IMPLEMENTOR=\"Azul Systems, Inc.\"\nJAVA_VERSION=\"{java_version}\"\n"
                    ),
                )
                .unwrap();
            }
        }
        let find = |query: &str| {
            let query = SdkmanQuery::parse(query).unwrap();
            let versions = installed.iter().map(|(v, _)| v.to_string()).collect();
            sdkman_installed_match(versions, &query, |v| dir.path().join(v))
        };
        assert_eq!(find("zulu-javafx-8.0.512"), ["zulu-javafx-8.98.0.111"]);
        assert!(find("zulu-8.0.512").is_empty());
        assert_eq!(find("zulu-21.0.13.0.3"), ["zulu-21.54.109.0"]);
        assert_eq!(
            find("liberica-nik-25.0.5.1+1"),
            ["liberica-nik-openjdk25-25.0.5.1+1"]
        );
        assert_eq!(
            find("jetbrains-21.0.11-b1163125"),
            ["jetbrains-21.0.11-b1163.125"]
        );
        assert_eq!(
            find("semeru-openj9-22.0.1.0"),
            ["semeru-openj9-22.0.1+8_openj9-0.45.0"]
        );
    }

    fn catalog_entry(
        vendor: &str,
        version: &str,
        java_version: &str,
        features: &[&str],
    ) -> JavaMetadata {
        JavaMetadata {
            vendor: vendor.to_string(),
            image_type: Some("jdk".to_string()),
            version: version.to_string(),
            java_version: java_version.to_string(),
            features: Some(features.iter().map(|f| f.to_string()).collect()),
            jvm_impl: "hotspot".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn sdkman_queries_match_the_catalog_build() {
        let platform = Platform::parse("linux-x64").unwrap();
        let mut metadata = HashMap::new();
        for m in [
            catalog_entry("zulu", "8.98.0.111", "8.0.512", &[]),
            catalog_entry("zulu", "8.98.0.111", "8.0.512", &["javafx"]),
            catalog_entry("zulu", "8.98.0.201", "8.0.514", &["javafx"]),
            catalog_entry("liberica-nik", "25.0.5.1+1", "25.0.5.1+5", &["javafx"]),
            catalog_entry("liberica-nik", "25.0.4+1", "25.0.4+10", &[]),
            catalog_entry("liberica-nik", "25.0.4.1+1", "25.0.4+10", &[]),
            catalog_entry("liberica-nik", "22.3.5+1", "17.0.13+12", &[]),
            catalog_entry("liberica-nik", "22.3.5+2", "21.0.5+11", &[]),
            catalog_entry("jetbrains", "21.0.11-b1163.125", "21.0.11-b1163.125", &[]),
            catalog_entry("jetbrains", "21.0.11-b1163.12", "21.0.11-b1163.12", &[]),
            JavaMetadata {
                jvm_impl: "openj9".to_string(),
                ..catalog_entry("semeru", "22.0.1+8_openj9-0.45.0", "22.0.1+8", &[])
            },
            JavaMetadata {
                jvm_impl: "openj9".to_string(),
                ..catalog_entry("semeru", "21.0.13.0", "21.0.13.0", &[])
            },
            JavaMetadata {
                jvm_impl: "openj9".to_string(),
                ..catalog_entry("semeru", "21.0.13.10", "21.0.13.10", &[])
            },
        ] {
            JavaPlugin::insert_java_metadata(&mut metadata, m, &platform);
        }
        let find = |query: &str| {
            let query = SdkmanQuery::parse(query).unwrap();
            sdkman_catalog_match(&metadata, &query, &platform)
        };
        assert_eq!(find("zulu-javafx-8.0.512"), ["zulu-javafx-8.98.0.111"]);
        assert_eq!(find("zulu-8.0.512"), ["zulu-8.98.0.111"]);
        assert!(find("zulu-8.0.513").is_empty());
        assert_eq!(
            find("liberica-nik-javafx-25.0.5.1+1"),
            ["liberica-nik-javafx-openjdk25-25.0.5.1+1"]
        );
        assert_eq!(
            find("liberica-nik-25.0.4"),
            ["liberica-nik-openjdk25-25.0.4+1"]
        );
        // `.r17` picks the release that bundles JDK 17, never a longer NIK version
        assert_eq!(
            find("liberica-nik-25.0.4.r25"),
            ["liberica-nik-openjdk25-25.0.4+1"]
        );
        assert_eq!(
            find("liberica-nik-22.3.5.r17"),
            ["liberica-nik-openjdk17-22.3.5+1"]
        );
        assert!(find("liberica-nik-22.3.5.r11").is_empty());
        assert_eq!(
            find("jetbrains-21.0.11-b1163125"),
            ["jetbrains-21.0.11-b1163.125"]
        );
        assert_eq!(
            find("semeru-openj9-22.0.1.0"),
            ["semeru-openj9-22.0.1+8_openj9-0.45.0"]
        );
        assert_eq!(find("semeru-openj9-21.0.13.0"), ["semeru-openj9-21.0.13.0"]);
        assert!(SdkmanQuery::parse("semeru-openj9-21.0.13.10").is_none());
        // versions that name a catalog build as written are not sdkman queries
        assert!(SdkmanQuery::parse("zulu-8.98.0.111").is_none());
        assert!(SdkmanQuery::parse("zulu-8").is_none());
        assert!(SdkmanQuery::parse("liberica-nik-openjdk25-25.0.4+1").is_none());
        assert!(SdkmanQuery::parse("jetbrains-21.0.11-b1163.125").is_none());
        assert!(SdkmanQuery::parse("temurin-21.0.13").is_none());
    }

    #[test]
    fn regular_temurin_build_query_skips_hotfix_builds() {
        let versions = [
            "temurin-21.0.13+9.0.LTS",
            "temurin-21.0.13+101.0.LTS",
            "temurin-21.0.13+91",
        ];
        assert_eq!(
            match_versions(&versions, "temurin-21.0.13+9"),
            ["temurin-21.0.13+9.0.LTS".to_string()]
        );
    }

    /// A numeric query must not bleed into a longer version: "11.0.1" selects
    /// "11.0.1" and its build numbers, never "11.0.10".
    #[test]
    fn numeric_query_does_not_match_a_longer_version() {
        let versions = ["11.0.1", "11.0.1+13", "11.0.10", "11.0.10+9"];
        assert_eq!(
            match_versions(&versions, "11.0.1"),
            ["11.0.1".to_string(), "11.0.1+13".to_string()]
        );
    }

    /// `mise upgrade --bump` queries with just the vendor prefix, which has to
    /// match versions whose next character is a digit. See discussion #7328.
    #[test]
    fn vendor_prefix_query_matches_versions_starting_with_a_digit() {
        let versions = ["temurin-25.0.1", "corretto-25.0.1.9.1", "zulu-25.0.1"];
        assert_eq!(
            match_versions(&versions, "temurin-"),
            ["temurin-25.0.1".to_string()]
        );
        assert_eq!(
            match_versions(&versions, "corretto-"),
            ["corretto-25.0.1.9.1".to_string()]
        );
    }

    /// A vendor-qualified major version stays within that major version.
    #[test]
    fn vendor_query_with_major_version_keeps_the_major_version() {
        let versions = ["temurin-21.0.9+11", "temurin-21.0.10+7", "temurin-25.0.1+9"];
        assert_eq!(
            match_versions(&versions, "temurin-21"),
            [
                "temurin-21.0.9+11".to_string(),
                "temurin-21.0.10+7".to_string()
            ]
        );
    }
}
