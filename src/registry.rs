use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::config::Settings;
use crate::toolset::ToolVersionOptions;
use heck::ToShoutySnakeCase;
use indexmap::IndexMap;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::env::consts::{ARCH, OS};
use std::fmt::Display;
use std::iter::Iterator;
use std::sync::{LazyLock as Lazy, Mutex};
use strum::IntoEnumIterator;
use url::Url;

// the registry is generated from registry/ in the project root
pub static REGISTRY: Registry = include!(concat!(env!("OUT_DIR"), "/registry.rs"));

pub struct Registry {
    entries: &'static [(&'static str, RegistryTool)],
    lookup: phf::Map<&'static str, usize>,
}

impl Registry {
    pub fn get(&self, name: &str) -> Option<&'static RegistryTool> {
        self.lookup.get(name).map(|index| &self.entries[*index].1)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.lookup.contains_key(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &'static RegistryTool)> {
        self.entries.iter().map(|(name, tool)| (*name, tool))
    }

    pub fn keys(&self) -> impl Iterator<Item = &'static str> {
        self.entries.iter().map(|(name, _)| *name)
    }

    pub fn values(&self) -> impl Iterator<Item = &'static RegistryTool> {
        self.entries.iter().map(|(_, tool)| tool)
    }
}

#[derive(Debug, Clone)]
pub struct RegistryTool {
    pub short: &'static str,
    pub description: Option<&'static str>,
    pub backends: &'static [RegistryBackend],
    #[allow(unused)]
    pub aliases: &'static [&'static str],
    pub overrides: &'static [&'static str],
    pub test: &'static Option<RegistryToolTest>,
    pub os: &'static [&'static str],
    pub idiomatic_files: &'static [&'static str],
    pub detect: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct RegistryToolTest {
    pub cmd: &'static str,
    pub expected: &'static str,
    pub tools: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct RegistryBackend {
    pub full: &'static str,
    pub platforms: &'static [&'static str],
    pub options: &'static [(&'static str, &'static str)],
}

// Cache for environment variable overrides
static ENV_BACKENDS: Lazy<Mutex<HashMap<String, &'static str>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

impl RegistryTool {
    pub fn backends(&self) -> Vec<&'static str> {
        // Check for environment variable override first
        // e.g., MISE_BACKENDS_GRAPHITE='github:withgraphite/homebrew-tap[exe=gt]'
        let env_key = format!("MISE_BACKENDS_{}", self.short.to_shouty_snake_case());

        // Check cache first
        {
            let cache = ENV_BACKENDS.lock().unwrap();
            if let Some(&backend) = cache.get(&env_key) {
                return vec![backend];
            }
        }

        // Check environment variable
        if let Ok(env_value) = env::var(&env_key) {
            // Store in cache with 'static lifetime
            let leaked = Box::leak(env_value.into_boxed_str());
            let mut cache = ENV_BACKENDS.lock().unwrap();
            cache.insert(env_key.clone(), leaked);
            return vec![leaked];
        }

