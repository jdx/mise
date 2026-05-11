use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::hash::Hash;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex as TokioMutex;

use jiff::Timestamp;

use crate::cli::args::{BackendArg, ToolVersionType};
use crate::cmd::CmdLineRunner;
use crate::config::config_file::config_root;
use crate::config::{Config, Settings};
use crate::duration::parse_into_timestamp;
use crate::file::{display_path, remove_all_with_progress, remove_all_with_warning};
use crate::install_before::resolve_before_date;
use crate::install_context::InstallContext;
use crate::lockfile::{PlatformInfo, ProvenanceType};
use crate::path_env::PathEnv;
use crate::platform::Platform;
use crate::plugins::core::CORE_PLUGINS;
use crate::plugins::{PEP440_PRERELEASE_REGEX, PluginType, VERSION_REGEX};
use crate::registry::{REGISTRY, full_to_url, normalize_remote, tool_enabled};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::tera::get_tera;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{
    ResolveOptions, ToolOptionSource, ToolRequest, ToolVersion, Toolset, install_state,
    is_outdated_version,
};
use crate::ui::progress_report::SingleReport;
use crate::{
    cache::{CacheManager, CacheManagerBuilder},
    plugins::PluginEnum,
};
use crate::{dirs, env, file, hash, lock_file, versions_host};
use async_trait::async_trait;
use backend_type::BackendType;
use eyre::{Result, bail, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use platform_target::PlatformTarget;
use regex::Regex;
use std::sync::LazyLock as Lazy;
use versions::Versioning;

pub mod aqua;
pub mod asdf;
pub mod asset_matcher;
pub mod backend_type;
pub mod cargo;
pub mod conda;
pub mod dotnet;
mod external_plugin_cache;
pub mod gem;
pub mod github;
pub mod go;
pub mod http;
pub mod jq;
pub mod npm;
pub mod pipx;
pub mod platform_target;
pub mod s3;
pub mod spm;
pub mod static_helpers;
pub mod ubi;
pub mod version_list;
pub mod vfox;

pub type ABackend = Arc<dyn Backend>;
pub type BackendMap = BTreeMap<String, ABackend>;
pub type BackendList = Vec<ABackend>;
pub type VersionCacheManager = CacheManager<Vec<VersionInfo>>;

const VERSIONS_HOST_LOCAL_OPT_SOURCES: &[ToolOptionSource] = &[
    ToolOptionSource::BackendAlias,
    ToolOptionSource::Config,
    ToolOptionSource::InlineBackendArg,
];

fn has_local_version_listing_option_override(
    resolved_opts: &crate::toolset::ResolvedToolOptions,
    version_listing_opt_keys: &[&str],
) -> bool {
    resolved_opts
        .has_any_key_from_sources(version_listing_opt_keys, VERSIONS_HOST_LOCAL_OPT_SOURCES)
}

static STRICT_METADATA: AtomicBool = AtomicBool::new(false);

pub fn set_strict_metadata(strict: bool) {
    STRICT_METADATA.store(strict, Ordering::Relaxed);
}

pub fn strict_metadata() -> bool {
    STRICT_METADATA.load(Ordering::Relaxed)
}

/// Information about a GitHub/GitLab release for platform-specific tools
#[derive(Debug, Clone)]
pub struct GitHubReleaseInfo {
    pub repo: String,
    pub asset_pattern: Option<String>,
    pub api_url: Option<String>,
    pub release_type: ReleaseType,
}

#[derive(Debug, Clone)]
pub enum ReleaseType {
    GitHub,
    GitLab,
}

/// Information about a tool version including optional metadata like creation time
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct VersionInfo {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub created_at: Option<String>,
    /// URL to the release page (e.g., GitHub/GitLab release page)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub release_url: Option<String>,
    /// If true, this is a rolling release (like "nightly") that should always
    /// be considered potentially outdated for `mise up` purposes
    #[serde(default, skip_serializing_if = "is_false")]
    pub rolling: bool,
    /// Checksum of the release asset, used to detect changes in rolling releases
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub checksum: Option<String>,
    /// Whether this is a pre-release. Backends with a reliable upstream signal
    /// (e.g. GitHub releases' `prerelease: true`) populate this directly.
    /// Metadata-free listing backends can opt in to stamping this from mise's
    /// legacy pre-release pattern before caching.
    #[serde(default, skip_serializing_if = "is_false")]
    pub prerelease: bool,
}

fn is_false(v: &bool) -> bool {
    !v
}

impl VersionInfo {
    /// Filter versions to only include those released before the given timestamp.
    /// Versions without a created_at timestamp are included by default.
    pub fn filter_by_date(versions: Vec<Self>, before: Timestamp) -> Vec<Self> {
        use crate::duration::parse_into_timestamp;
        versions
            .into_iter()
            .filter(|v| {
                match &v.created_at {
                    Some(ts) => {
                        // Parse the timestamp using parse_into_timestamp which handles
                        // RFC3339, date-only (YYYY-MM-DD), and other formats
                        match parse_into_timestamp(ts) {
                            Ok(created) => created < before,
                            Err(_) => {
                                // If we can't parse the timestamp, include the version
                                trace!("Failed to parse timestamp: {}", ts);
                                true
                            }
                        }
                    }
                    // Include versions without timestamps
                    None => true,
                }
            })
            .collect()
    }
}

/// Security feature information for a tool
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SecurityFeature {
    Checksum {
        #[serde(skip_serializing_if = "Option::is_none")]
        algorithm: Option<String>,
    },
    GithubAttestations {
        #[serde(skip_serializing_if = "Option::is_none")]
        signer_workflow: Option<String>,
    },
    Slsa {
        #[serde(skip_serializing_if = "Option::is_none")]
        level: Option<u8>,
    },
    Cosign,
    Minisign {
        #[serde(skip_serializing_if = "Option::is_none")]
        public_key: Option<String>,
    },
    Gpg,
}

static TOOLS: Mutex<Option<Arc<BackendMap>>> = Mutex::new(None);

pub async fn load_tools() -> Result<Arc<BackendMap>> {
    if let Some(memo_tools) = TOOLS.lock().unwrap().clone() {
        return Ok(memo_tools);
    }
    install_state::init().await?;
    time!("load_tools start");
    let core_tools = CORE_PLUGINS.values().cloned().collect::<Vec<ABackend>>();
    let mut tools = core_tools;
    // add tools with idiomatic files so they get parsed even if no versions are installed
    tools.extend(
        REGISTRY
            .values()
            .filter(|rt| !rt.idiomatic_files.is_empty() && rt.is_supported_os())
            .filter_map(|rt| arg_to_backend(rt.short.into())),
    );
    time!("load_tools core");
    tools.extend(
        install_state::list_tools()
            .values()
            .filter(|ist| ist.full.is_some())
            .flat_map(|ist| arg_to_backend(ist.clone().into())),
    );
    time!("load_tools install_state");
    let settings = Settings::get();
    let enable_tools = settings.enable_tools();
    let disable_tools = settings.disable_tools();
    tools.retain(|backend| {
        tool_enabled(
            enable_tools.as_ref(),
            &disable_tools,
            &backend.id().to_string(),
        )
    });
    tools.retain(|backend| {
        !settings
            .disable_backends
            .contains(&backend.get_type().to_string())
    });

    let tools: BackendMap = tools
        .into_iter()
        .map(|backend| (backend.ba().short.clone(), backend))
        .collect();
    let tools = Arc::new(tools);
    *TOOLS.lock().unwrap() = Some(tools.clone());
    time!("load_tools done");
    Ok(tools)
}

pub fn list() -> BackendList {
    TOOLS
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .values()
        .cloned()
        .collect()
}

pub fn get(ba: &BackendArg) -> Option<ABackend> {
    // Inline opts are command-scoped, so a short-name cache hit must not drop
    // the caller's BackendArg options.
    if ba.explicit_opts().is_some() {
        return arg_to_backend(ba.clone());
    }

    let mut tools = TOOLS.lock().unwrap();
    let tools_ = tools.as_ref().unwrap();
    if let Some(backend) = tools_.get(&ba.short) {
        Some(backend.clone())
    } else if let Some(backend) = arg_to_backend(ba.clone()) {
        let mut tools_ = tools_.deref().clone();
        tools_.insert(ba.short.clone(), backend.clone());
        *tools = Some(Arc::new(tools_));
        Some(backend)
    } else {
        None
    }
}

pub fn remove(short: &str) {
    let mut tools = TOOLS.lock().unwrap();
    if let Some(current) = tools.as_ref() {
        let mut tools_ = current.deref().clone();
        tools_.remove(short);
        *tools = Some(Arc::new(tools_));
    }
}

pub fn arg_to_backend(ba: BackendArg) -> Option<ABackend> {
    match ba.backend_type() {
        BackendType::Core => {
            CORE_PLUGINS
                .get(&ba.short)
                .or_else(|| {
                    // this can happen if something like "corenode" is aliased to "core:node"
                    ba.full()
                        .strip_prefix("core:")
                        .and_then(|short| CORE_PLUGINS.get(short))
                })
                .cloned()
        }
        BackendType::Aqua => Some(Arc::new(aqua::AquaBackend::from_arg(ba))),
        BackendType::Asdf => Some(Arc::new(asdf::AsdfBackend::from_arg(ba))),
        BackendType::Cargo => Some(Arc::new(cargo::CargoBackend::from_arg(ba))),
        BackendType::Conda => Some(Arc::new(conda::CondaBackend::from_arg(ba))),
        BackendType::Dotnet => Some(Arc::new(dotnet::DotnetBackend::from_arg(ba))),
        BackendType::Forgejo => Some(Arc::new(github::UnifiedGitBackend::from_arg(ba))),
        BackendType::Gem => Some(Arc::new(gem::GemBackend::from_arg(ba))),
        BackendType::Github => Some(Arc::new(github::UnifiedGitBackend::from_arg(ba))),
        BackendType::Gitlab => Some(Arc::new(github::UnifiedGitBackend::from_arg(ba))),
        BackendType::Go => Some(Arc::new(go::GoBackend::from_arg(ba))),
        BackendType::Npm => Some(Arc::new(npm::NPMBackend::from_arg(ba))),
        BackendType::Pipx => Some(Arc::new(pipx::PIPXBackend::from_arg(ba))),
        BackendType::Spm => Some(Arc::new(spm::SPMBackend::from_arg(ba))),
        BackendType::Http => Some(Arc::new(http::HttpBackend::from_arg(ba))),
        BackendType::S3 => Some(Arc::new(s3::S3Backend::from_arg(ba))),
        BackendType::Ubi => Some(Arc::new(ubi::UbiBackend::from_arg(ba))),
        BackendType::Vfox => Some(Arc::new(vfox::VfoxBackend::from_arg(ba, None))),
        BackendType::VfoxBackend(plugin_name) => Some(Arc::new(vfox::VfoxBackend::from_arg(
            ba,
            Some(plugin_name.to_string()),
        ))),
        BackendType::Unknown => None,
    }
}

/// Returns install-time-only option keys for a backend type.
/// These are options that only affect installation/download, not post-install behavior.
/// Used to filter cached options when config provides its own options.
pub fn install_time_option_keys_for_type(backend_type: &BackendType) -> Vec<String> {
    match backend_type {
        BackendType::Http => http::install_time_option_keys(),
        BackendType::S3 => s3::install_time_option_keys(),
        BackendType::Github | BackendType::Gitlab => github::install_time_option_keys(),
        BackendType::Ubi => ubi::install_time_option_keys(),
        BackendType::Cargo => cargo::install_time_option_keys(),
        BackendType::Go => go::install_time_option_keys(),
        BackendType::Npm => npm::install_time_option_keys(),
        BackendType::Pipx => pipx::install_time_option_keys(),
        BackendType::Aqua => aqua::install_time_option_keys(),
        _ => vec![],
    }
}

/// Returns true if a backend option only affects installation/download.
/// Used to filter cached options when config provides its own options.
pub fn is_install_time_option_key_for_type(backend_type: &BackendType, key: &str) -> bool {
    if matches!(backend_type, BackendType::Aqua) {
        return aqua::is_install_time_option_key(key);
    }

    let install_time_keys = install_time_option_keys_for_type(backend_type);
    install_time_keys.iter().any(|itk| itk == key)
        || install_time_keys
            .iter()
            .any(|itk| key.starts_with("platforms.") && key.ends_with(&format!(".{itk}")))
}

/// Normalize idiomatic file contents by removing comments and empty lines.
/// Full-line and inline comments are supported by .python-version, .nvmrc, etc.
pub(crate) fn normalize_idiomatic_contents(contents: &str) -> String {
    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();

            // Skip empty lines or lines that are entirely comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }

            // Find an inline comment marked by a `#` preceded by whitespace, preserving valid `#` chars in versions like `tool#tag`
            let comment_idx = trimmed.char_indices().find_map(|(i, c)| {
                if c == '#' && trimmed[..i].ends_with(char::is_whitespace) {
                    Some(i)
                } else {
                    None
                }
            });

            // Strip the inline comment if found, otherwise retain the whole trimmed string
            let without_inline = if let Some(idx) = comment_idx {
                trimmed[..idx].trim()
            } else {
                trimmed
            };

            // Double check the line hasn't become empty after stripping the comment
            if without_inline.is_empty() {
                None
            } else {
                Some(without_inline)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_idiomatic_contents() {
        assert_eq!(normalize_idiomatic_contents("tool # and a comment"), "tool");
        assert_eq!(normalize_idiomatic_contents("tool#tag"), "tool#tag");
        assert_eq!(
            normalize_idiomatic_contents("tool#tag # comment"),
            "tool#tag"
        );
        assert_eq!(normalize_idiomatic_contents("   # full line comment"), "");
        assert_eq!(
            normalize_idiomatic_contents("3.12.3\n3.11.11"),
            "3.12.3\n3.11.11"
        );
        assert_eq!(
            normalize_idiomatic_contents("3.12.3 # inline\n# comment\n3.11.11"),
            "3.12.3\n3.11.11"
        );
        assert_eq!(
            normalize_idiomatic_contents("# full line comment\n3.14.2 # inline comment\n   \n\n"),
            "3.14.2"
        );
    }

    #[test]
    fn test_remote_version_listing_opts_are_backend_specific() {
        use crate::toolset::{ResolvedToolOptions, ToolOptionSource, ToolVersionOptions};

        let mut install_only_opts = ToolVersionOptions::default();
        install_only_opts.opts.insert(
            "asset_pattern".to_string(),
            toml::Value::String("tool-{{version}}.tar.gz".into()),
        );
        let mut resolved = ResolvedToolOptions::default();
        resolved.apply_overrides(&install_only_opts, ToolOptionSource::Config);

        assert!(!has_local_version_listing_option_override(
            &resolved,
            &["api_url", "version_prefix"],
        ));

        let mut listing_opts = ToolVersionOptions::default();
        listing_opts.opts.insert(
            "api_url".to_string(),
            toml::Value::String("https://github.example.com/api/v3".into()),
        );
        resolved.apply_overrides(&listing_opts, ToolOptionSource::Config);

        assert!(has_local_version_listing_option_override(
            &resolved,
            &["api_url", "version_prefix"],
        ));
    }

    #[test]
    fn test_remote_version_listing_opts_ignore_registry_sources() {
        use crate::toolset::{ResolvedToolOptions, ToolOptionSource, ToolVersionOptions};

        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "version_prefix".to_string(),
            toml::Value::String("release-".into()),
        );
        let mut resolved = ResolvedToolOptions::default();
        resolved.apply_overrides(&opts, ToolOptionSource::Registry);

        assert!(!has_local_version_listing_option_override(
            &resolved,
            &["api_url", "version_prefix"],
        ));
    }

    #[test]
    fn test_fuzzy_match_versions_filters_prereleases_by_default() {
        let versions = vec![
            "1.0.0".to_string(),
            "1.1.0-rc1".to_string(),
            "1.1.0".to_string(),
            "1.2.0-dev.5".to_string(),
        ];
        let got = fuzzy_match_versions(versions, "latest", true);
        assert_eq!(got, vec!["1.0.0".to_string(), "1.1.0".to_string()]);
    }

    #[test]
    fn test_fuzzy_match_versions_includes_prereleases_when_opted_in() {
        let versions = vec![
            "1.0.0".to_string(),
            "1.1.0-rc1".to_string(),
            "1.1.0".to_string(),
            "1.2.0-dev.5".to_string(),
            "0.1.2-dev.86".to_string(),
        ];
        let got = fuzzy_match_versions(versions.clone(), "latest", false);
        assert_eq!(got, versions);
    }

    #[test]
    fn test_fuzzy_match_versions_partial_query_respects_prerelease_flag() {
        let versions = vec![
            "1.2.0".to_string(),
            "1.2.1-rc1".to_string(),
            "1.2.1".to_string(),
        ];
        assert_eq!(
            fuzzy_match_versions(versions.clone(), "1.2", true),
            vec!["1.2.0".to_string(), "1.2.1".to_string()]
        );
        assert_eq!(
            fuzzy_match_versions(versions.clone(), "1.2", false),
            versions
        );
    }

    #[test]
    fn test_fuzzy_match_versions_pep440_drops_alphas_but_honors_exact_match() {
        let versions = vec![
            "3.13.0".to_string(),
            "3.14.0a1".to_string(),
            "3.14.0".to_string(),
            "3.15.0a8".to_string(),
        ];
        // `latest` resolution skips PEP 440 prereleases.
        assert_eq!(
            fuzzy_match_versions_pep440(versions.clone(), "latest", true),
            vec!["3.13.0".to_string(), "3.14.0".to_string()]
        );
        // Explicit prerelease request still resolves — preserves the
        // exact-match bypass that `fuzzy_match_versions` already provides for
        // generic prereleases like `1.0.0-rc1`.
        assert_eq!(
            fuzzy_match_versions_pep440(versions.clone(), "3.14.0a1", true),
            vec!["3.14.0a1".to_string()]
        );
        // Opting in to prereleases keeps the full list.
        assert_eq!(
            fuzzy_match_versions_pep440(versions.clone(), "latest", false),
            versions
        );
    }

    #[test]
    fn test_filter_cached_prereleases_drops_flagged_entries_by_default() {
        // The cache stores the pre-release superset; `prerelease = false` (the
        // default) must filter out entries flagged by the upstream so the user
        // sees the same list whether or not pre-releases were ever fetched.
        let cached = vec![
            VersionInfo {
                version: "1.0.0".into(),
                ..Default::default()
            },
            VersionInfo {
                version: "1.1.0-rc1".into(),
                prerelease: true,
                ..Default::default()
            },
            VersionInfo {
                version: "1.1.0".into(),
                ..Default::default()
            },
        ];

        // Default opt: pre-releases dropped.
        let stable = filter_cached_prereleases(cached.clone(), false);
        let stable_versions: Vec<_> = stable.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(stable_versions, vec!["1.0.0", "1.1.0"]);

        // Opted in: same cache, full list returned without refetch.
        let all = filter_cached_prereleases(cached, true);
        let all_versions: Vec<_> = all.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(all_versions, vec!["1.0.0", "1.1.0-rc1", "1.1.0"]);
    }

    #[test]
    fn test_filter_cached_prereleases_leaves_unflagged_backends_alone() {
        // The cache-layer filter only trusts the metadata bit. Regex-shaped
        // versions are stamped before they enter the cache.
        let cached = vec![
            VersionInfo {
                version: "1.0.0".into(),
                ..Default::default()
            },
            VersionInfo {
                version: "1.1.0-rc1".into(),
                ..Default::default()
            },
        ];
        let filtered = filter_cached_prereleases(cached.clone(), false);
        let versions: Vec<_> = filtered.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(versions, vec!["1.0.0", "1.1.0-rc1"]);
    }

    #[test]
    fn test_mark_prerelease_flags_regex_matches() {
        let stable = mark_prerelease(VersionInfo {
            version: "1.0.0".into(),
            ..Default::default()
        });
        assert!(!stable.prerelease);

        let rc = mark_prerelease(VersionInfo {
            version: "1.1.0-rc1".into(),
            ..Default::default()
        });
        assert!(rc.prerelease);

        let already_flagged = mark_prerelease(VersionInfo {
            version: "2.0.0".into(),
            prerelease: true,
            ..Default::default()
        });
        assert!(already_flagged.prerelease);

        // Go pseudo-version (`-DATE-HASH`) must not false-positive on the
        // `[abc][0-9]+` alternative — that pattern lives in
        // PEP440_PRERELEASE_REGEX (pipx-only), not the general regex.
        let go_pseudo = mark_prerelease(VersionInfo {
            version: "2.0.0-20260404020628-f149714c1d54".into(),
            ..Default::default()
        });
        assert!(
            !go_pseudo.prerelease,
            "Go pseudo-version must not be flagged by the general regex"
        );

        // PEP 440 separator-less alpha is similarly not the general regex's
        // concern — pipx applies that rule itself.
        let py_alpha = mark_prerelease(VersionInfo {
            version: "3.12.0a1".into(),
            ..Default::default()
        });
        assert!(!py_alpha.prerelease);
    }

    #[test]
    fn test_include_prereleases_accepts_bool_and_string_values() {
        use crate::toolset::ToolVersionOptions;

        let backend = TestBackend::default();
        let mut opts = ToolVersionOptions::default();
        assert!(!backend.include_prereleases(&opts));

        // Inline backend args normalize scalars to strings — cover that shape.
        opts.opts
            .insert("prerelease".to_string(), toml::Value::String("true".into()));
        assert!(backend.include_prereleases(&opts));

        opts.opts.insert(
            "prerelease".to_string(),
            toml::Value::String("false".into()),
        );
        assert!(!backend.include_prereleases(&opts));

        // Defense-in-depth: also accept a native TOML boolean, in case a future
        // config path stores the value without string normalization.
        opts.opts
            .insert("prerelease".to_string(), toml::Value::Boolean(true));
        assert!(backend.include_prereleases(&opts));

        opts.opts
            .insert("prerelease".to_string(), toml::Value::Boolean(false));
        assert!(!backend.include_prereleases(&opts));
    }

    #[test]
    fn test_include_prereleases_global_setting_overrides_per_tool_default() {
        use crate::config::settings::SettingsPartial;
        use crate::toolset::ToolVersionOptions;
        use confique::Layer;

        let backend = TestBackend::default();
        let opts = ToolVersionOptions::default();
        // Sanity: with no per-tool opt and no setting, prereleases stay filtered.
        assert!(!backend.include_prereleases(&opts));

        // Flipping the global setting takes effect without any per-tool config —
        // this is the path `MISE_PRERELEASES=1` and `mise ls-remote --prerelease`
        // both ride on.
        let mut partial = SettingsPartial::empty();
        partial.prereleases = Some(true);
        Settings::reset(Some(partial));
        let res = backend.include_prereleases(&opts);
        Settings::reset(None);
        assert!(res);
    }

    #[derive(Debug)]
    struct TestBackend {
        ba: Arc<BackendArg>,
    }

    impl Default for TestBackend {
        fn default() -> Self {
            Self {
                ba: Arc::new("test".into()),
            }
        }
    }

    #[async_trait]
    impl Backend for TestBackend {
        fn ba(&self) -> &Arc<BackendArg> {
            &self.ba
        }

        async fn _list_remote_versions(
            &self,
            _config: &Arc<Config>,
        ) -> eyre::Result<Vec<VersionInfo>> {
            Ok(vec![])
        }

        async fn install_version_(
            &self,
            _ctx: &InstallContext,
            tv: ToolVersion,
        ) -> Result<ToolVersion> {
            Ok(tv)
        }
    }
}

#[async_trait]
pub trait Backend: Debug + Send + Sync {
    fn id(&self) -> &str {
        &self.ba().short
    }
    fn tool_name(&self) -> String {
        self.ba().tool_name()
    }
    fn get_type(&self) -> BackendType {
        BackendType::Core
    }
    fn ba(&self) -> &Arc<BackendArg>;

    /// Generates a platform key for lockfile storage.
    /// Default implementation uses the current platform key (os-arch or os-arch-qualifier),
    /// which includes the libc qualifier on musl systems.
    fn get_platform_key(&self) -> String {
        Platform::current().to_key()
    }

    /// Resolves the lockfile options for a tool request on a target platform.
    /// These options affect artifact identity and must match exactly for lockfile lookup.
    ///
    /// For the current platform: resolves from Settings and ToolRequest options
    /// For other platforms (cross-platform mise lock): uses sensible defaults
    ///
    /// Backends should override this to return options that affect which artifact is downloaded.
    fn resolve_lockfile_options(
        &self,
        _request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        BTreeMap::new() // Default: no options affect artifact identity
    }

    /// Returns all platform variants that should be locked for a given base platform.
    ///
    /// Some tools have compile-time variants (e.g., bun has baseline/musl variants)
    /// that result in different download URLs and checksums. This method allows
    /// backends to declare all variants so `mise lock` can fetch checksums for each.
    ///
    /// Default returns just the base platform. Backends should override this to
    /// return additional variants when applicable.
    ///
    /// Example: For bun on linux-x64, this might return:
    /// - linux-x64 (default, AVX2)
    /// - linux-x64-baseline (no AVX2)
    /// - linux-x64-musl (musl libc)
    /// - linux-x64-musl-baseline (musl + no AVX2)
    fn platform_variants(&self, platform: &Platform) -> Vec<Platform> {
        vec![platform.clone()] // Default: just the base platform
    }

    /// Whether this backend supports URL-based locking in locked mode.
    /// Backends that use external installers (like rustup for Rust) should override
    /// this to return false, since they don't have downloadable artifacts with lockable URLs.
    fn supports_lockfile_url(&self) -> bool {
        true
    }

    async fn description(&self) -> Option<String> {
        None
    }
    async fn security_info(&self) -> Vec<SecurityFeature> {
        vec![]
    }
    fn get_plugin_type(&self) -> Option<PluginType> {
        None
    }
    /// If any of these tools are installing in parallel, we should wait for them to finish
    /// before installing this tool.
    fn get_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec![])
    }

    /// Whether this backend's version source lacks an upstream prerelease flag
    /// and should mark regex-shaped versions as prereleases before caching.
    fn mark_prereleases_from_version_pattern(&self) -> bool {
        false
    }

    /// Whether pre-release versions should be included for this backend and
    /// current tool options. Backends can override this only for compatibility
    /// with deprecated backend-specific prerelease settings.
    fn include_prereleases(&self, opts: &crate::toolset::ToolVersionOptions) -> bool {
        if Settings::get().prereleases {
            return true;
        }

        if let Some(value) = opts.opts.get("prerelease") {
            return tool_option_bool(value);
        }

        false
    }

    /// Tool option keys whose non-registry overrides change the backend's
    /// remote version list. When any of these keys come from a backend alias,
    /// config, or inline backend arg, the versions host must be skipped because
    /// its cache is keyed by the registry/default listing.
    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &[]
    }

    /// dependencies which wait for install but do not warn, like cargo-binstall
    fn get_optional_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec![])
    }
    fn get_all_dependencies(&self, optional: bool) -> Result<IndexSet<BackendArg>> {
        let all_fulls = self.ba().all_fulls();
        if all_fulls.is_empty() {
            // this can happen on windows where we won't be able to install this os/arch so
            // the fact there might be dependencies is meaningless
            return Ok(Default::default());
        }
        let mut deps: Vec<&str> = self.get_dependencies()?;
        if optional {
            deps.extend(self.get_optional_dependencies()?);
        }
        let mut deps: IndexSet<_> = deps.into_iter().map(BackendArg::from).collect();
        deps.retain(|ba| &**self.ba() != ba);
        deps.retain(|ba| !all_fulls.contains(&ba.full()));
        for ba in deps.clone() {
            if let Ok(backend) = ba.backend() {
                deps.extend(backend.get_all_dependencies(optional)?);
            }
        }
        Ok(deps)
    }

    async fn list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        Ok(self
            .list_remote_versions_with_info(config)
            .await?
            .into_iter()
            .map(|v| v.version)
            .collect())
    }

    /// Like `list_remote_versions` but with explicit refresh control. Pass
    /// `refresh = true` to bypass the cached remote-versions list and re-fetch
    /// upstream. Used by install-time resolution for selectors whose answer
    /// depends on the freshest upstream entry (e.g. `latest`).
    async fn list_remote_versions_with_refresh(
        &self,
        config: &Arc<Config>,
        refresh: bool,
    ) -> eyre::Result<Vec<String>> {
        Ok(self
            .list_remote_versions_with_info_with_refresh(config, refresh)
            .await?
            .into_iter()
            .map(|v| v.version)
            .collect())
    }

    /// List remote versions with additional metadata like created_at timestamps.
    /// Results are cached. Backends can override `_list_remote_versions_with_info`
    /// to provide timestamp information.
    ///
    /// For backends that query canonical, always-fresh sources (npm, pipx, cargo,
    /// gem, go, and http/s3 with `version_list_url`), the versions host cache is
    /// skipped and the upstream is queried directly. For all other backends this
    /// method first tries the versions host (mise-versions.jdx.dev) which provides
    /// version info with created_at timestamps, and falls back to the backend's
    /// `_list_remote_versions_with_info` implementation on failure.
    ///
    /// In offline mode, this reads the existing remote-versions cache without
    /// fetching or writing. If no cache exists, it returns an empty list.
    async fn list_remote_versions_with_info(
        &self,
        config: &Arc<Config>,
    ) -> eyre::Result<Vec<VersionInfo>> {
        self.list_remote_versions_with_info_with_refresh(config, false)
            .await
    }

    async fn list_remote_versions_with_info_with_refresh(
        &self,
        config: &Arc<Config>,
        refresh: bool,
    ) -> eyre::Result<Vec<VersionInfo>> {
        let remote_versions = self.get_remote_version_cache();
        let mut remote_versions = remote_versions.lock().await;
        let ba = self.ba().clone();
        let id = self.id();
        let resolved_opts = config.resolve_tool_opts_with_overrides(&ba).await?;
        let opts = resolved_opts.options();

        // Only a subset of backends benefit from the versions host cache —
        // those whose upstream listing is rate-limited (github API) or not
        // otherwise available. Package-registry backends (npm, pipx, cargo,
        // gem, go, conda, dotnet, spm) and http/s3 with an explicit
        // version_list_url already have canonical, always-fresh sources, so
        // the cache would only add latency and staleness risk. Note: this
        // asymmetrically overrides `settings.use_versions_host = true` — the
        // setting can still disable the host globally, but cannot re-enable
        // it for backends that are not on this allowlist.
        let backend_type = self.get_type();
        let has_version_list_url = if matches!(backend_type, BackendType::Http | BackendType::S3) {
            opts.contains_key("version_list_url")
        } else {
            false
        };
        let versions_host_applies = match backend_type {
            BackendType::Github
            | BackendType::Gitlab
            | BackendType::Forgejo
            | BackendType::Ubi
            | BackendType::Aqua
            | BackendType::Core
            | BackendType::Asdf
            | BackendType::Vfox
            | BackendType::VfoxBackend(_) => true,
            BackendType::Http | BackendType::S3 => !has_version_list_url,
            _ => false,
        };

        let has_local_version_listing_override = has_local_version_listing_option_override(
            &resolved_opts,
            self.remote_version_listing_tool_option_keys(),
        );
        let use_versions_host = if !versions_host_applies {
            trace!(
                "Skipping versions host for {} because {} backend has a direct source",
                ba.short, backend_type
            );
            false
        } else if has_local_version_listing_override {
            trace!(
                "Skipping versions host for {} because local backend opts affect remote version listing",
                ba.short,
            );
            false
        } else if let Some(plugin) = self.plugin()
            && let Ok(Some(remote_url)) = plugin.get_remote_url()
        {
            // Check if remote matches the registry default
            let normalized_remote =
                normalize_remote(&remote_url).unwrap_or_else(|_| "INVALID_URL".into());
            let shorthand_remote = REGISTRY
                .get(plugin.name())
                .and_then(|rt| rt.backends().first().map(|b| full_to_url(b)))
                .unwrap_or_default();
            let matches =
                normalized_remote == normalize_remote(&shorthand_remote).unwrap_or_default();
            if !matches {
                trace!(
                    "Skipping versions host for {} because it has a non-default remote",
                    ba.short
                );
            }
            matches
        } else {
            // For non-plugin backends (e.g. github:, cargo:), check if the backend matches
            // the registry's default. When a user aliases a tool to a different backend
            // (e.g. `php = "github:verzly/php"`), the versions host would return versions
            // from the registry's default backend which may not match the aliased backend.
            let full = ba.full();
            if let Some(rt) = REGISTRY.get(ba.short.as_str()) {
                let is_registry_backend = rt.backends().iter().any(|b| *b == full);
                if !is_registry_backend {
                    trace!(
                        "Skipping versions host for {} because backend {} is not the registry default",
                        ba.short, full
                    );
                }
                is_registry_backend
            } else {
                true // Not in registry, safe to use versions host
            }
        };

        // Read-time filter: cache stores the pre-release superset for backends
        // that honor `prerelease`. When the current opts don't opt in, drop
        // entries with `prerelease = true` before returning so flipping the
        // tool option takes effect without invalidating the cache.
        let want_prereleases = self.include_prereleases(opts);

        if Settings::get().offline() {
            trace!(
                "Skipping remote version listing for {} due to offline mode",
                ba.to_string()
            );
            match remote_versions.get_cached() {
                Ok(versions) => return Ok(filter_cached_prereleases(versions, want_prereleases)),
                Err(err) => {
                    debug!(
                        "No cached remote versions available for {} while offline: {:#}",
                        ba.to_string(),
                        err
                    );
                }
            }
            return Ok(vec![]);
        }

        let fetch = || async {
            trace!("Listing remote versions for {}", ba.to_string());
            // Try versions host first (now returns VersionInfo with timestamps)
            if use_versions_host {
                match versions_host::list_versions(&ba.short).await {
                    Ok(Some(versions)) => {
                        trace!(
                            "Got {} versions from versions host for {}",
                            versions.len(),
                            ba.to_string()
                        );
                        return Ok(versions);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        debug!("Error getting versions from versions host: {:#}", e);
                    }
                }
            }
            trace!(
                "Calling backend to list remote versions for {}",
                ba.to_string()
            );
            let versions = self
                ._list_remote_versions(config)
                .await?
                .into_iter()
                .map(|v| match self.mark_prereleases_from_version_pattern() {
                    true => mark_prerelease(v),
                    false => v,
                })
                .filter(|v| match v.version.parse::<ToolVersionType>() {
                    Ok(ToolVersionType::Version(_)) => true,
                    _ => {
                        warn!("Invalid version: {id}@{}", v.version);
                        false
                    }
                })
                .collect_vec();
            if versions.is_empty()
                && self.get_type() != BackendType::Http
                && self.unresolved_latest_version().is_none()
            {
                warn!("No versions found for {id}");
            }
            Ok(versions)
        };
        let versions = if refresh {
            remote_versions.refresh_async(fetch).await?
        } else {
            remote_versions.get_or_try_init_async(fetch).await?.clone()
        };
        if versions.is_empty() {
            remote_versions.clear()?;
        }
        Ok(filter_cached_prereleases(versions, want_prereleases))
    }

    /// Backend implementation for fetching remote versions with metadata.
    /// Override this to provide version listing with optional timestamp information.
    /// Return `VersionInfo` with `created_at: None` if timestamps are not available.
    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>>;

    /// Backend-specific fast path for the absolute latest stable version.
    ///
    /// Do not call this from CLI/toolset code. Use `latest_version` instead so
    /// release-date cutoffs are handled around this fast path.
    ///
    /// Return `Ok(None)` when the backend does not have a fast path result.
    /// `latest_version` centrally falls back to the shared version-list path,
    /// which can use the remote versions cache in offline mode.
    async fn latest_stable_version(&self, _config: &Arc<Config>) -> eyre::Result<Option<String>> {
        Ok(None)
    }

    /// Backend opt-in for installing an unresolved `latest` request.
    ///
    /// Most backends must resolve `latest` to a concrete version before install.
    /// Override this only when the backend can pass an unresolved selector through
    /// to its installer, and only for requests where the selector is meaningful.
    ///
    /// `ToolVersion::resolve_version` uses this as a last resort after normal
    /// latest resolution fails, and only when the backend's unfiltered remote
    /// version list is empty. If remote versions exist but are all filtered out by
    /// a release-date cutoff, this hook is not used.
    fn unresolved_latest_version(&self) -> Option<String> {
        None
    }
    fn list_installed_versions(&self) -> Vec<String> {
        install_state::list_versions(&self.ba().short)
    }
    fn is_version_installed(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        check_symlink: bool,
    ) -> bool {
        let check_path = |install_path: &Path, check_symlink: bool| {
            let is_installed = install_path.exists();
            let is_not_incomplete = !self.incomplete_file_path(tv).exists();
            let is_valid_symlink = !check_symlink || !is_runtime_symlink(install_path);

            let installed = is_installed && is_not_incomplete && is_valid_symlink;
            if log::log_enabled!(log::Level::Trace) && !installed {
                let mut msg = format!(
                    "{} is not installed, path: {}",
                    self.ba(),
                    display_path(install_path)
                );
                if !is_installed {
                    msg += " (not installed)";
                }
                if !is_not_incomplete {
                    msg += " (incomplete)";
                }
                if !is_valid_symlink {
                    msg += " (runtime symlink)";
                }
                trace!("{}", msg);
            }
            installed
        };
        match tv.request {
            ToolRequest::System { .. } => true,
            _ => {
                if let Some(install_path) = tv.request.install_path(config)
                    && check_path(&install_path, true)
                {
                    // The request's install_path is derived from the REQUEST version
                    // (e.g., "latest" or prefix "1"), which may differ from the resolved
                    // concrete version. If they differ, the request path can refer to a
                    // stale install dir — for channel pins like `@latest` the dir is named
                    // after the channel (`installs/<id>/latest/`) from a prior install that
                    // never got a symlink (e.g., first install ran offline or remote resolution
                    // transiently returned None, leaving tv.version="latest"). We must check
                    // the RESOLVED path instead so that `mise upgrade` re-runs the backend's
                    // install hook and writes the new version to `installs/<id>/<new-version>/`.
                    if matches!(
                        &tv.request,
                        ToolRequest::Version { .. } | ToolRequest::Prefix { .. }
                    ) && install_path
                        .file_name()
                        .is_some_and(|f| f.to_string_lossy() != tv.version)
                    {
                        return check_path(&tv.install_path(), check_symlink);
                    }
                    return true;
                }
                check_path(&tv.install_path(), check_symlink)
            }
        }
    }
    async fn is_version_outdated(&self, config: &Arc<Config>, tv: &ToolVersion) -> bool {
        let latest = match tv.latest_version(config).await {
            Ok(latest) => latest,
            Err(e) => {
                warn!(
                    "Error getting latest version for {}: {:#}",
                    self.ba().to_string(),
                    e
                );
                return false;
            }
        };
        !self.is_version_installed(config, tv, true) || is_outdated_version(&tv.version, &latest)
    }
    fn symlink_path(&self, tv: &ToolVersion) -> Option<PathBuf> {
        let path = tv.install_path();
        if !path.is_symlink() {
            return None;
        }
        // Only skip symlinks pointing within installs (user aliases, not backend-managed)
        if let Ok(Some(target)) = file::resolve_symlink(&path) {
            let target = if target.is_absolute() {
                target
            } else {
                path.parent().unwrap_or(&path).join(&target)
            };
            // Canonicalize to resolve any ".." components before checking.
            // If target doesn't exist (canonicalize fails), don't skip - treat as needing install
            let Ok(target) = target.canonicalize() else {
                return None;
            };
            // Canonicalize INSTALLS too for consistent comparison (handles symlinked data dirs)
            let installs = dirs::INSTALLS
                .canonicalize()
                .unwrap_or(dirs::INSTALLS.to_path_buf());
            if target.starts_with(&installs) {
                return Some(path);
            }
            // Also check shared install directories
            for shared_dir in env::shared_install_dirs() {
                let shared = shared_dir
                    .canonicalize()
                    .unwrap_or(shared_dir.to_path_buf());
                if target.starts_with(&shared) {
                    return Some(path);
                }
            }
        }
        None
    }
    fn create_symlink(&self, version: &str, target: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
        let link = self.ba().installs_path.join(version);
        if link.exists() {
            return Ok(None);
        }
        file::create_dir_all(link.parent().unwrap())?;
        let link = file::make_symlink(target, &link)?;
        Ok(Some(link))
    }
    fn list_installed_versions_matching(&self, query: &str) -> Vec<String> {
        let versions = self.list_installed_versions();
        // No async config lookup available here; fall back to inline/registry
        // opts, which is the best we have for a sync path.
        let filter = !self.include_prereleases(&self.ba().opts());
        self.fuzzy_match_filter(versions, query, filter)
    }
    async fn list_versions_matching(
        &self,
        config: &Arc<Config>,
        query: &str,
    ) -> eyre::Result<Vec<String>> {
        let versions = self.list_remote_versions(config).await?;
        let opts = config.get_tool_opts_with_overrides(self.ba()).await?;
        let filter = !self.include_prereleases(&opts);
        Ok(self.fuzzy_match_filter(versions, query, filter))
    }

    /// List versions matching a query, optionally filtered by release date.
    /// Use this when you have a `before_date` from ResolveOptions.
    async fn list_versions_matching_with_opts(
        &self,
        config: &Arc<Config>,
        query: &str,
        before_date: Option<Timestamp>,
        refresh: bool,
    ) -> eyre::Result<Vec<String>> {
        let versions = match before_date {
            Some(before) => {
                // Use version info to filter by date
                let versions_with_info = self
                    .list_remote_versions_with_info_with_refresh(config, refresh)
                    .await?;
                let filtered = VersionInfo::filter_by_date(versions_with_info, before);
                // Warn if no versions have timestamps
                if filtered.iter().all(|v| v.created_at.is_none()) && !filtered.is_empty() {
                    debug!(
                        "Backend {} does not provide release dates; release-date filter may not work as expected",
                        self.id()
                    );
                }
                filtered.into_iter().map(|v| v.version).collect()
            }
            None => {
                self.list_remote_versions_with_refresh(config, refresh)
                    .await?
            }
        };
        let opts = config.get_tool_opts_with_overrides(self.ba()).await?;
        let filter = !self.include_prereleases(&opts);
        Ok(self.fuzzy_match_filter(versions, query, filter))
    }

    async fn latest_version_for_query(
        &self,
        config: &Arc<Config>,
        query: &str,
        before_date: Option<Timestamp>,
        refresh: bool,
    ) -> eyre::Result<Option<String>> {
        let mut matches = self
            .list_versions_matching_with_opts(config, query, before_date, refresh)
            .await?;
        if matches.is_empty() && query == "latest" {
            // Fall back to all versions if no match
            matches = match before_date {
                Some(before) => {
                    let versions_with_info = self
                        .list_remote_versions_with_info_with_refresh(config, refresh)
                        .await?;
                    VersionInfo::filter_by_date(versions_with_info, before)
                        .into_iter()
                        .map(|v| v.version)
                        .collect()
                }
                None => {
                    self.list_remote_versions_with_refresh(config, refresh)
                        .await?
                }
            };
        }
        Ok(find_match_in_list(&matches, query))
    }

    /// Get the latest version, optionally filtered by release date.
    ///
    /// `latest_stable_version` may use backend-specific fast paths (dist tags,
    /// latest release endpoints, plugin scripts). If the fast path returns
    /// `None`, fall back to the shared version-list path here instead of
    /// duplicating that fallback in each backend. When a cutoff is active,
    /// accept the fast path result only when remote-version metadata verifies
    /// that the candidate is older than the cutoff.
    async fn latest_version(
        &self,
        config: &Arc<Config>,
        query: Option<String>,
        before_date: Option<Timestamp>,
    ) -> eyre::Result<Option<String>> {
        self.latest_version_with_refresh(config, query, before_date, false)
            .await
    }

    /// Like `latest_version` but with explicit refresh control. Pass
    /// `refresh = true` to bypass the cached remote-versions list when falling
    /// back to the full version-list path. The `latest_stable_version` fast
    /// path is still tried first — it queries canonical upstream endpoints
    /// (e.g. GitHub's `/releases/latest`, npm dist tags) which are
    /// authoritative and not subject to the local version-list cache, so
    /// skipping it would actually return *older* results than refreshing the
    /// list (which itself may go through a versions-host cache).
    async fn latest_version_with_refresh(
        &self,
        config: &Arc<Config>,
        query: Option<String>,
        before_date: Option<Timestamp>,
        refresh: bool,
    ) -> eyre::Result<Option<String>> {
        let before_date = effective_latest_before_date(self, config, before_date).await?;
        let resolved_query = query.as_deref().unwrap_or("latest");
        let mut fallback_refresh = refresh;
        if resolved_query == "latest"
            && let Some(version) = self.latest_stable_version(config).await?
        {
            match before_date {
                Some(before) => {
                    let versions = self
                        .list_remote_versions_with_info_with_refresh(config, refresh)
                        .await?;
                    fallback_refresh = false;
                    let info = versions.iter().find(|v| v.version == version);
                    if latest_stable_candidate_allowed_by_before_date(&version, info, before) {
                        return Ok(Some(version));
                    }
                }
                None => return Ok(Some(version)),
            }
        }
        self.latest_version_for_query(config, resolved_query, before_date, fallback_refresh)
            .await
    }
    fn latest_installed_version(&self, query: Option<String>) -> eyre::Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_installed_versions_matching(&query);
                Ok(find_match_in_list(&matches, &query))
            }
            None => {
                let installed_symlink = self.ba().installs_path.join("latest");
                if installed_symlink.exists()
                    && let Some(target) = file::resolve_symlink(&installed_symlink)?
                {
                    let version = target
                        .file_name()
                        .ok_or_else(|| eyre!("Invalid symlink target"))?
                        .to_string_lossy()
                        .to_string();
                    return Ok(Some(version));
                }
                Ok(file::dir_subdirs(&self.ba().installs_path)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|v| !v.starts_with('.'))
                    .filter(|v| !is_runtime_symlink(&self.ba().installs_path.join(v)))
                    .filter(|v| !self.ba().installs_path.join(v).join("incomplete").exists())
                    .filter(|v| v != "latest")
                    .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
                    .last())
            }
        }
    }

    /// Get version info for a specific version (including checksum for rolling releases)
    async fn get_version_info(&self, config: &Arc<Config>, version: &str) -> Option<VersionInfo> {
        let versions = match self.list_remote_versions_with_info(config).await {
            Ok(v) => v,
            Err(_) => return None,
        };
        versions.into_iter().find(|v| v.version == version)
    }

    /// Check if a rolling version has changed (by comparing checksums)
    /// Returns true if the version should be updated
    async fn is_rolling_version_outdated(&self, config: &Arc<Config>, version: &str) -> bool {
        use crate::toolset::install_state;

        // Get the latest version info
        let version_info = match self.get_version_info(config, version).await {
            Some(v) if v.rolling => v,
            _ => return false, // Not rolling or not found
        };

        // If no checksum available, we can't detect changes - don't assume outdated
        let Some(latest_checksum) = version_info.checksum else {
            trace!(
                "No checksum available for rolling version {}, cannot detect updates",
                version
            );
            return false;
        };

        // Compare with stored checksum
        let stored_checksum = install_state::read_checksum(&self.ba().short, version);
        match stored_checksum {
            Some(stored) if stored == latest_checksum => {
                trace!("Rolling version {} checksum unchanged", version);
                false
            }
            Some(stored) => {
                trace!(
                    "Rolling version {} checksum changed: {} -> {}",
                    version, stored, latest_checksum
                );
                true
            }
            None => {
                trace!(
                    "No stored checksum for rolling version {}, assuming outdated",
                    version
                );
                true
            }
        }
    }

    async fn warn_if_dependencies_missing(&self, config: &Arc<Config>) -> eyre::Result<()> {
        let deps = self
            .get_all_dependencies(false)?
            .into_iter()
            .filter(|ba| &**self.ba() != ba)
            .map(|ba| ba.short)
            .collect::<HashSet<_>>();
        if !deps.is_empty() {
            trace!("Ensuring dependencies installed for {}", self.id());
            let ts = config.get_tool_request_set().await?.filter_by_tool(deps);
            let missing = ts.missing_tools(config).await;
            if !missing.is_empty() {
                warn_once!(
                    "missing dependency: {}",
                    missing.iter().map(|d| d.to_string()).join(", "),
                );
            }
        }
        Ok(())
    }
    fn purge(&self, pr: &dyn SingleReport) -> eyre::Result<()> {
        remove_all_with_progress(&self.ba().installs_path, pr)?;
        remove_all_with_progress(&self.ba().cache_path, pr)?;
        remove_all_with_progress(&self.ba().downloads_path, pr)?;
        Ok(())
    }
    fn get_aliases(&self) -> eyre::Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    /// Returns a list of idiomatic filenames for this tool.
    ///
    /// This method is additive:
    /// 1. It calls `_idiomatic_filenames` to get backend-specific filenames.
    /// 2. It checks the Registry for any additional filenames defined there.
    async fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        let mut filenames = self._idiomatic_filenames().await?;
        if let Some(rt) = REGISTRY.get(self.id()) {
            filenames.extend(rt.idiomatic_files.iter().map(|s| s.to_string()));
        }
        filenames = filenames.into_iter().unique().collect();
        Ok(filenames)
    }

    /// Backend-specific implementation for `idiomatic_filenames`.
    /// Override this to provide native idiomatic filenames for the backend.
    async fn _idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    /// Parses an idiomatic version file to extract the version.
    ///
    /// This handles special files like `package.json` which are parsed natively to avoid
    /// every backend needing to implement `package.json` support. For other files, it
    /// delegates to `_parse_idiomatic_file`.
    async fn parse_idiomatic_file(&self, path: &Path) -> eyre::Result<Vec<String>> {
        if crate::config::config_file::idiomatic_version::package_json::is_package_json(path) {
            return crate::config::config_file::idiomatic_version::package_json::parse(
                path,
                self.id(),
            );
        }
        self._parse_idiomatic_file(path).await
    }

    /// Backend-specific implementation for `parse_idiomatic_file`.
    /// Default implementation reads the file and treats each whitespace-separated token as a version.
    /// Override to provide format-specific parsing; return `Err` on real failures so the plugin is skipped.
    async fn _parse_idiomatic_file(&self, path: &Path) -> eyre::Result<Vec<String>> {
        let contents = file::read_to_string(path)?;
        let normalized = normalize_idiomatic_contents(&contents);
        if normalized.is_empty() {
            return Ok(vec![]);
        }
        Ok(normalized
            .split_whitespace()
            .map(|s| s.to_string())
            .collect())
    }

    fn plugin(&self) -> Option<&PluginEnum> {
        None
    }

    async fn install_version(
        &self,
        ctx: InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        // Check for --locked mode: if enabled and no lockfile URL exists, fail early
        // Exempt tool stubs from lockfile requirements since they are ephemeral
        // Also exempt backends that don't support URL locking (e.g., Rust uses rustup)
        // This must run before the dry-run check so that --locked --dry-run still validates
        let settings = Settings::get();
        if (ctx.locked || settings.locked) && settings.lockfile == Some(false) {
            bail!(
                "locked mode requires lockfile to be enabled\n\
                hint: Remove `lockfile = false` or set `lockfile = true`, or disable locked mode"
            );
        }
        if ctx.locked && !tv.request.source().is_tool_stub() && self.supports_lockfile_url() {
            let platform_key = self.get_platform_key();
            let has_lockfile_url = tv
                .lock_platforms
                .get(&platform_key)
                .and_then(|p| p.url.as_ref())
                .is_some();
            if !has_lockfile_url {
                bail!(
                    "No lockfile URL found for {} on platform {} (--locked mode)\n\
                    hint: Run `mise lock` to generate lockfile URLs, or disable locked mode",
                    tv.style(),
                    platform_key
                );
            }
        }

        // Handle dry-run mode early to avoid plugin installation
        if ctx.dry_run {
            use crate::ui::progress_report::ProgressIcon;
            if self.is_version_installed(&ctx.config, &tv, true) {
                ctx.pr
                    .finish_with_icon("already installed".into(), ProgressIcon::Skipped);
            } else {
                ctx.pr
                    .finish_with_icon("would install".into(), ProgressIcon::Skipped);
            }
            return Ok(tv);
        }

        if let Some(plugin) = self.plugin() {
            plugin.is_installed_err()?;
        }

        // If --force and the install path resolved to a shared dir (but wasn't explicitly
        // set via --system/--shared), redirect to primary dir to avoid modifying shared installs.
        if ctx.force
            && tv.install_path.is_none()
            && env::install_path_category(&tv.install_path()) != env::InstallPathCategory::Local
        {
            tv.install_path = Some(tv.ba().installs_path.join(tv.tv_pathname()));
        }

        let will_uninstall = ctx.force && self.is_version_installed(&ctx.config, &tv, true);

        // Query backend for operation count and set up progress tracking
        let install_ops = self.install_operation_count(&tv, &ctx).await;
        let total_ops = if will_uninstall {
            install_ops + 1
        } else {
            install_ops
        };
        ctx.pr.start_operations(total_ops);

        if will_uninstall {
            self.uninstall_version(&ctx.config, &tv, ctx.pr.as_ref(), false)
                .await?;
            ctx.pr.next_operation();
        } else if self.is_version_installed(&ctx.config, &tv, true) {
            return Ok(tv);
        }

        // Track the installation asynchronously (fire-and-forget)
        // Do this before install so the request has time to complete during installation
        versions_host::track_install(tv.short(), &tv.ba().full(), &tv.version);

        ctx.pr.set_message("install".into());
        let _lock = lock_file::get(&tv.install_path(), ctx.force)?;

        // Double-checked (locking) that it wasn't installed while we were waiting for the lock
        if self.is_version_installed(&ctx.config, &tv, true) && !ctx.force {
            return Ok(tv);
        }

        self.create_install_dirs(&tv)?;

        let old_tv = tv.clone();
        let tv = match self.install_version_(&ctx, tv).await {
            Ok(tv) => tv,
            Err(e) => {
                self.cleanup_install_dirs_on_error(&old_tv);
                // Pass through the error - it will be wrapped at a higher level
                return Err(e);
            }
        };

        let install_path = tv.install_path();
        if install_path.starts_with(*dirs::INSTALLS) {
            install_state::write_backend_meta(self.ba())?;
        } else if env::install_path_category(&install_path) != env::InstallPathCategory::Local {
            // For --system/--shared installs, write manifest to the target installs dir
            if let Some(installs_dir) = install_path.parent().and_then(|p| p.parent()) {
                let manifest = installs_dir.join(".mise-installs.toml");
                install_state::write_backend_meta_to(self.ba(), &manifest)?;
            }
        }

        self.cleanup_install_dirs(&tv);
        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::DATA.to_path_buf()];
        touch_dirs.extend(ctx.config.config_files.keys().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                trace!("error touching config file: {:?} {:?}", path, err);
            }
        }
        let incomplete_path = self.incomplete_file_path(&tv);
        if let Err(err) = file::remove_file(&incomplete_path) {
            debug!("error removing incomplete file: {:?}", err);
        } else {
            // Sync parent directory to ensure file removal is immediately visible
            if let Some(parent) = incomplete_path.parent()
                && let Err(err) = file::sync_dir(parent)
            {
                debug!("error syncing incomplete file parent directory: {:?}", err);
            }
        }
        if let Some(script) = tv.request.options().get("postinstall") {
            ctx.pr
                .finish_with_message("running custom postinstall hook".to_string());
            self.run_postinstall_hook(&ctx, &tv, script).await?;
        }
        ctx.pr.finish_with_message("installed".to_string());
        Ok(tv)
    }

    async fn run_postinstall_hook(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        script: &str,
    ) -> eyre::Result<()> {
        // Get pre-tools environment variables from config
        let mut env_vars = self.exec_env(&ctx.config, &ctx.ts, tv).await?;

        // Add pre-tools environment variables from config if available
        if let Some(config_env) = ctx.config.env_maybe() {
            for (k, v) in config_env {
                env_vars.entry(k).or_insert(v);
            }
        }

        // Use the backend's list_bin_paths to get the correct binary directories
        // instead of hardcoding install_path/bin, which may not match the actual
        // binary location for backends like aqua
        let bin_paths = self.list_bin_paths(&ctx.config, tv).await?;
        let mut path_env = PathEnv::from_iter(env::PATH.clone());
        for p in bin_paths {
            path_env.add(p);
        }

        // Render tera template variables (e.g. {{tools.ripgrep.path}})
        let tera_ctx = ctx.ts.tera_ctx(&ctx.config).await?;
        let dir = tv.request.source().path().and_then(|p| p.parent());
        let mut tera = get_tera(dir);
        let rendered_script = tera.render_str(script, tera_ctx)?;

        let mut runner = CmdLineRunner::new(&*env::SHELL)
            .env(&*env::PATH_KEY, path_env.join())
            .env("MISE_TOOL_INSTALL_PATH", tv.install_path())
            .env("MISE_TOOL_NAME", tv.ba().short.clone())
            .env("MISE_TOOL_VERSION", tv.version.clone())
            .with_pr(ctx.pr.as_ref())
            .arg(env::SHELL_COMMAND_FLAG)
            .arg(&rendered_script)
            .envs(env_vars);

        // Set MISE_CONFIG_ROOT and MISE_PROJECT_ROOT from the tool's source config file
        if let Some(source_path) = tv.request.source().path() {
            let root = config_root::config_root(source_path);
            let root = root.to_string_lossy().to_string();
            runner = runner
                .env("MISE_CONFIG_ROOT", &root)
                .env("MISE_PROJECT_ROOT", &root);
        }

        runner.execute()?;
        Ok(())
    }

    /// Returns the number of operations for installation progress tracking.
    /// Override this if your backend has a different number of operations.
    /// Default is 3: download, checksum, extract
    async fn install_operation_count(&self, _tv: &ToolVersion, _ctx: &InstallContext) -> usize {
        3
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion>;
    async fn uninstall_version(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        dryrun: bool,
    ) -> eyre::Result<()> {
        pr.set_message("uninstall".into());

        if !dryrun {
            self.uninstall_version_impl(config, pr, tv).await?;
        }
        let rmdir = |dir: &Path| {
            if dryrun {
                if dir.exists() {
                    pr.set_message(format!("remove {}", display_path(dir)));
                }
                return Ok(());
            }
            remove_all_with_progress(dir, pr)
        };
        rmdir(&tv.install_path())?;
        if !Settings::get().always_keep_download {
            rmdir(&tv.download_path())?;
        }
        rmdir(&tv.cache_path())?;
        Ok(())
    }
    async fn uninstall_version_impl(
        &self,
        _config: &Arc<Config>,
        _pr: &dyn SingleReport,
        _tv: &ToolVersion,
    ) -> Result<()> {
        Ok(())
    }
    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        match tv.request {
            ToolRequest::System { .. } => Ok(vec![]),
            _ => Ok(vec![tv.runtime_path().join("bin")]),
        }
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        _tv: &ToolVersion,
    ) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    async fn which(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        bin_name: &str,
    ) -> eyre::Result<Option<PathBuf>> {
        let bin_paths = self
            .list_bin_paths(config, tv)
            .await?
            .into_iter()
            .filter(|p| p.parent().is_some());
        for bin_path in bin_paths {
            let paths_with_ext = if cfg!(windows) {
                vec![
                    bin_path.clone(),
                    bin_path.join(bin_name).with_extension("exe"),
                    bin_path.join(bin_name).with_extension("cmd"),
                    bin_path.join(bin_name).with_extension("bat"),
                    bin_path.join(bin_name).with_extension("ps1"),
                ]
            } else {
                vec![bin_path.join(bin_name)]
            };
            for bin_path in paths_with_ext {
                if bin_path.exists() && file::is_executable(&bin_path) {
                    return Ok(Some(bin_path));
                }
            }
        }
        Ok(None)
    }

    fn create_install_dirs(&self, tv: &ToolVersion) -> eyre::Result<()> {
        let _ = remove_all_with_warning(tv.install_path());
        if !Settings::get().always_keep_download {
            let _ = remove_all_with_warning(tv.download_path());
        }
        let _ = remove_all_with_warning(tv.cache_path());
        let _ = file::remove_file(tv.install_path()); // removes if it is a symlink
        file::create_dir_all(tv.install_path())?;
        file::create_dir_all(tv.download_path())?;
        file::create_dir_all(tv.cache_path())?;
        File::create(self.incomplete_file_path(tv))?;
        Ok(())
    }
    fn cleanup_install_dirs_on_error(&self, tv: &ToolVersion) {
        if !Settings::get().always_keep_install {
            let _ = remove_all_with_warning(tv.install_path());
            // Clean up the incomplete marker from cache
            let _ = file::remove_file(self.incomplete_file_path(tv));
            // Remove parent installs dir if it's now empty (no other versions present)
            let installs_path = &self.ba().installs_path;
            if installs_path.exists()
                && let Ok(entries) = file::dir_subdirs(installs_path)
                && entries.is_empty()
            {
                let _ = remove_all_with_warning(installs_path);
            }
            self.cleanup_install_dirs(tv);
        }
    }
    fn cleanup_install_dirs(&self, tv: &ToolVersion) {
        if !Settings::get().always_keep_download {
            let _ = remove_all_with_warning(tv.download_path());
        }
    }
    fn incomplete_file_path(&self, tv: &ToolVersion) -> PathBuf {
        install_state::incomplete_file_path(&tv.ba().short, &tv.tv_pathname())
    }

    async fn path_env_for_cmd(&self, config: &Arc<Config>, tv: &ToolVersion) -> Result<OsString> {
        let path = self
            .list_bin_paths(config, tv)
            .await?
            .into_iter()
            .chain(
                self.dependency_toolset(config)
                    .await?
                    .list_paths(config)
                    .await,
            )
            .chain(env::PATH.clone());
        Ok(env::join_paths(path)?)
    }

    async fn dependency_toolset(&self, config: &Arc<Config>) -> eyre::Result<Toolset> {
        let dependencies = self
            .get_all_dependencies(true)?
            .into_iter()
            .map(|ba| ba.short)
            .collect();
        let mut ts: Toolset = config
            .get_tool_request_set()
            .await?
            .filter_by_tool(dependencies)
            .into();
        ts.resolve(config).await?;
        Ok(ts)
    }

    async fn dependency_which(&self, config: &Arc<Config>, bin: &str) -> Option<PathBuf> {
        if let Some(bin) = file::which_non_pristine(bin) {
            return Some(bin);
        }
        let Ok(ts) = self.dependency_toolset(config).await else {
            return None;
        };
        let (b, tv) = ts.which(config, bin).await?;
        b.which(config, &tv, bin).await.ok().flatten()
    }

    /// Check if a required dependency is available and show a warning if not.
    /// `provided_by` lists tool names that are known to provide the `program` binary
    /// (e.g., "npm" is provided by &["node"]). If any of these tools are configured
    /// in the toolset (even if not yet installed), the warning is suppressed since
    /// mise will install them as dependencies first.
    async fn warn_if_dependency_missing(
        &self,
        config: &Arc<Config>,
        program: &str,
        provided_by: &[&str],
        install_instructions: &str,
    ) {
        let found = if self.dependency_which(config, program).await.is_some() {
            true
        } else if cfg!(windows) {
            // On Windows, also check for program with Windows executable extensions
            let settings = Settings::get();
            let mut found = false;
            for ext in &settings.windows_executable_extensions {
                if self
                    .dependency_which(config, &format!("{}.{}", program, ext))
                    .await
                    .is_some()
                {
                    found = true;
                    break;
                }
            }
            found
        } else {
            false
        };

        if !found {
            // Check if a tool that provides this program is configured in the toolset
            // (even if not yet installed). If so, mise will install it as a dependency
            // before this tool needs it, so the warning is spurious.
            if let Ok(ts) = self.dependency_toolset(config).await
                && ts
                    .versions
                    .keys()
                    .any(|ba| provided_by.contains(&ba.short.as_str()))
            {
                return;
            }
            warn!(
                "{} may be required but was not found.\n\n{}",
                program, install_instructions
            );
        }
    }

    async fn dependency_env(&self, config: &Arc<Config>) -> eyre::Result<BTreeMap<String, String>> {
        // Use full_env_without_tools to avoid triggering `tools = true` env module
        // hooks (e.g., MiseEnv Lua hooks). Those modules may depend on tools that
        // are not in the dependency toolset, causing "command not found" errors.
        // The dependency env only needs tool bin paths on PATH, not module outputs.
        let mut env = self
            .dependency_toolset(config)
            .await?
            .full_env_without_tools(config)
            .await?;

        // Remove mise shims from PATH to prevent infinite shim recursion when a
        // dependency tool (e.g., go) is configured but not installed. Without this,
        // the shim for the dependency would call `mise exec` which would call the
        // shim again infinitely.
        //
        // `paths_eq` handles case-insensitive matching on macOS/Windows: e.g. if
        // `$HOME` is mixed-case in PATH (`/Users/Foo`) but lowercase in the
        // resolved shims path, byte-equal comparison would miss it and the shim
        // would survive in the child env.
        if let Some(path_val) = env.get(&*env::PATH_KEY) {
            let paths: Vec<_> = env::split_paths(path_val).collect();
            let original_len = paths.len();
            let filtered: Vec<_> = paths
                .into_iter()
                .filter(|p| !file::paths_eq(&file::replace_path(p), &dirs::SHIMS))
                .collect();
            if filtered.len() != original_len {
                let joined = env::join_paths(&filtered)?;
                env.insert(
                    env::PATH_KEY.to_string(),
                    joined.to_string_lossy().into_owned(),
                );
            }
        }

        Ok(env)
    }

    /// Default fuzzy-match. `filter_prereleases = true` applies the historical
    /// behavior of dropping versions that look like pre-releases
    /// (`1.0.0-rc1`, `1.0.0-dev.5`, ...). Callers that have opted into
    /// pre-releases pass `false` to keep those tags in the match set.
    fn fuzzy_match_filter(
        &self,
        versions: Vec<String>,
        query: &str,
        filter_prereleases: bool,
    ) -> Vec<String> {
        fuzzy_match_versions(versions, query, filter_prereleases)
    }

    fn get_remote_version_cache(&self) -> Arc<TokioMutex<VersionCacheManager>> {
        // use a mutex to prevent deadlocks that occurs due to reentrant cache access
        static REMOTE_VERSION_CACHE: Lazy<
            Mutex<HashMap<String, Arc<TokioMutex<VersionCacheManager>>>>,
        > = Lazy::new(Default::default);

        REMOTE_VERSION_CACHE
            .lock()
            .unwrap()
            .entry(self.ba().full())
            .or_insert_with(|| {
                let mut cm = CacheManagerBuilder::new(
                    self.ba().cache_path.join("remote_versions.msgpack.z"),
                )
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache());
                if let Some(plugin_path) = self.plugin().map(|p| p.path()) {
                    cm = cm
                        .with_fresh_file(plugin_path.clone())
                        .with_fresh_file(plugin_path.join("bin/list-all"))
                }

                TokioMutex::new(cm.build()).into()
            })
            .clone()
    }

    fn verify_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file: &Path,
    ) -> Result<()> {
        let settings = Settings::get();
        let filename = file.file_name().unwrap().to_string_lossy().to_string();
        let lockfile_enabled = settings.lockfile_enabled();

        // Get the platform key for this tool and platform
        let platform_key = self.get_platform_key();

        // Get or create asset info for this platform
        let platform_info = tv.lock_platforms.entry(platform_key.clone()).or_default();

        if let Some(checksum) = &platform_info.checksum {
            ctx.pr.set_message(format!("checksum {filename}"));
            if let Some((algo, check)) = checksum.split_once(':') {
                hash::ensure_checksum(file, check, Some(ctx.pr.as_ref()), algo)?;
            } else {
                bail!("Invalid checksum: {checksum}");
            }
        } else if lockfile_enabled {
            ctx.pr.set_message(format!("generate checksum {filename}"));
            let hash = hash::file_hash_blake3(file, Some(ctx.pr.as_ref()))?;
            platform_info.checksum = Some(format!("blake3:{hash}"));
        }

        // Handle size verification and generation
        if let Some(expected_size) = platform_info.size {
            ctx.pr.set_message(format!("verify size {filename}"));
            let actual_size = file.metadata()?.len();
            if actual_size != expected_size {
                bail!(
                    "Size mismatch for {}: expected {}, got {}",
                    filename,
                    expected_size,
                    actual_size
                );
            }
        } else if lockfile_enabled {
            platform_info.size = Some(file.metadata()?.len());
        }
        Ok(())
    }

    async fn outdated_info(
        &self,
        _config: &Arc<Config>,
        _tv: &ToolVersion,
        _bump: bool,
        _opts: &ResolveOptions,
    ) -> Result<Option<OutdatedInfo>> {
        Ok(None)
    }

    // ========== Lockfile Metadata Fetching Methods ==========

    /// Optional: Provide tarball URL for platform-specific tool installation
    /// Backends can implement this for simple tarball-based tools
    async fn get_tarball_url(
        &self,
        _tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<Option<String>> {
        Ok(None) // Default: no tarball URL available
    }

    /// Optional: Provide GitHub/GitLab release info for platform-specific tool installation
    /// Backends can implement this for GitHub/GitLab release-based tools
    async fn get_github_release_info(
        &self,
        _tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<Option<GitHubReleaseInfo>> {
        Ok(None) // Default: no GitHub release info available
    }

    /// Resolve platform-specific lock information without installation
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // Try simple tarball approach first
        if let Some(tarball_url) = self.get_tarball_url(tv, target).await? {
            return self
                .resolve_lock_info_from_tarball(&tarball_url, tv, target)
                .await;
        }

        // Try GitHub/GitLab release approach second
        if let Some(release_info) = self.get_github_release_info(tv, target).await? {
            return self
                .resolve_lock_info_from_github_release(&release_info, tv, target)
                .await;
        }

        // Fall back to basic platform info without URLs/metadata
        self.resolve_lock_info_fallback(tv, target).await
    }

    /// Shared logic for processing tarball-based tools
    /// Downloads tarball headers, extracts size and URL info, and populates PlatformInfo
    async fn resolve_lock_info_from_tarball(
        &self,
        tarball_url: &str,
        _tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // For now, just return basic info with the URL
        // In a full implementation, this would:
        // 1. Make HEAD request to get content-length
        // 2. Potentially download to get checksum
        // 3. Handle any URL-specific logic
        Ok(PlatformInfo {
            checksum: None, // TODO: Implement checksum fetching
            size: None,     // TODO: Implement size fetching via HEAD request
            url: Some(tarball_url.to_string()),
            url_api: None,
            conda_deps: None,
            ..Default::default()
        })
    }

    /// Shared logic for processing GitHub/GitLab release-based tools
    /// Queries release API, finds platform-specific assets, and populates PlatformInfo
    async fn resolve_lock_info_from_github_release(
        &self,
        release_info: &GitHubReleaseInfo,
        _tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // For now, just return basic info
        // In a full implementation, this would:
        // 1. Query GitHub/GitLab release API
        // 2. Find matching asset for the target platform
        // 3. Extract download URL, size, and checksums
        let asset_name = release_info.asset_pattern.as_ref().map(|pattern| {
            pattern
                .replace("{os}", target.os_name())
                .replace("{arch}", target.arch_name())
        });

        // Combine api_url (base URL) with asset_name to get full download URL
        let asset_url = match (&release_info.api_url, &asset_name) {
            (Some(base_url), Some(name)) => Some(format!("{}/{}", base_url, name)),
            _ => asset_name.clone(),
        };

        Ok(PlatformInfo {
            checksum: None, // TODO: Implement checksum fetching from releases
            size: None,     // TODO: Implement size fetching from GitHub API
            url: asset_url,
            url_api: None,
            conda_deps: None,
            ..Default::default()
        })
    }

    /// Fallback method when no specific metadata resolution is available
    /// Returns minimal PlatformInfo without external URLs
    async fn resolve_lock_info_fallback(
        &self,
        _tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        // This is the fallback - no external metadata available
        // The tool would need to be installed to generate platform info
        Ok(PlatformInfo {
            checksum: None,
            size: None,
            url: None,
            url_api: None,
            conda_deps: None,
            ..Default::default()
        })
    }
}

