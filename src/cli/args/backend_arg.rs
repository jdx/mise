use crate::backend::backend_type::BackendType;
use crate::backend::{ABackend, unalias_backend};
use crate::config::Config;
use crate::plugins::PluginType;
use crate::registry::REGISTRY;
use crate::toolset::install_state::InstallStateTool;
use crate::toolset::{
    EPHEMERAL_OPT_KEYS, ToolVersionOptions, install_state, parse_tool_options,
    serialize_tool_options, try_parse_tool_options,
};
use crate::{backend, config, dirs, lockfile, registry};
use contracts::requires;
use eyre::{Result, bail};
use heck::{ToKebabCase, ToShoutySnakeCase};
use std::collections::HashSet;
use std::env;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::path::PathBuf;
use std::str::FromStr;

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
        let (short, tool_name, mut opts) = parse_backend_components(&ist.short, ist.full.as_ref());

        // Merge manifest opts into the parsed opts (manifest opts provide defaults)
        if !ist.opts.is_empty() {
            let tvo = opts.get_or_insert_with(ToolVersionOptions::default);
            for (k, v) in ist.opts {
                tvo.opts.entry(k).or_insert(v);
            }
        }

        let mut tool = Self::new_raw(
            short,
            ist.full,
            tool_name,
            opts,
            BackendResolution::new(ist.explicit_backend),
        );
        if let Some(installs_path) = ist.installs_path {
            tool.installs_path = installs_path;
        }
        tool
    }
}

/// Split a string like `"http:hello[url=...,bin=bin]"` into `("http:hello", "url=...,bin=bin")`.
/// Returns `None` if no bracketed opts are present.
pub fn split_bracketed_opts(s: &str) -> Option<(&str, &str)> {
    if !s.ends_with(']') {
        return None;
    }

    let mut bracket_start = None;
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;

    for (index, ch) in s.char_indices() {
        match ch {
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            '"' if !in_single_quotes && !escaped => in_double_quotes = !in_double_quotes,
            '[' if !in_single_quotes && !in_double_quotes && bracket_start.is_none() => {
                bracket_start = Some(index);
            }
            ']' if !in_single_quotes && !in_double_quotes && index == s.len() - 1 => {
                if let Some(start) = bracket_start {
                    return Some((&s[..start], &s[start + 1..index]));
                }
                return None;
            }
            _ => {}
        }

        escaped = in_double_quotes && ch == '\\' && !escaped;
    }

    None
}

/// Strip trailing `[...]` opts from a string, e.g. `"foo[a=1]"` → `"foo"`.
pub(crate) fn strip_opts(s: &str) -> String {
    split_bracketed_opts(s)
        .map(|(name, _)| name.to_string())
        .unwrap_or_else(|| s.to_string())
}

fn parse_backend_components(
    short: &str,
    full: Option<&String>,
) -> (String, String, Option<ToolVersionOptions>) {
    let short = unalias_backend(short).to_string();
    let source = full.unwrap_or(&short);
    let (source, opts) = match split_bracketed_opts(source) {
        Some((name, opts_str)) => (name, Some(parse_tool_options(opts_str))),
        None => (source.as_str(), None),
    };
    let (_backend, tool_name) = source.split_once(':').unwrap_or(("", source));
    let short = strip_opts(&short);

    (short, tool_name.to_string(), opts)
}

