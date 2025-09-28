use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::config::Settings;
use heck::ToShoutySnakeCase;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::env::consts::{ARCH, OS};
use std::fmt::Display;
use std::iter::Iterator;
use std::sync::{LazyLock as Lazy, Mutex};
use strum::IntoEnumIterator;
use url::Url;

// the registry is generated from registry.toml in the project root
pub static REGISTRY: Lazy<BTreeMap<&'static str, RegistryTool>> =
    Lazy::new(|| include!(concat!(env!("OUT_DIR"), "/registry.rs")));

#[derive(Debug, Clone)]
pub struct RegistryTool {
    pub short: &'static str,
    pub description: Option<&'static str>,
    pub backends: &'static [RegistryBackend],
    #[allow(unused)]
    pub aliases: &'static [&'static str],
    pub test: &'static Option<(&'static str, &'static str)>,
    pub os: &'static [&'static str],
    pub depends: &'static [&'static str],
    pub idiomatic_files: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct RegistryBackend {
    pub full: &'static str,
    pub platforms: &'static [&'static str],
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
        let os = Settings::get().os.clone().unwrap_or(OS.to_string());
        let arch = Settings::get().arch.clone().unwrap_or(ARCH.to_string());
        let platform = format!("{os}-{arch}");
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

fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url.host_str().unwrap();
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

fn url_like(s: &str) -> bool {
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

pub fn tool_enabled<T: Ord>(
    enable_tools: &BTreeSet<T>,
    disable_tools: &BTreeSet<T>,
    name: &T,
) -> bool {
    if enable_tools.is_empty() {
        !disable_tools.contains(name)
    } else {
        enable_tools.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[tokio::test]
    async fn test_tool_disabled() {
        let _config = Config::get().await.unwrap();
        use super::*;
        let name = "cargo";

        assert!(tool_enabled(&BTreeSet::new(), &BTreeSet::new(), &name));
        assert!(tool_enabled(
            &BTreeSet::from(["cargo"]),
            &BTreeSet::new(),
            &name
        ));
        assert!(!tool_enabled(
            &BTreeSet::new(),
            &BTreeSet::from(["cargo"]),
            &name
        ));
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
}