async fn effective_latest_before_date<B: Backend + ?Sized>(
    backend: &B,
    config: &Arc<Config>,
    before_date: Option<Timestamp>,
) -> eyre::Result<Option<Timestamp>> {
    if before_date.is_some() {
        return Ok(before_date);
    }

    let opts = config.get_tool_opts_with_overrides(backend.ba()).await?;
    resolve_before_date(None, opts.minimum_release_age())
}

fn latest_stable_candidate_allowed_by_before_date(
    version: &str,
    info: Option<&VersionInfo>,
    before: Timestamp,
) -> bool {
    let Some(info) = info else {
        debug!(
            "Latest stable version {version} is missing from remote version metadata; falling back to full version list"
        );
        return false;
    };
    let Some(created_at) = info.created_at.as_deref() else {
        debug!(
            "Latest stable version {version} has no release date metadata; falling back to full version list"
        );
        return false;
    };
    match parse_into_timestamp(created_at) {
        Ok(created) => created < before,
        Err(err) => {
            debug!(
                "Failed to parse release date for latest stable version {version}: {created_at}: {err:#}"
            );
            false
        }
    }
}

#[cfg(test)]
mod latest_version_tests {
    use super::*;
    use crate::cli::args::BackendResolution;
    use crate::config::settings::SettingsPartial;
    use confique::Layer;
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct LatestBackend {
        ba: Arc<BackendArg>,
        stable_result: Option<String>,
        remote_versions: Vec<VersionInfo>,
        stable_calls: AtomicUsize,
        list_calls: AtomicUsize,
    }

