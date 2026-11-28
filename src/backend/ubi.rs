use crate::args::BackendArg;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::runtime_path_for_install_path;
use crate::backend::static_helpers::{
    ensure_safe_relative_bin_dir, ensure_safe_relative_bin_path, try_with_v_prefix,
};
use crate::config::{Config, Settings};
use crate::env::{
    GITHUB_TOKEN, GITLAB_TOKEN, MISE_GITHUB_ENTERPRISE_TOKEN, MISE_GITLAB_ENTERPRISE_TOKEN,
};
use crate::install_context::InstallContext;
use crate::plugins::VERSION_REGEX;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{backend::Backend, toolset::ToolVersionOptions};
use crate::{file, github, gitlab, hash};
use async_trait::async_trait;
use eyre::{Result, bail};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::{fmt::Debug, sync::LazyLock};
use ubi::{ForgeType, UbiBuilder};
use walkdir::WalkDir;
use xx::regex;

#[derive(Debug)]
pub(crate) struct UbiBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct UbiOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> UbiOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn provider(&self) -> eyre::Result<ForgeType> {
        match self.values.str("provider") {
            Some(forge) => Ok(ForgeType::from_str(forge)?),
            None => Ok(ForgeType::default()),
        }
    }

    fn api_url_override(&self) -> Option<&'a str> {
        self.values.str("api_url")
    }

    fn api_url(&self, forge: &ForgeType) -> eyre::Result<String> {
        match self.api_url_override() {
            Some(api_url) => Ok(api_url.strip_suffix('/').unwrap_or(api_url).to_string()),
            None => match forge {
                ForgeType::GitHub => Ok(github::API_URL.to_string()),
                ForgeType::GitLab => Ok(gitlab::API_URL.to_string()),
                _ => bail!("Unsupported forge type {:?}", forge),
            },
        }
    }

    fn tag_regex(&self) -> Option<&'a str> {
        self.values.str("tag_regex")
    }

    fn bin_path(&self) -> Option<String> {
        self.values.platform_string("bin_path")
    }

    fn extract_all(&self) -> bool {
        self.values.bool("extract_all")
    }

    fn exe(&self) -> Option<&'a str> {
        self.values.str("exe")
    }

    fn rename_exe(&self) -> Option<&'a str> {
        self.values.str("rename_exe")
    }

    /// `exe` and `rename_exe` are joined onto the install directory, and ubi
    /// writes the executable to the result.
    fn ensure_safe_exe_paths(&self) -> Result<()> {
        if let Some(exe) = self.exe() {
            ensure_safe_relative_bin_path("exe", exe)?;
        }
        if let Some(rename_exe) = self.rename_exe() {
            ensure_safe_relative_bin_path("rename_exe", rename_exe)?;
        }
        Ok(())
    }

    fn matching(&self) -> Option<&'a str> {
        self.values.str("matching")
    }

    fn matching_regex(&self) -> Option<&'a str> {
        self.values.str("matching_regex")
    }

    fn lockfile_options(&self) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        for key in ["exe", "matching", "matching_regex", "provider"] {
            if let Some(value) = self.values.str(key) {
                result.insert(key.to_string(), value.to_string());
            }
        }
        result
    }
}