        static BACKEND_TYPES: Lazy<HashSet<String>> = Lazy::new(|| {
            let mut backend_types = BackendType::iter()
                .map(|b| b.to_string())
                .collect::<HashSet<_>>();
            time!("disable_backends");
            for backend in &Settings::get().disable_backends {
                backend_types.remove(backend);
            }
            time!("disable_backends");
            if cfg!(windows) {
                backend_types.remove("asdf");
            }
            backend_types
        });
        let settings = Settings::get();
        let os = settings.os.clone().unwrap_or(OS.to_string());
        let arch = settings.arch.clone().unwrap_or(ARCH.to_string());
        let platform = format!("{os}-{arch}");
        let experimental = settings.experimental;
        self.backends
            .iter()
            .filter(|rb| {
                rb.platforms.is_empty()
                    || rb.platforms.contains(&&*os)
                    || rb.platforms.contains(&&*arch)
                    || rb.platforms.contains(&&*platform)
            })
            .map(|rb| rb.full)
            .filter(|full| {
                full.split(':')
                    .next()
                    .is_some_and(|b| BACKEND_TYPES.contains(b))
            })
            // Filter out experimental backends if experimental mode is disabled
            .filter(|full| {
                if experimental {
                    return true;
                }
                let backend_type = BackendType::guess(full);
                !backend_type.is_experimental()
            })
            .collect()
    }

    pub fn is_supported_os(&self) -> bool {
        self.os.is_empty() || self.os.contains(&OS)
    }

    pub fn ba(&self) -> Option<BackendArg> {
        self.backends()
            .first()
            .map(|f| BackendArg::new(self.short.to_string(), Some(f.to_string())))
    }

    /// Get RegistryBackend for a specific full backend string
    pub fn get_backend(&self, full: &str) -> Option<&RegistryBackend> {
        self.backends.iter().find(|rb| rb.full == full)
    }

    /// Get options for a specific backend
    pub fn backend_options(&self, full: &str) -> ToolVersionOptions {
        let mut opts = IndexMap::new();

        if let Some(backend) = self.get_backend(full) {
            for (k, v) in backend.options {
                let value = v.parse::<toml::Value>().unwrap_or_else(|e| {
                    panic!("failed to parse registry option {k} as a TOML value: {e}")
                });
                opts.insert(k.to_string(), value);
            }
        }

        ToolVersionOptions {
            opts,
            ..Default::default()
        }
    }
}

pub fn shorts_for_full(full: &str) -> &'static Vec<&'static str> {
    static EMPTY: Vec<&'static str> = vec![];
    static FULL_TO_SHORT: Lazy<HashMap<&'static str, Vec<&'static str>>> = Lazy::new(|| {
        let mut map: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
        for (short, rt) in REGISTRY.iter() {
            for full in rt.backends() {
                map.entry(full).or_default().push(short);
            }
        }
        map
    });
    FULL_TO_SHORT.get(full).unwrap_or(&EMPTY)
}

pub fn is_trusted_plugin(name: &str, remote: &str) -> bool {
    let normalized_url = normalize_remote(remote).unwrap_or("INVALID_URL".into());
    let is_shorthand = REGISTRY
        .get(name)
        .and_then(|tool| tool.backends().first().copied())
        .map(full_to_url)
        .is_some_and(|s| normalize_remote(&s).unwrap_or_default() == normalized_url);
    let is_mise_url = normalized_url.starts_with("github.com/mise-plugins/");

    !is_shorthand || is_mise_url
}

pub fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url
        .host_str()
        .ok_or_else(|| eyre::eyre!("URL has no host: {remote}"))?;
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}

pub fn full_to_url(full: &str) -> String {
    if url_like(full) {
        return full.to_string();
    }
    let (_backend, url) = full.split_once(':').unwrap_or(("", full));
    if url_like(url) {
        url.to_string()
    } else {
        format!("https://github.com/{url}.git")
    }
}

pub(crate) fn url_like(s: &str) -> bool {
    s.starts_with("https://")
        || s.starts_with("http://")
        || s.starts_with("git@")
        || s.starts_with("ssh://")
        || s.starts_with("git://")
}

impl Display for RegistryTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short)
    }
}

