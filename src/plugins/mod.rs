use crate::errors::Error::PluginNotInstalled;
use crate::git::Git;
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::registry::REGISTRY;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{config::Config, dirs};
use async_trait::async_trait;
use clap::Command;
use eyre::{Result, eyre};
use heck::ToKebabCase;
use regex::Regex;
pub use script_manager::{Script, ScriptManager};
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::vec;
use std::{
    fmt::{Debug, Display},
    sync::Arc,
};

pub mod asdf_plugin;
pub mod core;
pub mod mise_plugin_toml;
pub mod script_manager;
pub mod vfox_plugin;

#[derive(Debug, Clone, Copy, PartialEq, strum::EnumString, strum::Display)]
pub enum PluginType {
    Asdf,
    Vfox,
    VfoxBackend,
}

#[derive(Debug)]
pub enum PluginEnum {
    Asdf(Arc<AsdfPlugin>),
    Vfox(Arc<VfoxPlugin>),
    VfoxBackend(Arc<VfoxPlugin>),
}

impl PluginEnum {
    pub fn name(&self) -> &str {
        match self {
            PluginEnum::Asdf(plugin) => plugin.name(),
            PluginEnum::Vfox(plugin) => plugin.name(),
            PluginEnum::VfoxBackend(plugin) => plugin.name(),
        }
    }

    pub fn path(&self) -> PathBuf {
        match self {
            PluginEnum::Asdf(plugin) => plugin.path(),
            PluginEnum::Vfox(plugin) => plugin.path(),
            PluginEnum::VfoxBackend(plugin) => plugin.path(),
        }
    }

    pub fn get_plugin_type(&self) -> PluginType {
        match self {
            PluginEnum::Asdf(_) => PluginType::Asdf,
            PluginEnum::Vfox(_) => PluginType::Vfox,
            PluginEnum::VfoxBackend(_) => PluginType::VfoxBackend,
        }
    }

    pub fn get_remote_url(&self) -> eyre::Result<Option<String>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.get_remote_url(),
            PluginEnum::Vfox(plugin) => plugin.get_remote_url(),
            PluginEnum::VfoxBackend(plugin) => plugin.get_remote_url(),
        }
    }

    pub fn set_remote_url(&self, url: String) {
        match self {
            PluginEnum::Asdf(plugin) => plugin.set_remote_url(url),
            PluginEnum::Vfox(plugin) => plugin.set_remote_url(url),
            PluginEnum::VfoxBackend(plugin) => plugin.set_remote_url(url),
        }
    }

    pub fn current_abbrev_ref(&self) -> eyre::Result<Option<String>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.current_abbrev_ref(),
            PluginEnum::Vfox(plugin) => plugin.current_abbrev_ref(),
            PluginEnum::VfoxBackend(plugin) => plugin.current_abbrev_ref(),
        }
    }

    pub fn current_sha_short(&self) -> eyre::Result<Option<String>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.current_sha_short(),
            PluginEnum::Vfox(plugin) => plugin.current_sha_short(),
            PluginEnum::VfoxBackend(plugin) => plugin.current_sha_short(),
        }
    }

    pub fn remote_sha(&self) -> eyre::Result<Option<String>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.remote_sha(),
            PluginEnum::Vfox(plugin) => plugin.remote_sha(),
            PluginEnum::VfoxBackend(plugin) => plugin.remote_sha(),
        }
    }

    pub fn external_commands(&self) -> eyre::Result<Vec<Command>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.external_commands(),
            PluginEnum::Vfox(plugin) => plugin.external_commands(),
            PluginEnum::VfoxBackend(plugin) => plugin.external_commands(),
        }
    }

    pub fn execute_external_command(&self, command: &str, args: Vec<String>) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.execute_external_command(command, args),
            PluginEnum::Vfox(plugin) => plugin.execute_external_command(command, args),
            PluginEnum::VfoxBackend(plugin) => plugin.execute_external_command(command, args),
        }
    }

    pub async fn update(&self, pr: &dyn SingleReport, gitref: Option<String>) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.update(pr, gitref).await,
            PluginEnum::Vfox(plugin) => plugin.update(pr, gitref).await,
            PluginEnum::VfoxBackend(plugin) => plugin.update(pr, gitref).await,
        }
    }

    pub async fn uninstall(&self, pr: &dyn SingleReport) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.uninstall(pr).await,
            PluginEnum::Vfox(plugin) => plugin.uninstall(pr).await,
            PluginEnum::VfoxBackend(plugin) => plugin.uninstall(pr).await,
        }
    }

    pub async fn install(&self, config: &Arc<Config>, pr: &dyn SingleReport) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.install(config, pr).await,
            PluginEnum::Vfox(plugin) => plugin.install(config, pr).await,
            PluginEnum::VfoxBackend(plugin) => plugin.install(config, pr).await,
        }
    }

    pub fn is_installed(&self) -> bool {
        match self {
            PluginEnum::Asdf(plugin) => plugin.is_installed(),
            PluginEnum::Vfox(plugin) => plugin.is_installed(),
            PluginEnum::VfoxBackend(plugin) => plugin.is_installed(),
        }
    }

    pub fn is_installed_err(&self) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.is_installed_err(),
            PluginEnum::Vfox(plugin) => plugin.is_installed_err(),
            PluginEnum::VfoxBackend(plugin) => plugin.is_installed_err(),
        }
    }

    pub async fn ensure_installed(
        &self,
        config: &Arc<Config>,
        mpr: &MultiProgressReport,
        force: bool,
        dry_run: bool,
    ) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.ensure_installed(config, mpr, force, dry_run).await,
            PluginEnum::Vfox(plugin) => plugin.ensure_installed(config, mpr, force, dry_run).await,
            PluginEnum::VfoxBackend(plugin) => {
                plugin.ensure_installed(config, mpr, force, dry_run).await
            }
        }
    }
}

