use crate::backend::backend_type::BackendType;
use crate::backend::{ABackend, unalias_backend};
use crate::config::Config;
use crate::plugins::PluginType;
use crate::registry::REGISTRY;
use crate::toolset::install_state::InstallStateTool;
use crate::toolset::{ToolVersionOptions, install_state, parse_tool_options};
use crate::{backend, config, dirs, lockfile, registry};
use contracts::requires;
use eyre::{Result, bail};
use heck::{ToKebabCase, ToShoutySnakeCase};
use std::collections::HashSet;
use std::env;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::PathBuf;
use xx::regex;

/// Metadata about how a backend was resolved.
/// This struct is designed for extensibility - additional fields can be added
/// as needed without breaking existing code.
#[derive(Clone, Debug, Default)]
pub struct BackendResolution {
    /// Whether the user explicitly specified the full backend (e.g., "aqua:oven-sh/bun" vs "bun").
    /// Also true when restored from install state for backward compatibility with existing installations,
    /// and for plugin-based tools initialized from the plugin registry.
    pub explicit: bool,
}

impl BackendResolution {
    pub fn new(explicit: bool) -> Self {
        Self { explicit }
    }
}

#[derive(Clone)]
pub struct BackendArg {
    /// short or full identifier (what the user specified), "node", "prettier", "npm:prettier", "cargo:eza"
    pub short: String,
    /// full identifier, "core:node", "npm:prettier", "cargo:eza", "vfox:version-fox/vfox-nodejs"
    full: Option<String>,
    /// the name of the tool within the backend, e.g.: "node", "prettier", "eza", "vfox-nodejs"
    pub tool_name: String,
    /// ~/.local/share/mise/cache/<THIS>
    pub cache_path: PathBuf,
    /// ~/.local/share/mise/installs/<THIS>
    pub installs_path: PathBuf,
    /// ~/.local/share/mise/downloads/<THIS>
    pub downloads_path: PathBuf,
    pub opts: Option<ToolVersionOptions>,
    resolution: BackendResolution,
    // TODO: make this not a hash key anymore to use this
    // backend: OnceCell<ABackend>,
}

impl<A: AsRef<str>> From<A> for BackendArg {
    fn from(s: A) -> Self {
        let short = unalias_backend(s.as_ref()).to_string();
        // Check if this is a full backend identifier (e.g., "aqua:oven-sh/bun")
        // If so, treat it as explicit since the user specified the backend
        let explicit = if let Some((prefix, _)) = short.split_once(':') {
            BackendType::guess(prefix) != BackendType::Unknown
        } else {
            false
        };
        let (short_parsed, tool_name, opts) = parse_backend_components(&short, None);
        Self::new_raw(
            short_parsed,
            None,
            tool_name,
            opts,
            BackendResolution::new(explicit),
        )
    }
}

impl From<InstallStateTool> for BackendArg {
    fn from(ist: InstallStateTool) -> Self {
        let (short, tool_name, opts) = parse_backend_components(&ist.short, ist.full.as_ref());
        Self::new_raw(
            short,
            ist.full,
            tool_name,
            opts,
            BackendResolution::new(ist.explicit_backend),
        )
    }
}