    impl LatestBackend {
        fn new(name: &str) -> Self {
            Self {
                ba: Arc::new(name.into()),
                stable_result: Some("9.9.9".to_string()),
                remote_versions: vec![
                    VersionInfo {
                        version: "1.0.0".to_string(),
                        created_at: Some("2024-01-01".to_string()),
                        ..Default::default()
                    },
                    VersionInfo {
                        version: "2.0.0".to_string(),
                        created_at: Some("2025-01-01".to_string()),
                        ..Default::default()
                    },
                ],
                stable_calls: AtomicUsize::new(0),
                list_calls: AtomicUsize::new(0),
            }
        }

        fn with_stable_result(mut self, stable_result: Option<&str>) -> Self {
            self.stable_result = stable_result.map(str::to_string);
            self
        }

        fn stable_calls(&self) -> usize {
            self.stable_calls.load(Ordering::SeqCst)
        }

        fn list_calls(&self) -> usize {
            self.list_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Backend for LatestBackend {
        fn ba(&self) -> &Arc<BackendArg> {
            &self.ba
        }

        async fn _list_remote_versions(
            &self,
            _config: &Arc<Config>,
        ) -> eyre::Result<Vec<VersionInfo>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.remote_versions.clone())
        }

        async fn latest_stable_version(
            &self,
            _config: &Arc<Config>,
        ) -> eyre::Result<Option<String>> {
            self.stable_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.stable_result.clone())
        }