fn parse_backend_components_fallible(
    short: &str,
    full: Option<&String>,
) -> Result<(String, String, Option<ToolVersionOptions>)> {
    let short = unalias_backend(short).to_string();
    let source = full.unwrap_or(&short);
    let (source, opts) = match split_bracketed_opts(source) {
        Some((name, opts_str)) => (
            name,
            Some(try_parse_tool_options(opts_str).map_err(|err| eyre::eyre!(err))?),
        ),
        None => (source.as_str(), None),
    };
    let (_backend, tool_name) = source.split_once(':').unwrap_or(("", source));
    let short = strip_opts(&short);

    Ok((short, tool_name.to_string(), opts))
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

    /// Returns the kebab-cased directory name used for this tool's install path.
    /// This is the canonical name used on the filesystem (e.g. "github-user-repo").
    pub fn tool_dir_name(&self) -> String {
        self.installs_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
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
            // Check if the tool is in the registry but has no available backends
            if let Some(rt) = REGISTRY.get(self.short.as_str())
                && rt.backends().is_empty()
                && !rt.backends.is_empty()
            {
                let all_backends: Vec<&str> = rt.backends.iter().map(|rb| rb.full).collect();
                bail!(
                    "{self} is in the mise tool registry but none of its backends ({}) are supported in the current configuration",
                    all_backends.join(", ")
                );
            }

            let registry_shorts: Vec<&str> = REGISTRY.keys().collect();
            let mut suggestions: Vec<String> =
                xx::suggest::similar_n_with_threshold(&self.short, &registry_shorts, 3, 0.8)
                    .into_iter()
                    .filter(|s| *s != self.short)
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
        if let Some((backend, _)) = full.split_once(':')
            && let Ok(backend_type) = backend.parse()
        {
            return backend_type;
        }
        if config::is_loaded() && Config::get_().get_repo_url(&self.short).is_some() {
            return match install_state::get_plugin_type(&self.short).unwrap_or(PluginType::Asdf) {
                PluginType::Vfox => BackendType::Vfox,
                PluginType::VfoxBackend => BackendType::VfoxBackend(self.short.to_string()),
                PluginType::Asdf => BackendType::Asdf,
            };
        }
        BackendType::Unknown
    }

    pub fn full(&self) -> String {
        let short = unalias_backend(&self.short);

        // Check for environment variable override first
        // e.g., MISE_BACKENDS_MYTOOLS='github:myorg/mytools'
        if let Some(env_value) = self.env_backend_override() {
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
                return match install_state::get_plugin_type(short).unwrap_or(PluginType::Asdf) {
                    PluginType::Asdf => format!("asdf:{url}"),
                    PluginType::Vfox => format!("vfox:{short}"),
                    PluginType::VfoxBackend => short.to_string(),
                };
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
        if split_bracketed_opts(&full).is_some() {
            return full;
        }
        if let Some(opts) = &self.opts
            && let Some(opts_str) = serialize_tool_options(
                opts.opts
                    .iter()
                    .filter(|(k, _)| !EPHEMERAL_OPT_KEYS.contains(&k.as_str())),
            )
        {
            return format!("{full}[{opts_str}]");
        }
        full
    }

    pub fn full_without_opts(&self) -> String {
        let full = self.full();
        if let Some((name, _)) = split_bracketed_opts(&full) {
            return name.to_string();
        }
        full
    }

    pub fn opts(&self) -> ToolVersionOptions {
        self.opts_with_layers(self.backend_alias_opts_from_loaded_config(), None)
    }

    pub fn registry_opts(&self) -> ToolVersionOptions {
        let full = self.full_without_opts();
        REGISTRY
            .get(self.short.as_str())
            .map(|rt| rt.backend_options(&full))
            .unwrap_or_default()
    }

    pub fn opts_with_config(&self, config_opts: Option<ToolVersionOptions>) -> ToolVersionOptions {
        self.opts_with_layers(self.backend_alias_opts_from_loaded_config(), config_opts)
    }

    fn opts_with_layers(
        &self,
        alias_opts: Option<ToolVersionOptions>,
        config_opts: Option<ToolVersionOptions>,
    ) -> ToolVersionOptions {
        let mut opts = self.registry_opts();
        if alias_opts.is_none()
            && let Some(full_opts) = self.resolved_full_opts()
        {
            opts.apply_overrides(&full_opts);
        }
        if let Some(alias_opts) = alias_opts {
            opts.apply_overrides(&alias_opts);
        }
        if let Some(config_opts) = config_opts {
            opts.apply_overrides(&config_opts);
        }
        if let Some(user_opts) = self.explicit_opts() {
            opts.apply_overrides(user_opts);
        }
        opts
    }

    pub fn explicit_opts(&self) -> Option<&ToolVersionOptions> {
        self.opts.as_ref()
    }

    pub(crate) fn resolved_full_opts(&self) -> Option<ToolVersionOptions> {
        let full = self.full();
        split_bracketed_opts(&full).map(|(_, opts)| parse_tool_options(opts))
    }

    pub(crate) fn has_env_backend_override(&self) -> bool {
        self.env_backend_override().is_some()
    }

    fn env_backend_override(&self) -> Option<String> {
        let short = unalias_backend(&self.short);
        let env_key = format!("MISE_BACKENDS_{}", short.to_shouty_snake_case());
        env::var(&env_key).ok()
    }

    fn backend_alias_opts_from_loaded_config(&self) -> Option<ToolVersionOptions> {
        if !config::is_loaded() || self.has_env_backend_override() {
            return None;
        }
        let short = unalias_backend(&self.short);
        Config::get_()
            .all_aliases
            .get(short)
            .and_then(|alias| alias.backend.as_deref())
            .and_then(|backend| split_bracketed_opts(backend).map(|(_, opts)| opts))
            .map(parse_tool_options)
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
            if let Some((name, _)) = split_bracketed_opts(&full) {
                return name.to_string();
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
        if let Some((name, _)) = split_bracketed_opts(&full) {
            return name.to_string();
        }
        full
    }

    pub fn tool_name(&self) -> String {
        let full = self.full();
        let (_backend, tool_name) = full.split_once(':').unwrap_or(("", &full));
        strip_opts(tool_name)
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

impl FromStr for BackendArg {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self> {
        let short = unalias_backend(s).to_string();
        let explicit = if let Some((prefix, _)) = short.split_once(':') {
            BackendType::guess(prefix) != BackendType::Unknown
        } else {
            false
        };
        let (short_parsed, tool_name, opts) = parse_backend_components_fallible(&short, None)?;
        Ok(Self::new_raw(
            short_parsed,
            None,
            tool_name,
            opts,
            BackendResolution::new(explicit),
        ))
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
    use crate::config::Config;
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
    async fn test_bare_package_backend_names_are_not_implicit_tools() {
        let _config = Config::get().await.unwrap();

        for name in ["cargo", "gem"] {
            let fa: BackendArg = name.into();
            assert_str_eq!(name, fa.full());
            assert_eq!(BackendType::Unknown, fa.backend_type());
        }

        let fa: BackendArg = "cargo:ripgrep".into();
        assert_eq!(BackendType::Cargo, fa.backend_type());

        let fa: BackendArg = "gem:bashly".into();
        assert_eq!(BackendType::Gem, fa.backend_type());

        let fa: BackendArg = "npm".into();
        assert_str_eq!("npm:npm", fa.full());
        assert_eq!(BackendType::Npm, fa.backend_type());
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
        fa.set_opts(Some(parse_tool_options("a=1,postinstall=ignored,b=2")));
        // postinstall should be filtered out, remaining order preserved
        assert_str_eq!("npm:prettier[a=1,b=2]", fa.full_with_opts());

        fa = "http:hello-lock".into();
        fa.set_opts(Some(parse_tool_options("url=https://mise.en.dev/test-fixtures/hello-world-1.0.0.tar.gz,bin_path=hello-world-1.0.0/bin")));
        // install_env should be filtered out, remaining order preserved
        assert_str_eq!(
            "http:hello-lock[url=https://mise.en.dev/test-fixtures/hello-world-1.0.0.tar.gz,bin_path=hello-world-1.0.0/bin]",
            fa.full_with_opts()
        );
    }

    #[tokio::test]
    async fn test_full_with_opts_round_trips_comma_strings() {
        let _config = Config::get().await.unwrap();

        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "query".to_string(),
            toml::Value::String("first,second=value".to_string()),
        );

        let mut fa: BackendArg = "http:hello-lock".into();
        fa.set_opts(Some(opts));

        let serialized = fa.full_with_opts();
        assert_str_eq!(r#"http:hello-lock[query="first,second=value"]"#, serialized);

        let reparsed: BackendArg = serialized.as_str().into();
        let reparsed_opts = reparsed.opts();
        assert_eq!(reparsed_opts.get("query"), Some("first,second=value"));
        assert!(!reparsed_opts.contains_key("second"));
    }

    #[tokio::test]
    async fn test_full_with_opts_round_trips_strings_with_quotes_and_brackets() {
        let _config = Config::get().await.unwrap();

        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "pattern".to_string(),
            toml::Value::String(r#"a"b"#.to_string()),
        );
        opts.opts.insert(
            "bin_path".to_string(),
            toml::Value::String("bin[debug]".to_string()),
        );

        let mut fa: BackendArg = "http:hello-lock".into();
        fa.set_opts(Some(opts));

        let serialized = fa.full_with_opts();
        assert_str_eq!(
            r#"http:hello-lock[pattern='a"b',bin_path="bin[debug]"]"#,
            serialized
        );

        let reparsed: BackendArg = serialized.as_str().into();
        let reparsed_opts = reparsed.opts();
        assert_eq!(reparsed_opts.get("pattern"), Some(r#"a"b"#));
        assert_eq!(reparsed_opts.get("bin_path"), Some("bin[debug]"));
    }

    #[tokio::test]
    async fn test_split_bracketed_opts_ignores_quoted_brackets() {
        let _config = Config::get().await.unwrap();

        assert_eq!(
            split_bracketed_opts(r#"http:hello-lock[pattern='a"b',bin_path="bin[debug]"]"#),
            Some(("http:hello-lock", r#"pattern='a"b',bin_path="bin[debug]""#))
        );
        assert_str_eq!(
            "http:hello-lock",
            strip_opts(r#"http:hello-lock[pattern='a"b',bin_path="bin[debug]"]"#)
        );
    }

    #[tokio::test]
    async fn test_full_with_opts_omits_empty_brackets_for_complex_opts() {
        let _config = Config::get().await.unwrap();

        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "targets".to_string(),
            toml::Value::Array(vec![toml::Value::String("x86_64".to_string())]),
        );

        let mut fa: BackendArg = "npm:prettier".into();
        fa.set_opts(Some(opts));

        assert_str_eq!("npm:prettier", fa.full_with_opts());
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

    #[tokio::test]
    async fn test_parse_backend_opts_with_url_value_on_shorthand() {
        let _config = Config::get().await.unwrap();
        let ba: BackendArg = "tiny[api_url=https://inline.example/api/v3]".into();

        assert_eq!(ba.short, "tiny");
        assert_eq!(ba.tool_name, "tiny");
        assert_eq!(
            ba.opts().get("api_url"),
            Some("https://inline.example/api/v3")
        );
    }

    #[tokio::test]
    async fn test_parse_backend_opts_core_fields() {
        let _config = Config::get().await.unwrap();
        let ba: BackendArg =
            "pipx:ruff[depends=python,os=linux,install_env.PIPX_HOME=/tmp/pipx]".into();
        let opts = ba.opts();

        assert_eq!(opts.depends, Some(vec!["python".to_string()]));
        assert_eq!(opts.os, Some(vec!["linux".to_string()]));
        assert_eq!(
            opts.install_env.get("PIPX_HOME").map(String::as_str),
            Some("/tmp/pipx")
        );
        assert!(!opts.opts.contains_key("depends"));
        assert!(!opts.opts.contains_key("os"));
        assert!(!opts.opts.contains_key("install_env.PIPX_HOME"));
    }

    #[test]
    fn test_parse_backend_opts_rejects_invalid_core_fields() {
        let err = "pipx:ruff[depends={ name = \"python\" }]"
            .parse::<BackendArg>()
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("depends must be a string or array")
        );
    }

    #[tokio::test]
    async fn test_opts_with_config_overlays_registry_config_and_inline() {
        let _config = Config::get().await.unwrap();
        let ba: BackendArg = "graphite[exe=inline,foo=inline]".into();
        let config_opts = parse_tool_options("exe=config,bar=config");

        let opts = ba.opts_with_config(Some(config_opts));

        assert_eq!(ba.registry_opts().get("exe"), Some("gt"));
        assert_eq!(opts.get("exe"), Some("inline"));
        assert_eq!(opts.get("bar"), Some("config"));
        assert_eq!(opts.get("foo"), Some("inline"));
    }

    #[tokio::test]
    async fn test_opts_with_layers_preserves_alias_options() {
        let _config = Config::get().await.unwrap();
        let ba: BackendArg = "graphite[exe=inline,foo=inline]".into();
        let alias_opts = parse_tool_options("exe=alias,alias_only=alias");
        let config_opts = parse_tool_options("exe=config,config_only=config");

        let opts = ba.opts_with_layers(Some(alias_opts), Some(config_opts));

        assert_eq!(ba.registry_opts().get("exe"), Some("gt"));
        assert_eq!(opts.get("exe"), Some("inline"));
        assert_eq!(opts.get("alias_only"), Some("alias"));
        assert_eq!(opts.get("config_only"), Some("config"));
        assert_eq!(opts.get("foo"), Some("inline"));
    }

    #[test]
    fn test_opts_include_resolved_full_bracket_options() {
        let ba = BackendArg::new_raw(
            "graphite".to_string(),
            Some("github:withgraphite/homebrew-tap[foo=resolved]".to_string()),
            "withgraphite/homebrew-tap".to_string(),
            None,
            BackendResolution::new(true),
        );

        let opts = ba.opts();

        assert_eq!(ba.registry_opts().get("exe"), Some("gt"));
        assert_eq!(opts.get("exe"), Some("gt"));
        assert_eq!(opts.get("foo"), Some("resolved"));
    }
}