fn parse_backend_components(
    short: &str,
    full: Option<&String>,
) -> (String, String, Option<ToolVersionOptions>) {
    let short = unalias_backend(short).to_string();
    let (_backend, mut tool_name) = full
        .unwrap_or(&short)
        .split_once(':')
        .unwrap_or(("", full.unwrap_or(&short)));
    let short = regex!(r#"\[.+\]$"#).replace_all(&short, "").to_string();

    let mut opts = None;
    if let Some(c) = regex!(r"^(.+)\[(.+)\]$").captures(tool_name) {
        tool_name = c.get(1).unwrap().as_str();
        opts = Some(parse_tool_options(c.get(2).unwrap().as_str()));
    }

    (short, tool_name.to_string(), opts)
}

impl BackendArg {
    #[requires(!short.is_empty())]
    pub fn new(short: String, full: Option<String>) -> Self {
        let resolution = BackendResolution::new(full.is_some());
        let (short, tool_name, opts) = parse_backend_components(&short, full.as_ref());
        Self::new_raw(short, full, tool_name, opts, resolution)
    }

    pub fn new_raw(
        short: String,
        full: Option<String>,
        tool_name: String,
        opts: Option<ToolVersionOptions>,
        resolution: BackendResolution,
    ) -> Self {
        let pathname = short.to_kebab_case();
        Self {
            tool_name,
            short,
            full,
            cache_path: dirs::CACHE.join(&pathname),
            installs_path: dirs::INSTALLS.join(&pathname),
            downloads_path: dirs::DOWNLOADS.join(&pathname),
            opts,
            resolution,
            // backend: Default::default(),
        }
    }

    pub fn backend(&self) -> Result<ABackend> {
        // TODO: see above about hash key
        // let backend = self.backend.get_or_try_init(|| {
        //     if let Some(backend) = backend::get(self) {
        //         Ok(backend)
        //     } else {
        //         bail!("{self} not found in mise tool registry");
        //     }
        // })?;
        // Ok(backend.clone())
        if let Some(backend) = backend::get(self) {
            Ok(backend)
        } else if let Some((plugin_name, tool_name)) = self.short.split_once(':') {
            // Check if the plugin exists first
            if let Some(plugin_type) = install_state::get_plugin_type(plugin_name) {
                // Plugin exists, but the backend couldn't be created
                // This could be due to the tool not being available or plugin not properly installed
                match plugin_type {
                    PluginType::Asdf => {
                        bail!(
                            "asdf plugin '{plugin_name}' exists but '{tool_name}' is not available or the plugin is not properly installed"
                        );
                    }
                    PluginType::Vfox => {
                        bail!(
                            "vfox plugin '{plugin_name}' exists but '{tool_name}' is not available or the plugin is not properly installed"
                        );
                    }
                    PluginType::VfoxBackend => {
                        bail!(
                            "vfox-backend plugin '{plugin_name}' exists but '{tool_name}' is not available or the plugin is not properly installed"
                        );
                    }
                }
            } else {
                // Plugin doesn't exist
                bail!("{plugin_name} is not a valid plugin name");
            }
        } else {
            let registry_shorts: Vec<&str> = REGISTRY.keys().copied().collect();
            let mut suggestions: Vec<String> =
                xx::suggest::similar_n_with_threshold(&self.short, &registry_shorts, 3, 0.8)
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();

            let mise_names: HashSet<String> = suggestions.iter().cloned().collect();
            for aqua_id in crate::aqua::aqua_registry_wrapper::aqua_suggest(&self.short) {
                // Skip aqua suggestions whose tool name matches an existing mise suggestion
                let name = aqua_id
                    .rsplit_once('/')
                    .map_or(aqua_id.as_str(), |(_, n)| n);
                if !mise_names.contains(name) {
                    suggestions.push(format!("aqua:{aqua_id}"));
                }
            }

            let mut msg = format!("{self} not found in mise tool registry");
            if !suggestions.is_empty() {
                msg.push_str("\n\nDid you mean?");
                for s in suggestions.iter().take(5) {
                    msg.push_str(&format!("\n  {s}"));
                }
            }
            bail!("{msg}");
        }
    }

    pub fn backend_type(&self) -> BackendType {
        // Check if this is a valid backend:tool format first
        if let Some((backend_prefix, _tool_name)) = self.short.split_once(':')
            && let Ok(backend_type) = backend_prefix.parse::<BackendType>()
        {
            return backend_type;
        }

        // Then check if this is a vfox plugin:tool format
        if let Some((plugin_name, _tool_name)) = self.short.split_once(':') {
            // we cannot reliably determine backend type within install state so we check config first
            if config::is_loaded() && Config::get_().get_repo_url(plugin_name).is_some() {
                return BackendType::VfoxBackend(plugin_name.to_string());
            }
            if let Some(plugin_type) = install_state::get_plugin_type(plugin_name) {
                return match plugin_type {
                    PluginType::Vfox => BackendType::Vfox,
                    PluginType::VfoxBackend => BackendType::VfoxBackend(plugin_name.to_string()),
                    PluginType::Asdf => BackendType::Asdf,
                };
            }
        }

        // Only check install state for non-plugin:tool format entries
        if !self.short.contains(':')
            && let Ok(Some(backend_type)) = install_state::backend_type(&self.short)
        {
            return backend_type;
        }

        let full = self.full();
        let backend = full.split(':').next().unwrap();
        if let Ok(backend_type) = backend.parse() {
            return backend_type;
        }
        if config::is_loaded()
            && let Some(repo_url) = Config::get_().get_repo_url(&self.short)
        {
            return if repo_url.contains("vfox-") {
                BackendType::Vfox
            } else {
                // TODO: maybe something more intelligent?
                BackendType::Asdf
            };
        }
        BackendType::Unknown
    }

    pub fn full(&self) -> String {
        let short = unalias_backend(&self.short);

        // Check for environment variable override first
        // e.g., MISE_BACKENDS_MYTOOLS='github:myorg/mytools'
        let env_key = format!("MISE_BACKENDS_{}", short.to_shouty_snake_case());
        if let Ok(env_value) = env::var(&env_key) {
            return env_value;
        }

        if config::is_loaded() {
            if let Some(full) = Config::get_()
                .all_aliases
                .get(short)
                .and_then(|a| a.backend.clone())
            {
                return full;
            }
            if let Some(url) = Config::get_().repo_urls.get(short) {
                return format!("asdf:{url}");
            }

            let config = Config::get_();
            if let Some(backend) = lockfile::get_locked_backend(&config, short) {
                return backend;
            }
        }

        // For non-explicit short-name tools that are not plugins, use registry's current
        // backend if available. This allows tools to automatically switch backends when
        // the registry changes (e.g., when a tool moves from one maintainer to another).
        if !self.resolution.explicit
            && install_state::get_plugin_type(short).is_none()
            && let Some(registry_full) = REGISTRY
                .get(short)
                .and_then(|rt| rt.backends().first().cloned())
        {
            if let Some(stored_full) = &self.full
                && stored_full != registry_full
            {
                debug!(
                    "backend for '{short}' changed from stored '{stored_full}' to registry '{registry_full}'"
                );
            }
            return registry_full.to_string();
        }

        if let Some(full) = &self.full {
            full.clone()
        } else if let Some(full) = install_state::get_tool_full(short) {
            full
        } else if let Some((plugin_name, _tool_name)) = short.split_once(':') {
            // Check if this is a plugin:tool format
            if BackendType::guess(short) != BackendType::Unknown {
                // Handle built-in backends
                short.to_string()
            } else if let Some(pt) = install_state::get_plugin_type(plugin_name) {
                match pt {
                    PluginType::Asdf => {
                        // For asdf plugins, plugin:tool format is invalid
                        // Return just the plugin name since asdf doesn't support plugin:tool structure
                        plugin_name.to_string()
                    }
                    // For vfox plugins, when already in plugin:tool format, return as-is
                    // because the plugin itself is the backend specification
                    PluginType::Vfox => short.to_string(),
                    PluginType::VfoxBackend => short.to_string(),
                }
            } else if plugin_name.starts_with("asdf-") {
                // Handle asdf plugin:tool format even if not installed
                plugin_name.to_string()
            } else {
                short.to_string()
            }
        } else if let Some(pt) = install_state::get_plugin_type(short) {
            match pt {
                PluginType::Asdf => format!("asdf:{short}"),
                PluginType::Vfox => format!("vfox:{short}"),
                PluginType::VfoxBackend => short.to_string(),
            }
        } else if let Some(full) = REGISTRY
            .get(short)
            .and_then(|rt| rt.backends().first().cloned())
        {
            full.to_string()
        } else {
            short.to_string()
        }
    }

    pub fn full_with_opts(&self) -> String {
        let full = self.full();
        if regex!(r"^(.+)\[(.+)\]$").is_match(&full) {
            return full;
        }
        if let Some(opts) = &self.opts {
            let opts_str = opts
                .opts
                .iter()
                // filter out global options that are only relevant for initial installation
                .filter(|(k, _)| !["postinstall", "install_env"].contains(&k.as_str()))
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            if !full.contains(['[', ']']) && !opts_str.is_empty() {
                return format!("{full}[{opts_str}]");
            }
        }
        full
    }

    pub fn full_without_opts(&self) -> String {
        let full = self.full();
        if let Some(c) = regex!(r"^(.+)\[(.+)\]$").captures(&full) {
            return c.get(1).unwrap().as_str().to_string();
        }
        full
    }

    pub fn opts(&self) -> ToolVersionOptions {
        // Start with registry options as base (if available)
        // Use backend_options to get options specific to the backend being used
        let full = self.full();
        let mut opts = REGISTRY
            .get(self.short.as_str())
            .map(|rt| rt.backend_options(&full))
            .unwrap_or_default();

        // Get user-provided options (from self.opts or from full string)
        let user_opts = self.opts.clone().unwrap_or_else(|| {
            if let Some(c) = regex!(r"^(.+)\[(.+)\]$").captures(&full) {
                parse_tool_options(c.get(2).unwrap().as_str())
            } else {
                ToolVersionOptions::default()
            }
        });

        // Merge user options on top (user options take precedence)
        for (k, v) in user_opts.opts {
            opts.opts.insert(k, v);
        }
        for (k, v) in user_opts.install_env {
            opts.install_env.insert(k, v);
        }
        if user_opts.os.is_some() {
            opts.os = user_opts.os;
        }

        opts
    }

    pub fn set_opts(&mut self, opts: Option<ToolVersionOptions>) {
        self.opts = opts;
    }

    /// Returns true if the user explicitly specified the full backend identifier.
    /// When false and the tool is not plugin-based, it may resolve to the current
    /// registry backend on next operation, allowing automatic backend migration
    /// when registry/ is updated.
    pub fn has_explicit_backend(&self) -> bool {
        self.resolution.explicit
    }

    /// Returns the stored backend identifier, preferring the explicitly stored value
    /// over dynamic registry resolution. For non-explicit tools, uses `full()` which
    /// respects registry updates, allowing automatic backend migration when registry/
    /// is updated. Used for lockfiles to preserve the actual installed backend when possible.
    /// Options are stripped since lockfiles have a separate options field.
    pub fn stored_full(&self) -> String {
        // For non-explicit tools, use full() which respects registry updates.
        // This allows tools to automatically switch backends when the registry changes.
        if !self.resolution.explicit {
            let full = self.full();
            // Strip options since lockfiles have a separate options field
            if let Some(c) = regex!(r"^(.+)\[(.+)\]$").captures(&full) {
                return c.get(1).unwrap().as_str().to_string();
            }
            return full;
        }

        // For explicit tools, preserve the stored value
        let full = if let Some(full) = &self.full {
            full.clone()
        } else {
            let short = unalias_backend(&self.short);
            if let Some(full) = install_state::get_tool_full(short) {
                full
            } else if let Some(pt) = install_state::get_plugin_type(short) {
                match pt {
                    PluginType::Asdf => format!("asdf:{short}"),
                    PluginType::Vfox => format!("vfox:{short}"),
                    PluginType::VfoxBackend => short.to_string(),
                }
            } else {
                self.full()
            }
        };
        // Strip options since lockfiles have a separate options field
        if let Some(c) = regex!(r"^(.+)\[(.+)\]$").captures(&full) {
            return c.get(1).unwrap().as_str().to_string();
        }
        full
    }

    pub fn tool_name(&self) -> String {
        let full = self.full();
        let (_backend, tool_name) = full.split_once(':').unwrap_or(("", &full));
        let tool_name = regex!(r#"\[.+\]$"#).replace_all(tool_name, "").to_string();
        tool_name.to_string()
    }

    /// maps something like cargo:cargo-binstall to cargo-binstall and ubi:cargo-binstall, etc
    pub fn all_fulls(&self) -> HashSet<String> {
        let full = self.full();
        let mut all = HashSet::new();
        for short in registry::shorts_for_full(&full) {
            let rt = REGISTRY.get(short).unwrap();
            let backends = rt.backends();
            if backends.contains(&full.as_str()) {
                all.insert(rt.short.to_string());
                all.extend(backends.into_iter().map(|s| s.to_string()));
            }
        }
        all.insert(full);
        all.insert(self.short.to_string());
        all
    }

    pub fn is_os_supported(&self) -> bool {
        if self.uses_plugin() {
            return true;
        }
        if let Some(rt) = REGISTRY.get(self.short.as_str()) {
            return rt.is_supported_os();
        }
        true
    }

    pub fn uses_plugin(&self) -> bool {
        install_state::get_plugin_type(&self.short).is_some()
    }
}

impl Display for BackendArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short)
    }
}