        async fn install_version_(
            &self,
            _ctx: &InstallContext,
            _tv: ToolVersion,
        ) -> Result<ToolVersion> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn test_explicit_latest_uses_latest_stable_version() {
        let config = Config::get().await.unwrap();
        let backend = LatestBackend::new("test-latest-stable");

        assert_eq!(
            backend
                .latest_version(&config, Some("latest".to_string()), None)
                .await
                .unwrap()
                .as_deref(),
            Some("9.9.9")
        );
        assert_eq!(backend.stable_calls(), 1);
        assert_eq!(backend.list_calls(), 0);

        assert_eq!(
            backend
                .latest_version(&config, None, None)
                .await
                .unwrap()
                .as_deref(),
            Some("9.9.9")
        );
        assert_eq!(backend.stable_calls(), 2);
        assert_eq!(backend.list_calls(), 0);
    }

    #[tokio::test]
    async fn test_date_filtered_latest_uses_stable_when_not_newer() {
        let config = Config::get().await.unwrap();
        let backend =
            LatestBackend::new("test-latest-before-date-allowed").with_stable_result(Some("1.0.0"));
        backend
            .get_remote_version_cache()
            .lock()
            .await
            .clear()
            .unwrap();
        let before = crate::duration::parse_into_timestamp("2024-06-01").unwrap();

        assert_eq!(
            backend
                .latest_version(&config, Some("latest".to_string()), Some(before))
                .await
                .unwrap()
                .as_deref(),
            Some("1.0.0")
        );
        assert_eq!(backend.stable_calls(), 1);
        assert_eq!(backend.list_calls(), 1);
    }