/// Returns true when `name` passes the configured tool filter.
///
/// `None` means no allowlist is configured, so `disable_tools` excludes
/// individual tools. `Some(empty)` is an explicit empty allowlist and disables
/// every tool. When an allowlist is configured, it is authoritative and
/// `disable_tools` is not applied.
pub fn tool_enabled<T: Ord>(
    enable_tools: Option<&BTreeSet<T>>,
    disable_tools: &BTreeSet<T>,
    name: &T,
) -> bool {
    match enable_tools {
        Some(enable_tools) => enable_tools.contains(name),
        None => !disable_tools.contains(name),
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn test_tool_disabled() {
        use super::*;
        let name = "cargo";

        assert!(tool_enabled(None, &BTreeSet::new(), &name));
        assert!(!tool_enabled(
            Some(&BTreeSet::new()),
            &BTreeSet::new(),
            &name
        ));
        assert!(tool_enabled(
            Some(&BTreeSet::from(["cargo"])),
            &BTreeSet::new(),
            &name
        ));
        assert!(!tool_enabled(None, &BTreeSet::from(["cargo"]), &name));
        assert!(tool_enabled(
            Some(&BTreeSet::from(["cargo"])),
            &BTreeSet::from(["cargo"]),
            &name
        ));
    }

    #[test]
    fn test_registry_iteration_is_sorted() {
        use super::*;

        // The interactive tool selector and --all test-tool path consume registry
        // iteration order directly, so keep PHF lookup separate from sorted output.
        let keys = REGISTRY.keys().collect::<Vec<_>>();
        let mut sorted = keys.clone();
        sorted.sort_unstable();

        assert!(!keys.is_empty());
        assert_eq!(keys, sorted);
    }

    #[test]
    fn test_backend_options_parse_toml_values() {
        use super::*;

        static OPTIONS: &[(&str, &str)] = &[
            ("bin", r#""rg""#),
            ("prerelease", "true"),
            ("strip_components", "1"),
            (
                "targets",
                r#"["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]"#,
            ),
            (
                "platforms",
                r#"{ linux-x64 = { asset_pattern = "tool-linux.tar.gz" } }"#,
            ),
        ];
        static BACKENDS: &[RegistryBackend] = &[RegistryBackend {
            full: "github:owner/repo",
            platforms: &[],
            options: OPTIONS,
        }];
        let tool = RegistryTool {
            short: "test",
            description: None,
            backends: BACKENDS,
            aliases: &[],
            overrides: &[],
            test: &None,
            os: &[],
            idiomatic_files: &[],
            detect: &[],
        };

        let opts = tool.backend_options("github:owner/repo");

        assert_eq!(opts.get("bin"), Some("rg"));
        assert_eq!(
            opts.opts.get("prerelease"),
            Some(&toml::Value::Boolean(true))
        );
        assert_eq!(
            opts.opts.get("strip_components"),
            Some(&toml::Value::Integer(1))
        );
        assert!(opts.opts.get("targets").is_some_and(toml::Value::is_array));
        assert_eq!(
            opts.get_nested_string("platforms.linux-x64.asset_pattern"),
            Some("tool-linux.tar.gz".to_string())
        );
    }

    #[tokio::test]
    async fn test_backend_env_override() {
        let _config = Config::get().await.unwrap();
        use super::*;

        // Clear the cache first
        ENV_BACKENDS.lock().unwrap().clear();

        // Test with a known tool from the registry
        if let Some(tool) = REGISTRY.get("node") {
            // First test without env var - should return default backends
            let default_backends = tool.backends();
            assert!(!default_backends.is_empty());

            // Test with env var override
            // SAFETY: This is safe in a test environment
            unsafe {
                env::set_var("MISE_BACKENDS_NODE", "test:backend");
            }
            let overridden_backends = tool.backends();
            assert_eq!(overridden_backends.len(), 1);
            assert_eq!(overridden_backends[0], "test:backend");

            // Clean up
            // SAFETY: This is safe in a test environment
            unsafe {
                env::remove_var("MISE_BACKENDS_NODE");
            }
            ENV_BACKENDS.lock().unwrap().clear();
        }
    }

    #[test]
    fn test_normalize_remote() {
        use super::*;

        // Standard HTTPS URLs should work
        let result = normalize_remote("https://github.com/mise-plugins/vfox-node.git");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "github.com/mise-plugins/vfox-node");

        // file:// URLs should return an error (no host)
        let result = normalize_remote("file:///path/to/repo");
        assert!(result.is_err());

        // Invalid URLs should return an error
        let result = normalize_remote("not-a-url");
        assert!(result.is_err());
    }
}