impl PluginType {
    pub fn from_full(full: &str) -> eyre::Result<Self> {
        match full.split(':').next() {
            Some("asdf") => Ok(Self::Asdf),
            Some("vfox") => Ok(Self::Vfox),
            Some("vfox-backend") => Ok(Self::VfoxBackend),
            _ => Err(eyre!("unknown plugin type: {full}")),
        }
    }

    pub fn from_plugin_config(key: &str) -> (Self, &str) {
        if let Some(name) = key.strip_prefix("vfox:") {
            (Self::Vfox, name)
        } else if let Some(name) = key.strip_prefix("vfox-backend:") {
            (Self::VfoxBackend, name)
        } else if let Some(name) = key.strip_prefix("asdf:") {
            (Self::Asdf, name)
        } else {
            let path = dirs::PLUGINS.join(key.to_kebab_case());
            (Self::from_plugin_path(&path).unwrap_or(Self::Asdf), key)
        }
    }

    pub fn from_plugin_path(path: &Path) -> Option<Self> {
        if path.join("metadata.lua").exists() {
            if path.join("hooks").join("backend_install.lua").exists() {
                Some(Self::VfoxBackend)
            } else {
                Some(Self::Vfox)
            }
        } else if path.join("bin").join("list-all").exists() {
            Some(Self::Asdf)
        } else {
            None
        }
    }

    pub fn plugin(&self, short: String) -> PluginEnum {
        let path = dirs::PLUGINS.join(short.to_kebab_case());
        match self {
            PluginType::Asdf => PluginEnum::Asdf(Arc::new(AsdfPlugin::new(short, path))),
            PluginType::Vfox => PluginEnum::Vfox(Arc::new(VfoxPlugin::new(short, path))),
            PluginType::VfoxBackend => {
                PluginEnum::VfoxBackend(Arc::new(VfoxPlugin::new(short, path)))
            }
        }
    }
}

/// Warn if a plugin is an env-only vfox plugin that shadows a registry entry.
/// Env-only plugins have `hooks/mise_env.lua` but not `hooks/available.lua`.
pub fn warn_if_env_plugin_shadows_registry(name: &str, plugin_path: &Path) {
    let hooks = plugin_path.join("hooks");
    let is_env_only = hooks.join("mise_env.lua").exists() && !hooks.join("available.lua").exists();
    if is_env_only && REGISTRY.contains_key(name) {
        warn!(
            "plugin '{name}' is an env plugin and is shadowing the '{name}' registry tool - \
            consider renaming the plugin or removing it with: mise plugins rm {name}"
        );
    }
}

pub static VERSION_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(^Available versions:|-src|[-\\.]dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|-test|-nightly|-canary|-experimental|-insider|-edge|snapshot|SNAPSHOT|master)"
    )
        .unwrap()
});