    #[tokio::test]
    async fn test_date_filtered_latest_falls_back_when_stable_is_newer() {
        let config = Config::get().await.unwrap();
        let backend =
            LatestBackend::new("test-latest-before-date-newer").with_stable_result(Some("2.0.0"));
        backend
            .get_remote_version_cache()
            .lock()
            .await
            .clear()
            .unwrap();
        let before = crate::duration::parse_into_timestamp("2024-06-01").unwrap();

        assert_eq!(
            backend
                .latest_version(&config, Some("latest".to_string()), Some(before))
                .await
                .unwrap()
                .as_deref(),
            Some("1.0.0")
        );
        assert_eq!(backend.stable_calls(), 1);
        assert_eq!(backend.list_calls(), 1);
    }

    #[tokio::test]
    async fn test_date_filtered_latest_falls_back_when_stable_metadata_is_missing() {
        let config = Config::get().await.unwrap();
        let backend = LatestBackend::new("test-latest-before-date-missing-metadata")
            .with_stable_result(Some("3.0.0"));
        backend
            .get_remote_version_cache()
            .lock()
            .await
            .clear()
            .unwrap();
        let before = crate::duration::parse_into_timestamp("2024-06-01").unwrap();

        assert_eq!(
            backend
                .latest_version(&config, Some("latest".to_string()), Some(before))
                .await
                .unwrap()
                .as_deref(),
            Some("1.0.0")
        );
        assert_eq!(backend.stable_calls(), 1);
        assert_eq!(backend.list_calls(), 1);
    }