impl Debug for BackendArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(full) = &self.full {
            write!(f, r#"BackendArg({} -> {})"#, self.short, full)
        } else {
            write!(f, r#"BackendArg({})"#, self.short)
        }
    }
}

impl PartialEq for BackendArg {
    fn eq(&self, other: &Self) -> bool {
        self.short == other.short
    }
}

impl Eq for BackendArg {}

impl PartialOrd for BackendArg {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BackendArg {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.short.cmp(&other.short)
    }
}

impl Hash for BackendArg {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.short.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_str_eq};

    #[tokio::test]
    async fn test_backend_arg() {
        let _config = Config::get().await.unwrap();
        let t = |s: &str, full, tool_name, t| {
            let fa: BackendArg = s.into();
            assert_str_eq!(full, fa.full());
            assert_str_eq!(tool_name, fa.tool_name);
            assert_eq!(t, fa.backend_type());
        };
        #[cfg(unix)]
        let asdf = |s, full, name| t(s, full, name, BackendType::Asdf);
        let cargo = |s, full, name| t(s, full, name, BackendType::Cargo);
        // let core = |s, full, name| t(s, full, name, BackendType::Core);
        let npm = |s, full, name| t(s, full, name, BackendType::Npm);
        let vfox = |s, full, name| t(s, full, name, BackendType::Vfox);

        #[cfg(unix)]
        {
            asdf("asdf:clojure", "asdf:clojure", "clojure");
            asdf("clojure", "asdf:mise-plugins/mise-clojure", "clojure");
        }
        cargo("cargo:eza", "cargo:eza", "eza");
        // core("node", "node", "node");
        npm("npm:@antfu/ni", "npm:@antfu/ni", "@antfu/ni");
        npm("npm:prettier", "npm:prettier", "prettier");
        vfox(
            "vfox:version-fox/vfox-nodejs",
            "vfox:version-fox/vfox-nodejs",
            "version-fox/vfox-nodejs",
        );
    }

    #[tokio::test]
    async fn test_backend_arg_pathname() {
        let _config = Config::get().await.unwrap();
        let t = |s: &str, expected| {
            let fa: BackendArg = s.into();
            let actual = fa.installs_path.to_string_lossy();
            let expected = dirs::INSTALLS.join(expected);
            assert_str_eq!(actual, expected.to_string_lossy());
        };
        t("asdf:node", "asdf-node");
        t("node", "node");
        t("cargo:eza", "cargo-eza");
        t("npm:@antfu/ni", "npm-antfu-ni");
        t("npm:prettier", "npm-prettier");
        t(
            "vfox:version-fox/vfox-nodejs",
            "vfox-version-fox-vfox-nodejs",
        );
        t("vfox:version-fox/nodejs", "vfox-version-fox-nodejs");
    }

    #[tokio::test]
    async fn test_backend_arg_bug_fixes() {
        let _config = Config::get().await.unwrap();

        // Test that asdf plugins in plugin:tool format return just the plugin name
        // (asdf doesn't support plugin:tool structure)
        let fa: BackendArg = "asdf-plugin:tool".into();
        assert_str_eq!("asdf-plugin", fa.full());

        // Test that vfox plugins in plugin:tool format return as-is
        let fa: BackendArg = "vfox-plugin:tool".into();
        assert_str_eq!("vfox-plugin:tool", fa.full());
    }

    #[tokio::test]
    async fn test_backend_arg_improved_error_messages() {
        let _config = Config::get().await.unwrap();

        // Test that when a plugin exists but the tool is not available,
        // we get a more specific error message instead of "not a valid backend name"
        let fa: BackendArg = "nonexistent-plugin:some-tool".into();
        let result = fa.backend();
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("is not a valid plugin name"),
            "Expected error to mention invalid plugin name, got: {error_msg}"
        );

        // Note: We can't easily test the case where a plugin exists but the tool doesn't
        // because that would require setting up actual plugins in the test environment.
        // The logic has been improved to check plugin existence first and provide
        // more specific error messages based on the plugin type.
    }

    #[tokio::test]
    async fn test_full_with_opts_appends_and_filters() {
        let _config = Config::get().await.unwrap();

        // start with a normal full like "npm:prettier" and attach opts via set_opts
        let mut fa: BackendArg = "npm:prettier".into();
        fa.set_opts(Some(parse_tool_options("a=1,install_env=ignored,b=2")));
        // install_env should be filtered out, remaining order preserved
        assert_str_eq!("npm:prettier[a=1,b=2]", fa.full_with_opts());

        fa = "http:hello-lock".into();
        fa.set_opts(Some(parse_tool_options("url=https://mise.jdx.dev/test-fixtures/hello-world-1.0.0.tar.gz,bin_path=hello-world-1.0.0/bin")));
        // install_env should be filtered out, remaining order preserved
        assert_str_eq!(
            "http:hello-lock[url=https://mise.jdx.dev/test-fixtures/hello-world-1.0.0.tar.gz,bin_path=hello-world-1.0.0/bin]",
            fa.full_with_opts()
        );
    }

    #[tokio::test]
    async fn test_full_with_opts_preserves_existing_brackets() {
        let _config = Config::get().await.unwrap();

        // when the full already contains options brackets, full_with_opts should return it unchanged
        let mut fa = BackendArg::new_raw(
            "node".to_string(),
            Some("node[foo=bar]".to_string()),
            "node".to_string(),
            None,
            BackendResolution::new(true),
        );
        assert_str_eq!("node[foo=bar]", fa.full_with_opts());

        fa = BackendArg::new_raw(
            "gitlab:jdxcode/mise-test-fixtures".to_string(),
            Some("gitlab:jdxcode/mise-test-fixtures[asset_pattern=hello-world-1.0.0.tar.gz,bin_path=hello-world-1.0.0/bin]".to_string()),
            "gitlab:jdxcode/mise-test-fixtures".to_string(),
            None,
            BackendResolution::new(true),
        );
        assert_str_eq!(
            "gitlab:jdxcode/mise-test-fixtures[asset_pattern=hello-world-1.0.0.tar.gz,bin_path=hello-world-1.0.0/bin]",
            fa.full_with_opts()
        );
    }
}