/// The versions a ubi fuzzy `query` selects: the query, optionally followed by
/// `-` or `.` and more.
pub(crate) fn ubi_fuzzy_match(
    versions: Vec<String>,
    query: &str,
    filter_prereleases: bool,
) -> Vec<String> {
    let escaped_query = regex::escape(query);
    let query = if query == "latest" {
        "\\D*[0-9].*"
    } else {
        &escaped_query
    };
    // The escaped query always parses; one too long to compile names no
    // version, so it matches nothing.
    let Ok(query_regex) = Regex::new(&format!("^{query}([-.].+)?$")) else {
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

#[async_trait]
impl Backend for UbiBackend {
    fn option_spec(&self) -> Option<crate::backend::OptionSpec> {
        Some(crate::backend::OptionSpec::with_platforms(
            &["tool_ubi_options"],
            &["tool_ubi_platforms"],
        ))
    }

    fn get_type(&self) -> BackendType {
        BackendType::Ubi
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &["provider", "api_url", "tag_regex"]
    }

    /// `tag_regex` only filters which tags show up in the version list; the installed
    /// version is already a concrete tag. `provider` and `api_url` are not listed: they
    /// are read again at install time and name the forge and server the release comes from.
    fn identity_ignored_options(&self) -> &'static [&'static str] {
        &["tag_regex"]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        deprecated_at!(
            "2026.4.0",
            "2027.1.0",
            "ubi",
            "The ubi backend is deprecated. Use the github backend instead (e.g., github:owner/repo)."
        );
        if name_is_url(&self.tool_name()) {
            Ok(vec![VersionInfo {
                version: "latest".to_string(),
                ..Default::default()
            }])
        } else {
            let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
            let opts = UbiOptions::new(&raw_opts);
            let forge = opts.provider()?;
            let api_url = opts.api_url(&forge)?;
            let tag_regex = opts.tag_regex();

            let tag_regex_cell = OnceLock::new();

            // Build release URL base based on forge type and api_url. A login in
            // `api_url` authenticates the API requests and is not part of the
            // web URL, which is printed as `release_url` and stored in the
            // version cache.
            let web_api_url = mise_util::redactions::strip_credentials_in_value(&api_url);
            let release_url_base = match forge {
                ForgeType::GitHub => {
                    if web_api_url == github::API_URL {
                        format!("https://github.com/{}", self.tool_name())
                    } else {
                        // Enterprise GitHub - derive web URL from API URL
                        let web_url = web_api_url.replace("/api/v3", "").replace("api.", "");
                        format!("{}/{}", web_url, self.tool_name())
                    }
                }
                ForgeType::GitLab => {
                    if web_api_url == gitlab::API_URL {
                        format!("https://gitlab.com/{}", self.tool_name())
                    } else {
                        // Enterprise GitLab - derive web URL from API URL
                        let web_url = web_api_url.replace("/api/v4", "");
                        format!("{}/{}", web_url, self.tool_name())
                    }
                }
                _ => bail!("Unsupported forge type {:?}", forge),
            };

            // Helper to check if tag matches tag_regex (if provided)
            let matches_tag_regex = |tag: &str| -> bool {
                if let Some(re_str) = tag_regex {
                    let re = tag_regex_cell.get_or_init(|| Regex::new(re_str).unwrap());
                    re.is_match(tag)
                } else {
                    true
                }
            };

            // Helper to strip 'v' prefix from version
            let strip_v_prefix = |tag: &str| -> String {
                if regex!(r"^v[0-9]").is_match(tag) {
                    tag[1..].to_string()
                } else {
                    tag.to_string()
                }
            };

            let mut version_infos: Vec<VersionInfo> = match forge {
                ForgeType::GitHub => {
                    let releases =
                        github::list_releases_from_url(&api_url, &self.tool_name()).await?;
                    if releases.is_empty() {
                        // Fall back to tags (no created_at available)
                        github::list_tags_from_url(&api_url, &self.tool_name())
                            .await?
                            .into_iter()
                            .filter(|tag| matches_tag_regex(tag))
                            .map(|tag| {
                                let release_url =
                                    format!("{}/releases/tag/{}", release_url_base, tag);
                                VersionInfo {
                                    version: strip_v_prefix(&tag),
                                    release_url: Some(release_url),
                                    ..Default::default()
                                }
                            })
                            .collect()
                    } else {
                        releases
                            .into_iter()
                            .filter(|r| matches_tag_regex(&r.tag_name))
                            .map(|r| {
                                let release_url =
                                    format!("{}/releases/tag/{}", release_url_base, r.tag_name);
                                let created_at = Some(r.released_at().to_string());
                                VersionInfo {
                                    version: strip_v_prefix(&r.tag_name),
                                    created_at,
                                    release_url: Some(release_url),
                                    ..Default::default()
                                }
                            })
                            .collect()
                    }
                }
                ForgeType::GitLab => {
                    let releases =
                        gitlab::list_releases_from_url(&api_url, &self.tool_name()).await?;
                    if releases.is_empty() {
                        // Fall back to tags (no created_at available)
                        gitlab::list_tags_from_url(&api_url, &self.tool_name())
                            .await?
                            .into_iter()
                            .filter(|tag| matches_tag_regex(tag))
                            .map(|tag| {
                                // Use /-/tags/ for tag-only URLs (no release exists)
                                let release_url = format!("{}/-/tags/{}", release_url_base, tag);
                                VersionInfo {
                                    version: strip_v_prefix(&tag),
                                    release_url: Some(release_url),
                                    ..Default::default()
                                }
                            })
                            .collect()
                    } else {
                        releases
                            .into_iter()
                            .filter(|r| matches_tag_regex(&r.tag_name))
                            .map(|r| {
                                let release_url =
                                    format!("{}/-/releases/{}", release_url_base, r.tag_name);
                                VersionInfo {
                                    version: strip_v_prefix(&r.tag_name),
                                    created_at: r.published_at().map(str::to_string),
                                    release_url: Some(release_url),
                                    ..Default::default()
                                }
                            })
                            .collect()
                    }
                }
                _ => bail!("Unsupported forge type {:?}", forge),
            };

            // Sort: versions starting with digits first, then reverse
            version_infos.sort_by_cached_key(|vi| !regex!(r"^[0-9]").is_match(&vi.version));
            version_infos.reverse();

            Ok(version_infos)
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        deprecated_at!(
            "2026.4.0",
            "2027.1.0",
            "ubi",
            "The ubi backend is deprecated. Use the github backend instead (e.g., github:owner/repo)."
        );
        // Check if lockfile has URL for this platform
        let platform_key = self.get_platform_key();
        let lockfile_url = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|p| p.url.clone());

        let v = tv.version.to_string();
        let raw_opts = tv.request.options();
        let opts = UbiOptions::new(&raw_opts);
        let bin_path = opts.bin_path().unwrap_or_else(|| "bin".to_string());
        ensure_safe_relative_bin_dir("bin_path", &bin_path)?;
        opts.ensure_safe_exe_paths()?;
        let extract_all = opts.extract_all();
        let bin_dir = tv.install_path();

        // Use lockfile URL if available, otherwise fall back to standard resolution
        if let Some(url) = &lockfile_url {
            install(url, &v, &bin_dir, extract_all, &opts).await?;
        } else if name_is_url(&self.tool_name()) {
            install(&self.tool_name(), &v, &bin_dir, extract_all, &opts).await?;
        } else {
            try_with_v_prefix(&v, None, |candidate| {
                let bin_dir = bin_dir.clone();
                async move {
                    install(
                        &self.tool_name(),
                        &candidate,
                        &bin_dir,
                        extract_all,
                        &opts,
                    )
                    .await
                }
            })
            .await?;
        }

        let mut possible_exes = vec![
            opts.exe()
                .map(str::to_string)
                .unwrap_or(tv.ba().short.to_string()),
        ];
        if cfg!(windows) {
            possible_exes.push(format!("{}.exe", possible_exes[0]));
        }
        let full_binary_path = if let Some(bin_file) = possible_exes
            .into_iter()
            .map(|e| bin_dir.join(e))
            .find(|f| f.exists())
        {
            bin_file
        } else {
            let mut bin_dir = bin_dir.to_path_buf();
            if extract_all && bin_dir.join(&bin_path).exists() {
                bin_dir = bin_dir.join(&bin_path);
            }
            first_visible_file(&bin_dir)?
        };
        if extract_all {
            self.verify_extract_all_checksum(ctx, &mut tv, &bin_dir, &full_binary_path)?;
        } else {
            self.verify_checksum(ctx, &mut tv, &full_binary_path)?;
        }

        Ok(tv)
    }

    fn fuzzy_match_filter(
        &self,
        versions: Vec<String>,
        query: &str,
        filter_prereleases: bool,
    ) -> Vec<String> {
        ubi_fuzzy_match(versions, query, filter_prereleases)
    }

    fn verify_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file: &Path,
    ) -> eyre::Result<()> {
        let platform_key = self.checksum_platform_key(tv, &checksum_file_name(file)?);

        // Get or create platform info for this platform key
        let platform_info = tv.lock_platforms.entry(platform_key.clone()).or_default();

        if let Some(checksum) = &platform_info.checksum {
            ctx.pr
                .set_message(format!("checksum verify {platform_key}"));
            if let Some((algo, check)) = checksum.split_once(':') {
                hash::ensure_checksum(file, check, Some(ctx.pr.as_ref()), algo)?;
            } else {
                bail!("Invalid checksum: {platform_key}");
            }
        } else if Settings::get().lockfile_enabled() {
            ctx.pr
                .set_message(format!("checksum generate {platform_key}"));
            let hash = hash::file_hash_blake3(file, Some(ctx.pr.as_ref()))?;
            platform_info.checksum = Some(format!("blake3:{hash}"));
        }
        Ok(())
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<Vec<std::path::PathBuf>> {
        let raw_opts = tv.request.options();
        let opts = UbiOptions::new(&raw_opts);
        if let Some(bin_path) = opts.bin_path() {
            // bin_path should always point to a directory containing binaries
            ensure_safe_relative_bin_dir("bin_path", &bin_path)?;
            Ok(vec![runtime_path_for_install_path(
                tv,
                tv.install_path().join(&bin_path),
            )])
        } else if opts.extract_all() {
            Ok(vec![tv.runtime_path()])
        } else {
            let bin_path = tv.install_path().join("bin");
            if bin_path.exists() {
                Ok(vec![runtime_path_for_install_path(tv, bin_path)])
            } else {
                Ok(vec![tv.runtime_path()])
            }
        }
    }

    /// UBI is deprecated in favor of the github backend and doesn't resolve download URLs
    /// at lock time. Return false so --locked mode doesn't error for ubi tools.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        Ok(UbiOptions::new(&raw_opts).lockfile_options())
    }
}