    #[test]
    fn test_latest_stable_candidate_rejects_unverified_cutoff_metadata() {
        let before = crate::duration::parse_into_timestamp("2024-06-01").unwrap();

        assert!(!latest_stable_candidate_allowed_by_before_date(
            "1.0.0", None, before
        ));
        assert!(!latest_stable_candidate_allowed_by_before_date(
            "1.0.0",
            Some(&VersionInfo {
                version: "1.0.0".to_string(),
                created_at: None,
                ..Default::default()
            }),
            before
        ));
        assert!(!latest_stable_candidate_allowed_by_before_date(
            "1.0.0",
            Some(&VersionInfo {
                version: "1.0.0".to_string(),
                created_at: Some("not-a-date".to_string()),
                ..Default::default()
            }),
            before
        ));
    }

    #[tokio::test]
    async fn test_offline_remote_versions_use_cache_without_fetching() {
        let config = Config::get().await.unwrap();
        let backend = LatestBackend::new("test-offline-cache");
        let cache = backend.get_remote_version_cache();
        {
            let cache = cache.lock().await;
            cache
                .write(&vec![
                    VersionInfo {
                        version: "1.0.0".to_string(),
                        ..Default::default()
                    },
                    VersionInfo {
                        version: "2.0.0".to_string(),
                        ..Default::default()
                    },
                ])
                .unwrap();
        }

        let mut partial = SettingsPartial::empty();
        partial.offline = Some(true);
        Settings::reset(Some(partial));
        let versions = backend.list_remote_versions(&config).await.unwrap();
        Settings::reset(None);

        assert_eq!(versions, vec!["1.0.0".to_string(), "2.0.0".to_string()]);
        assert_eq!(backend.list_calls(), 0);
    }