/// PEP 440 separator-less pre-release segment, grounded in the canonical
/// public version grammar:
///
/// > `[N!]N(.N)*[{a|b|rc}N][.postN][.devN]`
///
/// The pre-release segment (`{a|b|rc}N`) must follow the release segment, so
/// the regex requires a leading digit. `c` is included as PEP 440's recognized
/// alternate spelling for `rc`. The trailing boundary `(?:$|[^a-z0-9])` keeps
/// it from matching inside hex hashes or other identifiers.
///
/// Only consulted by Python-flavored backends (currently `pipx`); other
/// backends would false-positive on hex hashes like `f149714c1d54`.
pub static PEP440_PRERELEASE_REGEX: Lazy<regex::Regex> =
    Lazy::new(|| Regex::new(r"(?i)[0-9](?:a|b|c|rc)[0-9]+(?:$|[^a-z0-9])").unwrap());

pub fn get(short: &str) -> Result<PluginEnum> {
    let (name, full) = short.split_once(':').unwrap_or((short, short));

    // For plugin:tool format, look up the plugin by just the plugin name
    let plugin_lookup_key = if short.contains(':') {
        // Check if the part before the colon is a plugin name
        if let Some(_plugin_type) = install_state::list_plugins().get(name) {
            name
        } else {
            short
        }
    } else {
        short
    };

    let plugin_type =
        if let Some(plugin_type) = install_state::list_plugins().get(plugin_lookup_key) {
            *plugin_type
        } else {
            PluginType::from_full(full)?
        };
    Ok(plugin_type.plugin(name.to_string()))
}

#[allow(unused_variables)]
#[async_trait]
pub trait Plugin: Debug + Send {
    fn name(&self) -> &str;
    fn path(&self) -> PathBuf;
    fn get_remote_url(&self) -> eyre::Result<Option<String>>;
    fn set_remote_url(&self, url: String) {}
    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>>;
    fn current_sha_short(&self) -> eyre::Result<Option<String>>;
    fn remote_sha(&self) -> eyre::Result<Option<String>> {
        Ok(None)
    }
    fn is_installed(&self) -> bool {
        true
    }
    fn is_installed_err(&self) -> eyre::Result<()> {
        if !self.is_installed() {
            return Err(PluginNotInstalled(self.name().to_string()).into());
        }
        Ok(())
    }

    async fn ensure_installed(
        &self,
        _config: &Arc<Config>,
        _mpr: &MultiProgressReport,
        _force: bool,
        _dry_run: bool,
    ) -> eyre::Result<()> {
        Ok(())
    }
    async fn update(&self, _pr: &dyn SingleReport, _gitref: Option<String>) -> eyre::Result<()> {
        Ok(())
    }
    async fn uninstall(&self, _pr: &dyn SingleReport) -> eyre::Result<()> {
        Ok(())
    }
    async fn install(&self, _config: &Arc<Config>, _pr: &dyn SingleReport) -> eyre::Result<()> {
        Ok(())
    }
    fn external_commands(&self) -> eyre::Result<Vec<Command>> {
        Ok(vec![])
    }
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn execute_external_command(&self, _command: &str, _args: Vec<String>) -> eyre::Result<()> {
        unimplemented!(
            "execute_external_command not implemented for {}",
            self.name()
        )
    }
}

impl Ord for PluginEnum {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name().cmp(other.name())
    }
}

impl PartialOrd for PluginEnum {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for PluginEnum {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for PluginEnum {}

impl Display for PluginEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone)]
pub enum PluginSource {
    /// Git repository with URL and optional ref
    Git {
        url: String,
        git_ref: Option<String>,
    },
    /// Zip file accessible via HTTPS
    Zip { url: String },
}