/// Returns install-time-only option keys for UBI backend.
pub(crate) fn install_time_option_keys() -> Vec<String> {
    vec![
        "exe".into(),
        "matching".into(),
        "matching_regex".into(),
        "provider".into(),
    ]
}

impl UbiBackend {
    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    /// For ubi backend, generate a more specific platform key that includes tool-specific options
    fn checksum_platform_key(&self, tv: &ToolVersion, name: &str) -> String {
        let mut platform_key = self.get_platform_key();
        let raw_opts = tv.request.options();
        let opts = UbiOptions::new(&raw_opts);

        if let Some(exe) = opts.exe() {
            platform_key = format!("{platform_key}-{exe}");
        }
        if let Some(matching) = opts.matching() {
            platform_key = format!("{platform_key}-{matching}");
        }
        // Include filename to distinguish different downloads for the same platform
        format!("{platform_key}-{name}")
    }

    /// `verify_checksum` for an `extract_all` install. The whole release is unpacked into
    /// `install_dir` and any file of it can be the one that runs, so the checksum covers
    /// all of it, under a key that contains no name from the archive.
    ///
    /// A `mise.lock` can instead hold the checksum of one file of the install
    /// (`picked_file`), under that file's name. Such an entry says nothing about the
    /// other files, so it does not make an install a locked one.
    fn verify_extract_all_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        install_dir: &Path,
        picked_file: &Path,
    ) -> eyre::Result<()> {
        let platform_key = self.checksum_platform_key(tv, EXTRACT_ALL_CHECKSUM_NAME);
        let prefix = self.checksum_platform_key(tv, "");
        let own_musl = self.get_platform_key().ends_with("-musl");
        let picked_name = checksum_file_name(picked_file)?;
        if tv.lock_platforms.contains_key(&platform_key) {
            ctx.pr
                .set_message(format!("checksum verify {platform_key}"));
        }
        let checksums: Vec<(&str, &str)> = tv
            .lock_platforms
            .iter()
            .filter_map(|(key, p)| Some((key.as_str(), p.checksum.as_deref()?)))
            .collect();
        let lock = extract_all_lock(&checksums, &prefix, &picked_name, own_musl, || {
            dir_hash_blake3(install_dir)
        })?;
        match lock {
            ExtractAllLock::TreeMatches => return Ok(()),
            ExtractAllLock::TreeMismatch { expected, actual } => bail!(
                "Checksum mismatch for directory {}:\nExpected: blake3:{expected}\nActual:   blake3:{actual}",
                file::display_path(install_dir)
            ),
            ExtractAllLock::InvalidTree => bail!("Invalid checksum: {platform_key}"),
            ExtractAllLock::OneFile { key, picked } => {
                if ctx.locked {
                    bail!(
                        "The lockfile checksum of {} ({key}) covers one file of an extract_all install (--locked mode)\n\
                        hint: Reinstall it without locked mode to update the lockfile, or use the github backend",
                        tv.style()
                    );
                }
                // The names in this install come from the release, which must not get
                // to choose a file the lockfile has no checksum for.
                if !picked {
                    bail!(
                        "The lockfile checksum of {} ({key}) names a file this install does not use\n\
                        hint: Remove the entry from mise.lock to record a checksum of the whole install, or use the github backend",
                        tv.style()
                    );
                }
                self.verify_checksum(ctx, tv, picked_file)?;
            }
            ExtractAllLock::Unlocked => {}
        }
        if Settings::get().lockfile_enabled() {
            ctx.pr
                .set_message(format!("checksum generate {platform_key}"));
            let hash = dir_hash_blake3(install_dir)?;
            tv.lock_platforms.entry(platform_key).or_default().checksum =
                Some(format!("blake3:{hash}"));
        }
        Ok(())
    }
}