    #[tokio::test]
    async fn test_offline_latest_uses_fast_path_when_available() {
        let config = Config::get().await.unwrap();
        let backend = LatestBackend::new("test-offline-latest-cache");

        let mut partial = SettingsPartial::empty();
        partial.offline = Some(true);
        Settings::reset(Some(partial));
        let latest = backend.latest_version(&config, None, None).await.unwrap();
        Settings::reset(None);

        assert_eq!(latest.as_deref(), Some("9.9.9"));
        assert_eq!(backend.stable_calls(), 1);
        assert_eq!(backend.list_calls(), 0);
    }

    #[tokio::test]
    async fn test_offline_latest_falls_back_to_cached_versions_when_fast_path_has_no_result() {
        let config = Config::get().await.unwrap();
        let backend =
            LatestBackend::new("test-offline-latest-cache-fallback").with_stable_result(None);
        let cache = backend.get_remote_version_cache();
        {
            let cache = cache.lock().await;
            cache
                .write(&vec![
                    VersionInfo {
                        version: "1.0.0".to_string(),
                        ..Default::default()
                    },
                    VersionInfo {
                        version: "2.0.0".to_string(),
                        ..Default::default()
                    },
                ])
                .unwrap();
        }

        let mut partial = SettingsPartial::empty();
        partial.offline = Some(true);
        Settings::reset(Some(partial));
        let latest = backend.latest_version(&config, None, None).await.unwrap();
        Settings::reset(None);

        assert_eq!(latest.as_deref(), Some("2.0.0"));
        assert_eq!(backend.stable_calls(), 1);
        assert_eq!(backend.list_calls(), 0);
    }