impl PluginSource {
    pub fn parse(repository: &str) -> Self {
        // Split Parameters
        let url_path = repository
            .split('?')
            .next()
            .unwrap_or(repository)
            .split('#')
            .next()
            .unwrap_or(repository);
        // Check if it's a zip file (ends with -zip)
        if url_path.to_lowercase().ends_with(".zip") {
            return PluginSource::Zip {
                url: repository.to_string(),
            };
        }
        // Otherwise treat as git repository
        let (url, git_ref) = Git::split_url_and_ref(repository);
        PluginSource::Git {
            url: url.to_string(),
            git_ref: git_ref.map(|s| s.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_source_parse_git() {
        // Test parsing Git URL
        let source = PluginSource::parse("https://github.com/user/plugin.git");
        match source {
            PluginSource::Git { url, git_ref } => {
                assert_eq!(url, "https://github.com/user/plugin.git");
                assert_eq!(git_ref, None);
            }
            _ => panic!("Expected a git plugin"),
        }
    }

    #[test]
    fn test_plugin_source_parse_git_with_ref() {
        // Test parsing Git URL with refs
        let source = PluginSource::parse("https://github.com/user/plugin.git#v1.0.0");
        match source {
            PluginSource::Git { url, git_ref } => {
                assert_eq!(url, "https://github.com/user/plugin.git");
                assert_eq!(git_ref, Some("v1.0.0".to_string()));
            }
            _ => panic!("Expected a git plugin"),
        }
    }

    #[test]
    fn test_plugin_source_parse_zip() {
        // Test parsing zip URL
        let source = PluginSource::parse("https://example.com/plugins/my-plugin.zip");
        match source {
            PluginSource::Zip { url } => {
                assert_eq!(url, "https://example.com/plugins/my-plugin.zip");
            }
            _ => panic!("Expected a Zip source"),
        }
    }

    #[test]
    fn test_plugin_source_parse_uppercase_zip_with_query() {
        // Test parsing zip URL with query
        let source =
            PluginSource::parse("https://example.com/plugins/my-plugin.ZIP?version=v1.0.0");
        match source {
            PluginSource::Zip { url } => {
                assert_eq!(
                    url,
                    "https://example.com/plugins/my-plugin.ZIP?version=v1.0.0"
                );
            }
            _ => panic!("Expected a Zip source"),
        }
    }

    #[test]
    fn test_plugin_source_parse_edge_cases() {
        // Test parsing git url which contains `.zip`
        let source = PluginSource::parse("https://example.com/.zip/plugin");
        match source {
            PluginSource::Git { .. } => {}
            _ => panic!("Expected a git plugin"),
        }
    }

    #[test]
    fn test_plugin_type_from_plugin_config() {
        assert_eq!(
            PluginType::from_plugin_config("vfox:node"),
            (PluginType::Vfox, "node")
        );
        assert_eq!(
            PluginType::from_plugin_config("vfox-backend:npm"),
            (PluginType::VfoxBackend, "npm")
        );
        assert_eq!(
            PluginType::from_plugin_config("asdf:node"),
            (PluginType::Asdf, "node")
        );
        assert_eq!(
            PluginType::from_plugin_config("missing-test-plugin"),
            (PluginType::Asdf, "missing-test-plugin")
        );
    }

    #[test]
    fn test_plugin_type_from_plugin_path() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(PluginType::from_plugin_path(dir.path()), None);

        let asdf = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(asdf.path().join("bin")).unwrap();
        std::fs::write(asdf.path().join("bin").join("list-all"), "").unwrap();
        assert_eq!(
            PluginType::from_plugin_path(asdf.path()),
            Some(PluginType::Asdf)
        );

        let vfox = tempfile::tempdir().unwrap();
        std::fs::write(vfox.path().join("metadata.lua"), "").unwrap();
        assert_eq!(
            PluginType::from_plugin_path(vfox.path()),
            Some(PluginType::Vfox)
        );

        let backend = tempfile::tempdir().unwrap();
        std::fs::write(backend.path().join("metadata.lua"), "").unwrap();
        std::fs::create_dir_all(backend.path().join("hooks")).unwrap();
        std::fs::write(backend.path().join("hooks").join("backend_install.lua"), "").unwrap();
        assert_eq!(
            PluginType::from_plugin_path(backend.path()),
            Some(PluginType::VfoxBackend)
        );
    }

    #[test]
    fn test_version_regex_filters_prerelease() {
        // Standard pre-release patterns
        assert!(VERSION_REGEX.is_match("1.0.0-alpha"));
        assert!(VERSION_REGEX.is_match("1.0.0-beta"));
        assert!(VERSION_REGEX.is_match("1.0.0-rc1"));
        assert!(VERSION_REGEX.is_match("1.0.0.rc1"));
        assert!(VERSION_REGEX.is_match("1.0.0-dev"));
        assert!(VERSION_REGEX.is_match("1.0.0-pre1"));
        assert!(VERSION_REGEX.is_match("1.0.0.pre1"));

        // PEP 440 dot-separated dev versions (GitHub discussion #8784)
        assert!(
            VERSION_REGEX.is_match("2026.3.3.dev0"),
            "PEP 440 .dev suffix should be filtered"
        );
        assert!(
            VERSION_REGEX.is_match("2026.3.3.162408.dev0"),
            "PEP 440 .dev suffix with build number should be filtered"
        );

        // npm prerelease channels (GitHub discussion #9503).
        // Use suffixes that don't accidentally match `([abc])[0-9]+`, so each
        // assertion exercises only the channel-tag alternative it names.
        assert!(
            VERSION_REGEX.is_match("0.42.0-nightly.20260429.g6d9911393"),
            "npm -nightly tag should be filtered"
        );
        assert!(
            VERSION_REGEX.is_match("13.0.0-canary"),
            "npm -canary tag should be filtered"
        );
        assert!(
            VERSION_REGEX.is_match("18.0.0-experimental.1"),
            "npm -experimental tag should be filtered"
        );
        assert!(
            VERSION_REGEX.is_match("1.99.0-insider"),
            "npm -insider tag should be filtered"
        );
        assert!(
            VERSION_REGEX.is_match("1.99.0-edge"),
            "npm -edge tag should be filtered"
        );

        // Stable versions should NOT match
        assert!(!VERSION_REGEX.is_match("1.0.0"));
        assert!(!VERSION_REGEX.is_match("2026.3.3"));
        assert!(!VERSION_REGEX.is_match("22.6.0"));

        // PEP 440 separator-less suffixes (`3.12.0a1`, `1.2.3c1`) live in
        // PEP440_PRERELEASE_REGEX, not the general regex — see that test below.
        assert!(!VERSION_REGEX.is_match("3.12.0a1"));
        assert!(!VERSION_REGEX.is_match("1.2.3c1"));

        // Go pseudo-versions and other identifiers with incidental `[abc]\d`
        // substrings (commit hashes) must not be flagged.
        assert!(!VERSION_REGEX.is_match("2.0.0-20260404020628-f149714c1d54"));
    }

    #[test]
    fn test_pep440_prerelease_regex() {
        // Canonical PEP 440 pre-release segments: `aN`, `bN`, `rcN`, plus the
        // recognized `cN` alias for `rcN`.
        assert!(PEP440_PRERELEASE_REGEX.is_match("3.12.0a1"));
        assert!(PEP440_PRERELEASE_REGEX.is_match("3.12.0b2"));
        assert!(PEP440_PRERELEASE_REGEX.is_match("1.2.3c1"));
        assert!(PEP440_PRERELEASE_REGEX.is_match("1.2.3rc1"));
        assert!(PEP440_PRERELEASE_REGEX.is_match("1.0.0c1+build"));
        assert!(PEP440_PRERELEASE_REGEX.is_match("1.0.0a1.dev0"));

        // Stable releases — including `.postN`, which PEP 440 specifies as a
        // post-release (after a stable), NOT a pre-release.
        assert!(!PEP440_PRERELEASE_REGEX.is_match("1.0.0"));
        assert!(!PEP440_PRERELEASE_REGEX.is_match("3.12.0"));
        assert!(!PEP440_PRERELEASE_REGEX.is_match("1.0.0.post1"));

        // The `{a|b|rc}N` segment must follow the release segment per the
        // PEP 440 grammar — the leading-digit anchor enforces that. Identifiers
        // whose hex hashes happen to contain `c1` / `a1` / `b2` substrings
        // (e.g. Go pseudo-versions) do not match because the `[abc]` is
        // preceded by a hex letter, not a digit.
        assert!(
            !PEP440_PRERELEASE_REGEX.is_match("2.0.0-20260404020628-f149714c1d54"),
            "Go pseudo-version with `c1` in hash should not match"
        );
        assert!(
            !PEP440_PRERELEASE_REGEX.is_match("1.0.0-20240101000000-a1b2c3d4e5f6"),
            "Go pseudo-version with `a1`/`b2`/`c3` in hash should not match"
        );

        // Bare `aN` / `bN` / `cN` not attached to a release segment (uncommon
        // but possible identifier shapes) is also rejected.
        assert!(!PEP440_PRERELEASE_REGEX.is_match("a1"));
        assert!(!PEP440_PRERELEASE_REGEX.is_match("b1234567"));
    }
}