/// What the `mise.lock` checksum keys of one platform name for an `extract_all`
/// install. A key is `<prefix><name>`, where `<prefix>` holds the platform and the
/// `exe` and `matching` options, and `<name>` is `extract_all` for the checksum of
/// the whole install or a file's name for the checksum of that file. One key can
/// therefore name either, and this is the rule that tells them apart:
///
/// - the key of the picked file holds that file's checksum, whatever its name,
///   so a file named `tool-extract_all` or `musl-tool` keeps its checksum;
/// - the key `<prefix>extract_all` holds the checksum of the whole install, and,
///   when the picked file is named `extract_all`, may hold that file's instead;
/// - of the other keys, one ending in `-extract_all` is the whole-install key of
///   other options, and on glibc one starting with `musl-` belongs to the musl
///   platform, whose keys extend the glibc ones;
/// - every other key holds the checksum of another file of this install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtractAllKey {
    Tree,
    PickedFile,
    TreeOrPickedFile,
    OtherFile,
    Unrelated,
}

fn extract_all_key(key: &str, prefix: &str, picked_name: &str, own_musl: bool) -> ExtractAllKey {
    let Some(name) = key.strip_prefix(prefix) else {
        return ExtractAllKey::Unrelated;
    };
    match (name == EXTRACT_ALL_CHECKSUM_NAME, name == picked_name) {
        (true, true) => ExtractAllKey::TreeOrPickedFile,
        (true, false) => ExtractAllKey::Tree,
        (false, true) => ExtractAllKey::PickedFile,
        (false, false)
            if name.ends_with(&format!("-{EXTRACT_ALL_CHECKSUM_NAME}"))
                || (!own_musl && name.starts_with("musl-")) =>
        {
            ExtractAllKey::Unrelated
        }
        (false, false) => ExtractAllKey::OtherFile,
    }
}

/// The `mise.lock` checksum an `extract_all` install is held to; see [`ExtractAllKey`].
#[derive(Debug, PartialEq, Eq)]
enum ExtractAllLock {
    /// The checksum of the whole install matched.
    TreeMatches,
    TreeMismatch {
        expected: String,
        actual: String,
    },
    /// The whole-install checksum is not a blake3 one.
    InvalidTree,
    /// The checksum of one file, under `key`, which is the picked file's when `picked`.
    OneFile {
        key: String,
        picked: bool,
    },
    Unlocked,
}

/// Reads `checksums` (key and checksum) by the rule of [`ExtractAllKey`].
/// `tree_hash` is called only when there is a whole-install checksum.
fn extract_all_lock(
    checksums: &[(&str, &str)],
    prefix: &str,
    picked_name: &str,
    own_musl: bool,
    tree_hash: impl FnOnce() -> eyre::Result<String>,
) -> eyre::Result<ExtractAllLock> {
    let kind = |key: &str| extract_all_key(key, prefix, picked_name, own_musl);
    let tree = checksums.iter().find(|(key, _)| {
        matches!(
            kind(key),
            ExtractAllKey::Tree | ExtractAllKey::TreeOrPickedFile
        )
    });
    if let Some((key, checksum)) = tree {
        let actual = tree_hash()?;
        match checksum.strip_prefix("blake3:") {
            Some(expected) if actual == expected.to_lowercase() => {
                return Ok(ExtractAllLock::TreeMatches);
            }
            // The checksum of the picked file, read below.
            _ if kind(key) == ExtractAllKey::TreeOrPickedFile => {}
            Some(expected) => {
                return Ok(ExtractAllLock::TreeMismatch {
                    expected: expected.to_string(),
                    actual,
                });
            }
            None => return Ok(ExtractAllLock::InvalidTree),
        }
    }
    let mut other = None;
    for (key, _) in checksums {
        match kind(key) {
            ExtractAllKey::PickedFile | ExtractAllKey::TreeOrPickedFile => {
                return Ok(ExtractAllLock::OneFile {
                    key: key.to_string(),
                    picked: true,
                });
            }
            ExtractAllKey::OtherFile => other = other.or(Some(*key)),
            ExtractAllKey::Tree | ExtractAllKey::Unrelated => {}
        }
    }
    Ok(match other {
        Some(key) => ExtractAllLock::OneFile {
            key: key.to_string(),
            picked: false,
        },
        None => ExtractAllLock::Unlocked,
    })
}

/// What an `extract_all` install has in its `mise.lock` platform key where a single-file
/// install has the name of the file.
const EXTRACT_ALL_CHECKSUM_NAME: &str = "extract_all";