    #[tokio::test]
    async fn test_latest_falls_back_to_cached_versions_when_fast_path_has_no_result() {
        Settings::reset(None);
        let config = Config::get().await.unwrap();
        let backend = LatestBackend::new("test-latest-fast-path-none").with_stable_result(None);
        let cache = backend.get_remote_version_cache();
        {
            let cache = cache.lock().await;
            cache
                .write(&vec![
                    VersionInfo {
                        version: "1.0.0".to_string(),
                        ..Default::default()
                    },
                    VersionInfo {
                        version: "2.0.0".to_string(),
                        ..Default::default()
                    },
                ])
                .unwrap();
        }

        let latest = backend.latest_version(&config, None, None).await.unwrap();

        assert_eq!(latest.as_deref(), Some("2.0.0"));
        assert_eq!(backend.stable_calls(), 1);
        assert_eq!(backend.list_calls(), 0);
    }

    #[test]
    fn test_latest_installed_version_ignores_real_latest_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut ba = BackendArg::new_raw(
            "latest-real-dir".into(),
            None,
            "latest-real-dir".into(),
            None,
            BackendResolution::new(false),
        );
        ba.installs_path = temp_dir.path().join("installs").join("latest-real-dir");
        fs::create_dir_all(ba.installs_path.join("2.0.0")).unwrap();
        fs::create_dir_all(ba.installs_path.join("latest")).unwrap();

        let backend = LatestBackend {
            ba: Arc::new(ba),
            stable_result: Some("9.9.9".to_string()),
            remote_versions: vec![],
            stable_calls: AtomicUsize::new(0),
            list_calls: AtomicUsize::new(0),
        };

        assert_eq!(
            backend.latest_installed_version(None).unwrap(),
            Some("2.0.0".into())
        );
    }

    #[tokio::test]
    async fn test_inline_install_before_wins_over_config_entry() {
        let config = Config::get().await.unwrap();
        // The test fixture has a `tiny` config entry without install_before.
        // Inline backend opts must still win when a config entry exists.
        let backend =
            LatestBackend::new("tiny[install_before=2024-06-01]").with_stable_result(Some("2.0.0"));
        backend
            .get_remote_version_cache()
            .lock()
            .await
            .clear()
            .unwrap();

        assert_eq!(
            backend
                .latest_version(&config, Some("latest".to_string()), None)
                .await
                .unwrap()
                .as_deref(),
            Some("1.0.0")
        );
        assert_eq!(backend.stable_calls(), 1);
        assert_eq!(backend.list_calls(), 1);
    }
}

/// Helper function for calculating install operation count in HTTP/S3-style backends.
/// Used by HttpBackend and S3Backend to avoid code duplication.
pub fn http_install_operation_count(
    has_checksum_opt: bool,
    platform_key: &str,
    tv: &ToolVersion,
) -> usize {
    let settings = Settings::get();
    let mut count = 2; // download + extraction
    if has_checksum_opt {
        count += 1;
    }
    let lockfile_enabled = settings.lockfile_enabled();
    let has_lockfile_checksum = tv
        .lock_platforms
        .get(platform_key)
        .and_then(|p| p.checksum.as_ref())
        .is_some();
    if lockfile_enabled || has_lockfile_checksum {
        count += 1;
    }
    count
}

/// Check that the provenance type recorded in the lockfile is still enabled in settings.
/// `is_disabled` receives the provenance type and returns `Ok(true)` when the corresponding
/// setting is off, or `Err` for provenance types unexpected in the calling backend.
pub fn ensure_provenance_setting_enabled(
    tv: &ToolVersion,
    platform_key: &str,
    is_disabled: impl FnOnce(&ProvenanceType) -> Result<bool>,
) -> Result<()> {
    let provenance = tv
        .lock_platforms
        .get(platform_key)
        .and_then(|pi| pi.provenance.as_ref());
    let Some(provenance) = provenance else {
        return Ok(());
    };
    if is_disabled(provenance)? {
        return Err(eyre!(
            "Lockfile requires {provenance} provenance for {tv} but the corresponding \
             verification setting is disabled. This may indicate a downgrade attack. \
             Enable the setting or update the lockfile."
        ));
    }
    Ok(())
}

fn find_match_in_list(list: &[String], query: &str) -> Option<String> {
    match list.contains(&query.to_string()) {
        true => Some(query.to_string()),
        false => list.last().map(|s| s.to_string()),
    }
}

/// Apply the read-time `prerelease` filter to the cached remote-versions
/// superset. Backends cache the full list and stamp `VersionInfo.prerelease`
/// either from upstream metadata or, for metadata-free listing backends, mise's
/// legacy pre-release pattern. This helper drops pre-release entries when the
/// current tool opts don't opt in.
pub(crate) fn filter_cached_prereleases(
    versions: Vec<VersionInfo>,
    want_prereleases: bool,
) -> Vec<VersionInfo> {
    if want_prereleases {
        versions
    } else {
        versions.into_iter().filter(|v| !v.prerelease).collect()
    }
}

pub(crate) fn mark_prerelease(mut version: VersionInfo) -> VersionInfo {
    if !version.prerelease && VERSION_REGEX.is_match(&version.version) {
        version.prerelease = true;
    }
    version
}

fn tool_option_bool(value: &toml::Value) -> bool {
    match value {
        toml::Value::Boolean(b) => *b,
        toml::Value::String(s) => s.parse::<bool>().unwrap_or(false),
        _ => false,
    }
}

/// Fuzzy-match `versions` against `query` with PEP 440 prerelease detection
/// applied on top of the shared filter. Used by Python-flavored backends
/// (`pipx`, the `python` core plugin) so `3.15.0a8`-style versions are dropped
/// from `latest` resolution and partial-prefix queries when the user hasn't
/// opted in to prereleases.
pub(crate) fn fuzzy_match_versions_pep440(
    versions: Vec<String>,
    query: &str,
    filter_prereleases: bool,
) -> Vec<String> {
    let versions = if filter_prereleases {
        // Mirror the exact-match bypass in `fuzzy_match_versions` so an
        // explicit prerelease request (`python@3.14.0a1`) still resolves even
        // when filter_prereleases is on.
        versions
            .into_iter()
            .filter(|v| query == v || !PEP440_PRERELEASE_REGEX.is_match(v))
            .collect()
    } else {
        versions
    };
    fuzzy_match_versions(versions, query, filter_prereleases)
}

/// Fuzzy-match `versions` against `query`. When `filter_prereleases` is true,
/// drop strings matching [`VERSION_REGEX`] (e.g. `1.0.0-rc1`, `1.0.0-dev`) —
/// the historical behavior. Backends opting into pre-releases call this with
/// `false` to keep those tags in the match set.
pub(crate) fn fuzzy_match_versions(
    versions: Vec<String>,
    query: &str,
    filter_prereleases: bool,
) -> Vec<String> {
    let escaped_query = regex::escape(query);
    let query_pattern = if query == "latest" {
        "v?[0-9].*"
    } else {
        &escaped_query
    };
    // For numeric-ish prefixes like "1.2" we want to match "1.2.3" / "1.2-rc1" etc,
    // but NOT "1.20". The old pattern achieved this by requiring a separator after the query.
    // However, vendor-prefixed queries like "temurin-" need to match digits immediately after
    // the prefix (e.g. "temurin-25.0.1").
    let query_regex = if query != "latest" && query.ends_with('-') {
        Regex::new(&format!("^{query_pattern}.*$")).unwrap()
    } else {
        Regex::new(&format!("^{query_pattern}([+\\-.].+)?$")).unwrap()
    };

    // Also create a regex without the 'v' prefix if query starts with 'v'
    // This allows "v1.0.0" to match "1.0.0" in registries that don't use v-prefix
    let query_without_v_regex = if query.starts_with('v') || query.starts_with('V') {
        let without_v = regex::escape(&query[1..]);
        let re = if query.ends_with('-') {
            Regex::new(&format!("^{without_v}.*$")).unwrap()
        } else {
            Regex::new(&format!("^{without_v}([+\\-.].+)?$")).unwrap()
        };
        Some(re)
    } else {
        None
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
            if query_regex.is_match(v) {
                return true;
            }
            if let Some(ref re) = query_without_v_regex
                && re.is_match(v)
            {
                return true;
            }
            false
        })
        .collect()
}

pub fn unalias_backend(backend: &str) -> &str {
    match backend {
        "nodejs" => "node",
        "golang" => "go",
        _ => backend.trim_start_matches("core:"),
    }
}

#[test]
fn test_unalias_backend() {
    assert_eq!(unalias_backend("node"), "node");
    assert_eq!(unalias_backend("nodejs"), "node");
    assert_eq!(unalias_backend("core:node"), "node");
    assert_eq!(unalias_backend("golang"), "go");
}

impl Display for dyn Backend {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

impl Eq for dyn Backend {}

impl PartialEq for dyn Backend {
    fn eq(&self, other: &Self) -> bool {
        self.get_plugin_type() == other.get_plugin_type() && self.id() == other.id()
    }
}

impl Hash for dyn Backend {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state)
    }
}

impl PartialOrd for dyn Backend {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for dyn Backend {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id().cmp(other.id())
    }
}

pub async fn reset() -> Result<()> {
    install_state::reset();
    *TOOLS.lock().unwrap() = None;
    load_tools().await?;
    Ok(())
}