/// A blake3 digest of everything below `dir`: each entry's relative path and type, a
/// file's content and whether it is executable, and a symlink's target.
fn dir_hash_blake3(dir: &Path) -> eyre::Result<String> {
    let mut hasher = blake3::Hasher::new();
    // Each field with its length, so that two different trees never hash the same bytes.
    let mut field = |bytes: &[u8]| {
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    for entry in WalkDir::new(dir).min_depth(1).sort_by_file_name() {
        let entry = entry?;
        let path = entry.path();
        field(path.strip_prefix(dir)?.as_os_str().as_encoded_bytes());
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            field(b"symlink");
            field(std::fs::read_link(path)?.as_os_str().as_encoded_bytes());
        } else if file_type.is_dir() {
            field(b"dir");
        } else if file_type.is_file() {
            let kind: &[u8] = if file::is_executable(path) {
                b"executable"
            } else {
                b"file"
            };
            field(kind);
            field(hash::file_hash_blake3(path, None)?.as_bytes());
        } else {
            // Not opened: reading a FIFO would block.
            field(b"other");
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// The first file in `dir` whose name does not start with a dot.
fn first_visible_file(dir: &Path) -> Result<std::path::PathBuf> {
    file::ls(dir)?
        .into_iter()
        .find(|f| {
            !f.file_name()
                .is_some_and(|f| f.to_string_lossy().starts_with("."))
        })
        .ok_or_else(|| {
            eyre::eyre!(
                "ubi installed no file into {} whose name does not start with a dot",
                file::display_path(dir)
            )
        })
}

/// The name of an installed file, which its `mise.lock` checksum key ends with.
fn checksum_file_name(file: &Path) -> Result<String> {
    match file.file_name() {
        Some(name) => Ok(name.to_string_lossy().to_string()),
        None => bail!("{} is not a file", file::display_path(file)),
    }
}

fn name_is_url(n: &str) -> bool {
    n.starts_with("http")
}

/// The token ubi sends to `forge`: the one mise gives it (the enterprise token
/// for an `api_url` that is not the public API, else the forge token), or else
/// the one ubi reads from [`forge_token_env_var`] itself.
fn ubi_token(forge: &ForgeType, public_api: bool) -> Option<String> {
    let (token, enterprise_token) = match forge {
        ForgeType::GitHub => (&*GITHUB_TOKEN, &*MISE_GITHUB_ENTERPRISE_TOKEN),
        ForgeType::GitLab => (&*GITLAB_TOKEN, &*MISE_GITLAB_ENTERPRISE_TOKEN),
        _ => (&None, &None),
    };
    enterprise_token
        .clone()
        .filter(|_| !public_api)
        .or_else(|| token.clone())
        .or_else(|| forge_token_env_var(forge).and_then(|name| std::env::var(name).ok()))
}

/// The forge ubi talks to: a URL on codeberg.org or gitlab.com selects that
/// forge, any other URL GitHub, whatever `provider` says.
fn ubi_forge(name: &str, provider: &ForgeType) -> ForgeType {
    if !name_is_url(name) {
        return provider.clone();
    }
    let url = url::Url::parse(name).ok();
    match url.as_ref().and_then(|url| url.domain()) {
        Some("codeberg.org") => ForgeType::Forgejo,
        Some("gitlab.com") => ForgeType::GitLab,
        _ => ForgeType::GitHub,
    }
}

/// The environment variable ubi reads a token from for `forge`, when one is set.
fn forge_token_env_var(forge: &ForgeType) -> Option<&'static str> {
    let names: &[&'static str] = match forge {
        ForgeType::Forgejo => &["CODEBERG_TOKEN", "FORGEJO_TOKEN"],
        ForgeType::GitHub => &["GITHUB_TOKEN"],
        ForgeType::GitLab => &["CI_JOB_TOKEN", "GITLAB_TOKEN"],
    };
    names
        .iter()
        .copied()
        .find(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}

/// Whether `api_url` is the public GitHub or GitLab API, which ubi uses
/// without being told.
fn is_public_forge_api_url(api_url: &str) -> Result<bool> {
    let url =
        url::Url::parse(api_url).map_err(|e| eyre::eyre!("api_url {api_url} is not a URL: {e}"))?;
    Ok(matches!(
        url.host_str(),
        Some("github.com" | "api.github.com" | "gitlab.com")
    ))
}

/// The hosts a forge token may go to: the origin of the forge API, and for the
/// public GitHub API also github.com, which serves its release downloads.
struct TokenHosts {
    forge: ForgeType,
    /// The API ubi uses, without a trailing slash.
    api_url: String,
    api_origin: url::Origin,
    api_host: String,
    public_api: bool,
}

impl TokenHosts {
    fn new(forge: ForgeType, api_url: Option<&str>) -> Result<Self> {
        let public_api = match api_url {
            Some(api_url) => is_public_forge_api_url(api_url)?,
            None => true,
        };
        let api_url = match api_url.filter(|_| !public_api) {
            Some(api_url) => api_url,
            None => match forge {
                ForgeType::Forgejo => "https://codeberg.org/api/v1",
                ForgeType::GitHub => "https://api.github.com",
                ForgeType::GitLab => "https://gitlab.com/api/v4",
            },
        };
        let api_url = api_url.strip_suffix('/').unwrap_or(api_url);
        let parsed = url::Url::parse(api_url)?;
        Ok(Self {
            forge,
            api_url: api_url.to_string(),
            api_origin: parsed.origin(),
            api_host: parsed.host_str().unwrap_or_default().to_string(),
            public_api,
        })
    }

    fn allows(&self, url: &str) -> bool {
        let Ok(url) = url::Url::parse(url) else {
            return false;
        };
        url.origin() == self.api_origin
            || (self.public_api
                && self.forge == ForgeType::GitHub
                && url.scheme() == "https"
                && url.port().is_none()
                && url.host_str() == Some("github.com"))
    }

    /// A host this install would send `token` to that may not get it.
    async fn foreign_host(
        &self,
        name: &str,
        v: &str,
        token: Option<&str>,
    ) -> Result<Option<String>> {
        let host = |url: &str| {
            url::Url::parse(url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
                .unwrap_or_else(|| url.to_string())
        };
        if name_is_url(name) {
            return Ok((!self.allows(name)).then(|| host(name)));
        }
        let Some(token) = token.filter(|_| self.forge == ForgeType::GitLab) else {
            return Ok(None);
        };
        // GitLab release links can point at any host, and ubi downloads from them.
        // The release is read now and with ubi's token, as ubi reads it.
        let release = gitlab::get_release_now(&self.api_url, name, v, Some(token)).await?;
        Ok(release
            .assets
            .links
            .iter()
            .find(|link| !self.allows(&link.url))
            .map(|link| host(&link.url)))
    }
}

async fn install(
    name: &str,
    v: &str,
    bin_dir: &Path,
    extract_all: bool,
    opts: &UbiOptions<'_>,
) -> eyre::Result<()> {
    let mut builder = UbiBuilder::new().install_dir(bin_dir);

    if name_is_url(name) {
        builder = builder.url(name);
    } else {
        builder = builder.project(name);
        builder = builder.tag(v);
    }

    if extract_all {
        builder = builder.extract_all();
    } else {
        if let Some(exe) = opts.exe() {
            builder = builder.exe(exe);
        }
        if let Some(rename_exe) = opts.rename_exe() {
            builder = builder.rename_exe_to(rename_exe)
        }
    }
    if let Some(matching) = opts.matching() {
        builder = builder.matching(matching);
    }
    if let Some(matching_regex) = opts.matching_regex() {
        builder = builder.matching_regex(matching_regex);
    }

    let provider = opts.provider()?;
    builder = builder.forge(provider.clone());
    let forge = ubi_forge(name, &provider);
    let token_hosts = TokenHosts::new(forge.clone(), opts.api_url_override())?;
    if let Some(api_url) = opts.api_url_override()
        && !token_hosts.public_api
    {
        builder = builder.api_base_url(api_url.strip_suffix("/").unwrap_or(api_url));
    }
    // An `api_url` that project config named in safe mode gets no token; see
    // `mise_util::untrusted_hosts`.
    let token_denied = opts
        .api_url_override()
        .and_then(|api_url| url::Url::parse(api_url).ok())
        .and_then(|url| url.host_str().map(str::to_string))
        .is_some_and(|host| mise_util::untrusted_hosts::forge_token_denied(&host));
    // ubi sends its token with every request, the download included, and reads
    // one from the environment when it is given none.
    let env_token = forge_token_env_var(&forge);
    let token = ubi_token(&forge, token_hosts.public_api);
    if token_denied {
        if let Some(var) = env_token {
            bail!(
                "ubi would send the {var} token to {}, which mise does not send a token to here\nhint: unset {var}, or install the tool with the github or gitlab backend",
                token_hosts.api_host
            );
        }
    } else if let Some(host) = token_hosts.foreign_host(name, v, token.as_deref()).await? {
        if let Some(var) = env_token {
            bail!(
                "ubi would send the {var} token to {host}, which is not the {forge:?} API\nhint: unset {var}, or install the tool with the github, gitlab or http backend"
            );
        }
    } else if let Some(token) = &token {
        builder = builder.token(token);
    }

    let mut ubi = builder.build().map_err(|e| eyre::eyre!("{e:#}"))?;

    // TODO: hacky but does not compile without it
    tokio::task::block_in_place(|| {
        static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
        });
        RT.block_on(async { ubi.install_binary().await })
            .map_err(|e| eyre::eyre!("{e:#}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version too long to compile into a pattern matches nothing instead of
    /// stopping the resolution of every tool.
    #[test]
    fn an_overlong_query_matches_nothing() {
        let long = "1".repeat(400_000);
        assert!(ubi_fuzzy_match(vec!["1.0.0".to_string()], &long, true).is_empty());
        assert_eq!(
            ubi_fuzzy_match(vec!["1.0.0".to_string()], "1", true),
            vec!["1.0.0"]
        );
    }

    #[test]
    fn test_identity_options_ignore_tag_regex_only() {
        use crate::backend::static_helpers::test_identity_options;
        let backend = UbiBackend::from_arg(BackendArg::from("ubi:test/repo"));
        let identity = |options: &[(&str, &str)]| test_identity_options(&backend, "1.0.0", options);
        let base = identity(&[]);

        // A listing filter never changes what a concrete tag installs.
        assert_eq!(identity(&[("tag_regex", "^v\\d+")]), base);

        for (key, value) in [
            ("exe", "tool"),
            ("matching", "musl"),
            ("provider", "gitlab"),
            ("api_url", "https://ghe.example.com/api/v3"),
        ] {
            assert_ne!(
                identity(&[(key, value)]),
                base,
                "{key} changes what is installed"
            );
        }
    }

    #[test]
    fn test_public_forge_api_url_is_matched_on_the_host() {
        for api_url in [
            "https://api.github.com",
            "https://github.com/api/v3/",
            "https://GitLab.com/api/v4",
        ] {
            assert!(is_public_forge_api_url(api_url).unwrap(), "{api_url}");
        }
        for api_url in [
            "https://github.company.com/api/v3",
            "https://gitlab.company.com/api/v4",
            "https://gitlab.com.example/api/v4",
            "https://mirror.example/github.com/api/v3",
        ] {
            assert!(!is_public_forge_api_url(api_url).unwrap(), "{api_url}");
        }
        assert!(is_public_forge_api_url("ghe.example.com/api/v3").is_err());
    }

    #[test]
    fn test_token_hosts_are_the_forge_api() {
        let github = TokenHosts::new(ForgeType::GitHub, None).unwrap();
        assert!(github.allows("https://api.github.com/repos/o/r/releases/assets/1"));
        assert!(github.allows("https://github.com/o/r/releases/download/v1/tool.tar.gz"));
        assert!(!github.allows("http://api.github.com/repos/o/r/releases/assets/1"));
        assert!(!github.allows("https://github.com.example/tool.tar.gz"));
        assert!(!github.allows("https://downloads.example/tool.tar.gz"));

        let enterprise =
            TokenHosts::new(ForgeType::GitHub, Some("https://ghe.example.com/api/v3/")).unwrap();
        assert!(!enterprise.public_api);
        assert!(enterprise.allows("https://ghe.example.com/api/v3/repos/o/r/releases/assets/1"));
        assert!(!enterprise.allows("https://api.github.com/repos/o/r/releases/assets/1"));
        assert!(!enterprise.allows("https://github.com/o/r/releases/download/v1/tool.tar.gz"));

        let gitlab = TokenHosts::new(ForgeType::GitLab, Some("https://gitlab.com/api/v4")).unwrap();
        assert!(gitlab.public_api);
        assert!(gitlab.allows("https://gitlab.com/o/r/-/releases/v1/downloads/tool.tar.gz"));
        assert!(!gitlab.allows("https://cdn.example/tool.tar.gz"));
        assert!(!gitlab.allows("https://github.com/o/r/releases/download/v1/tool.tar.gz"));
    }

    #[tokio::test]
    async fn test_gitlab_release_links_are_read_now_with_ubi_token() {
        let mut server = mockito::Server::new_async().await;
        let api_url = server.url();
        let release = |link: &str| {
            serde_json::json!({
                "tag_name": "v1",
                "description": null,
                "released_at": null,
                "assets": {"sources": [], "links": [{
                    "id": 1,
                    "name": "tool.tar.gz",
                    "url": link,
                    "direct_asset_url": link,
                    "link_type": "package",
                }]},
            })
            .to_string()
        };
        let path = "/projects/o%2Fr/releases/v1";
        let hosts = TokenHosts::new(ForgeType::GitLab, Some(&format!("{api_url}/"))).unwrap();
        assert!(!hosts.public_api);

        // A job token is sent as ubi sends it.
        let on_gitlab = server
            .mock("GET", path)
            .match_header("authorization", "Bearer job-token")
            .with_header("content-type", "application/json")
            .with_body(release(&format!(
                "{api_url}/o/r/-/releases/v1/downloads/tool.tar.gz"
            )))
            .expect(1)
            .create_async()
            .await;
        let host = hosts
            .foreign_host("o/r", "v1", Some("job-token"))
            .await
            .unwrap();
        assert_eq!(host, None);
        on_gitlab.assert_async().await;
        on_gitlab.remove_async().await;

        // A link changed since is seen on the next install.
        let elsewhere = server
            .mock("GET", path)
            .match_header("authorization", "Bearer job-token")
            .with_header("content-type", "application/json")
            .with_body(release("https://downloads.example/tool.tar.gz"))
            .expect(1)
            .create_async()
            .await;
        let host = hosts
            .foreign_host("o/r", "v1", Some("job-token"))
            .await
            .unwrap();
        assert_eq!(host.as_deref(), Some("downloads.example"));
        elsewhere.assert_async().await;

        // With no token there is nothing to send, and nothing is read.
        let host = hosts.foreign_host("o/r", "v1", None).await.unwrap();
        assert_eq!(host, None);
    }

    #[test]
    fn test_ubi_forge_follows_the_url_host() {
        let gitlab_url = "https://gitlab.com/o/r/-/releases/v1/downloads/tool.tar.gz";
        assert_eq!(ubi_forge(gitlab_url, &ForgeType::GitHub), ForgeType::GitLab);
        let other_url = "https://downloads.example/tool.tar.gz";
        assert_eq!(ubi_forge(other_url, &ForgeType::GitLab), ForgeType::GitHub);
        assert_eq!(ubi_forge("o/r", &ForgeType::GitLab), ForgeType::GitLab);
    }

    #[test]
    fn test_dir_hash_blake3_covers_every_entry() {
        let tree = |files: &[(&str, &str)]| {
            let dir = tempfile::tempdir().unwrap();
            for (path, content) in files {
                let path = dir.path().join(path);
                file::create_dir_all(path.parent().unwrap()).unwrap();
                file::write(&path, content).unwrap();
            }
            dir
        };
        let hash = |dir: &tempfile::TempDir| dir_hash_blake3(dir.path()).unwrap();
        let genuine = [("LICENSE", "MIT"), ("tool", "genuine"), ("lib/a.so", "a")];
        let base = hash(&tree(&genuine));

        // The same tree somewhere else has the same digest.
        assert_eq!(hash(&tree(&genuine)), base);

        for changed in [
            &[("LICENSE", "MIT"), ("tool", "replaced"), ("lib/a.so", "a")][..],
            &[("LICENSE", "MIT"), ("tool", "genuine"), ("lib/a.so", "b")],
            &[("LICENSE", "MIT"), ("tool", "genuine"), ("lib/b.so", "a")],
            &[("LICENSE", "MIT"), ("tool", "genuine"), ("a.so", "a")],
            &[("LICENSE", "MIT"), ("tool", "genuine")],
            &[
                ("LICENSE", "MIT"),
                ("tool", "genuine"),
                ("lib/a.so", "a"),
                (".hidden", ""),
            ],
            &[
                ("LICENSE", "MIT"),
                ("tool", "genuine"),
                ("lib/a.so", "a"),
                ("bin/tool", "genuine"),
            ],
        ] {
            assert_ne!(hash(&tree(changed)), base, "{changed:?}");
        }

        #[cfg(unix)]
        {
            let dir = tree(&genuine);
            file::make_executable(dir.path().join("tool")).unwrap();
            let executable = hash(&dir);
            assert_ne!(executable, base);

            // A symlink is hashed as its target path, not as what it points to.
            file::remove_file(dir.path().join("LICENSE")).unwrap();
            std::os::unix::fs::symlink("tool", dir.path().join("LICENSE")).unwrap();
            let to_tool = hash(&dir);
            file::remove_file(dir.path().join("LICENSE")).unwrap();
            std::os::unix::fs::symlink("lib/a.so", dir.path().join("LICENSE")).unwrap();
            assert_ne!(to_tool, executable);
            assert_ne!(hash(&dir), to_tool);
        }
    }

    #[test]
    fn test_extract_all_lock_reads_keys_by_one_rule() {
        use ExtractAllLock::*;
        let one_file = |key: &str, picked: bool| OneFile {
            key: key.to_string(),
            picked,
        };
        let glibc = "linux-x64-";
        let musl = "linux-x64-musl-";
        // (entries, prefix, picked file, own musl, expected); the install hashes to "aa".
        type Case<'a> = (
            Vec<(&'a str, &'a str)>,
            &'a str,
            &'a str,
            bool,
            ExtractAllLock,
        );
        let cases: Vec<Case> = vec![
            (
                vec![("linux-x64-extract_all", "blake3:aa")],
                glibc,
                "tool",
                false,
                TreeMatches,
            ),
            (
                vec![("linux-x64-extract_all", "blake3:bb")],
                glibc,
                "tool",
                false,
                TreeMismatch {
                    expected: "bb".into(),
                    actual: "aa".into(),
                },
            ),
            (
                vec![("linux-x64-extract_all", "sha256:aa")],
                glibc,
                "tool",
                false,
                InvalidTree,
            ),
            // A file named `extract_all` shares the whole-install key.
            (
                vec![("linux-x64-extract_all", "blake3:aa")],
                glibc,
                "extract_all",
                false,
                TreeMatches,
            ),
            (
                vec![("linux-x64-extract_all", "blake3:ff")],
                glibc,
                "extract_all",
                false,
                one_file("linux-x64-extract_all", true),
            ),
            // The picked file's key is its checksum, whatever the name.
            (
                vec![("linux-x64-tool-extract_all", "blake3:ff")],
                glibc,
                "tool-extract_all",
                false,
                one_file("linux-x64-tool-extract_all", true),
            ),
            (
                vec![("linux-x64-musl-tool", "blake3:ff")],
                glibc,
                "musl-tool",
                false,
                one_file("linux-x64-musl-tool", true),
            ),
            (
                vec![
                    ("linux-x64-LICENSE", "blake3:ff"),
                    ("linux-x64-tool", "blake3:ff"),
                ],
                glibc,
                "tool",
                false,
                one_file("linux-x64-tool", true),
            ),
            // Other options' whole-install key, and the musl platform's keys.
            (
                vec![("linux-x64-tool-extract_all", "blake3:ff")],
                glibc,
                "tool",
                false,
                Unlocked,
            ),
            (
                vec![("linux-x64-musl-tool", "blake3:ff")],
                glibc,
                "tool",
                false,
                Unlocked,
            ),
            (
                vec![("linux-x64-musl-extract_all", "blake3:ff")],
                glibc,
                "tool",
                false,
                Unlocked,
            ),
            (
                vec![("macos-arm64-tool", "blake3:ff")],
                glibc,
                "tool",
                false,
                Unlocked,
            ),
            // Any other key is another file of this install.
            (
                vec![("linux-x64-LICENSE", "blake3:ff")],
                glibc,
                "tool",
                false,
                one_file("linux-x64-LICENSE", false),
            ),
            (
                vec![("linux-x64-musl-tool", "blake3:ff")],
                musl,
                "tool",
                true,
                one_file("linux-x64-musl-tool", true),
            ),
            (
                vec![("linux-x64-musl-extract_all", "blake3:aa")],
                musl,
                "tool",
                true,
                TreeMatches,
            ),
        ];
        for (entries, prefix, picked, own_musl, expected) in cases {
            let lock =
                extract_all_lock(&entries, prefix, picked, own_musl, || Ok("aa".into())).unwrap();
            assert_eq!(lock, expected, "{entries:?} picking {picked}");
        }
    }

    #[test]
    fn test_first_visible_file_fails_without_one() {
        let dir = tempfile::tempdir().unwrap();
        assert!(first_visible_file(dir.path()).is_err());
        assert!(first_visible_file(&dir.path().join("bin")).is_err());
        file::write(dir.path().join(".tool"), "").unwrap();
        let err = first_visible_file(dir.path()).unwrap_err();
        assert!(err.to_string().contains("no file"), "{err}");
        file::write(dir.path().join("tool"), "").unwrap();
        assert_eq!(
            first_visible_file(dir.path()).unwrap(),
            dir.path().join("tool")
        );
    }

    #[test]
    fn test_exe_paths_stay_in_the_install_dir() {
        let opts = |key: &str, value: &str| {
            let mut opts = ToolVersionOptions::default();
            opts.opts
                .insert(key.to_string(), toml::Value::String(value.to_string()));
            opts
        };
        for key in ["exe", "rename_exe"] {
            for value in ["../../../.bashrc", "/home/user/.bashrc", "bin/../../tool"] {
                let err = UbiOptions::new(&opts(key, value))
                    .ensure_safe_exe_paths()
                    .unwrap_err();
                assert!(err.to_string().starts_with(key), "{err}");
            }
            for value in ["tool", "bin/tool"] {
                UbiOptions::new(&opts(key, value))
                    .ensure_safe_exe_paths()
                    .unwrap();
            }
        }
    }

    #[test]
    fn test_extract_all_accepts_native_bool_or_string() {
        let mut bool_opts = ToolVersionOptions::default();
        bool_opts
            .opts
            .insert("extract_all".to_string(), toml::Value::Boolean(true));
        assert!(UbiOptions::new(&bool_opts).extract_all());

        let mut string_opts = ToolVersionOptions::default();
        string_opts.opts.insert(
            "extract_all".to_string(),
            toml::Value::String("true".to_string()),
        );
        assert!(UbiOptions::new(&string_opts).extract_all());

        let mut invalid_opts = ToolVersionOptions::default();
        invalid_opts.opts.insert(
            "extract_all".to_string(),
            toml::Value::String("yes".to_string()),
        );
        assert!(!UbiOptions::new(&invalid_opts).extract_all());
    }
}
