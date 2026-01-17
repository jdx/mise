use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::config::{Config, ConfigMap, Settings};
use crate::toolset::Toolset;
use crate::{dirs, env, file};

/// Cached environment data
#[derive(Debug, Serialize, Deserialize)]
pub struct CachedEnv {
    pub paths: Vec<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub created_at: u64,
    /// Files referenced by _.file directives (path, mtime_nanos)
    /// Used to invalidate cache when .env files change
    #[serde(default)]
    pub referenced_files: Vec<(PathBuf, u128)>,
}

impl CachedEnv {
    /// Load cached environment from file
    pub fn load(key: &str) -> Option<Self> {
        let path = cache_file(key);
        let content = std::fs::read(&path).ok()?;
        rmp_serde::from_slice(&content).ok()
    }

    /// Save cached environment to file
    pub fn save(&self, key: &str) -> Result<()> {
        file::create_dir_all(cache_dir())?;
        let content = rmp_serde::to_vec(self)?;
        std::fs::write(cache_file(key), content)?;
        Ok(())
    }

    /// Check if cache is still valid based on TTL and referenced file mtimes
    pub fn is_valid(&self) -> bool {
        // Check TTL
        let ttl = Settings::get().env_cache_ttl_duration();
        let now = unix_timestamp();
        if now.saturating_sub(self.created_at) >= ttl.as_secs() {
            return false;
        }

        // Check that all referenced files (from _.file directives) haven't changed
        for (path, cached_mtime) in &self.referenced_files {
            match path.metadata() {
                Ok(meta) => {
                    if let Ok(mtime) = meta.modified() {
                        let current_mtime = mtime
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos();
                        if current_mtime != *cached_mtime {
                            trace!("env cache invalid: {} mtime changed", path.display());
                            return false;
                        }
                    }
                }
                Err(_) => {
                    // File no longer exists or can't be accessed
                    trace!("env cache invalid: {} no longer accessible", path.display());
                    return false;
                }
            }
        }

        true
    }
}

/// Compute cache key from config and toolset
pub fn compute_cache_key(config: &Arc<Config>, toolset: &Toolset) -> String {
    let mut hasher = DefaultHasher::new();

    // Hash project root
    config.project_root.hash(&mut hasher);

    // Hash config files and their modification times
    hash_config_files(&config.config_files, &mut hasher);

    // Hash relevant settings
    hash_relevant_settings(&mut hasher);

    // Hash resolved tool versions AND installation state
    // This ensures cache invalidation when tools are installed/uninstalled
    hash_tool_requests(config, toolset, &mut hasher);

    // Hash mise version
    std::env!("CARGO_PKG_VERSION").hash(&mut hasher);

    // Hash the user's base PATH from PRISTINE_ENV
    // This ensures cache invalidation when user modifies their shell PATH
    // (e.g., adds /custom/bin to .bashrc and opens a new terminal)
    env::PRISTINE_ENV
        .get(&*env::PATH_KEY)
        .map(|p| p.as_str())
        .unwrap_or("")
        .hash(&mut hasher);

    format!("{:x}", hasher.finish())
}

fn cache_dir() -> PathBuf {
    dirs::STATE.join("env-cache")
}

fn cache_file(key: &str) -> PathBuf {
    cache_dir().join(key)
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hash_config_files(config_files: &ConfigMap, hasher: &mut DefaultHasher) {
    for (path, _) in config_files {
        path.hash(hasher);
        if let Ok(meta) = path.metadata()
            && let Ok(mtime) = meta.modified()
        {
            // Use nanoseconds for more precise cache invalidation
            // (seconds would miss rapid file changes in tests)
            mtime
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(hasher);
        }
    }
}

fn hash_relevant_settings(hasher: &mut DefaultHasher) {
    let settings = Settings::get();
    settings.experimental.hash(hasher);
    settings.all_compile.hash(hasher);
    settings.node.compile.hash(hasher);
    settings.python.compile.hash(hasher);
    settings.ruby.compile.hash(hasher);
    // Include no_env flag state (checks --no-env flag and MISE_NO_ENV env var)
    Settings::no_env().hash(hasher);
    // Include env-related environment variables that affect config loading
    std::env::var("MISE_ENV_FILE").ok().hash(hasher);
    std::env::var("MISE_ENV").ok().hash(hasher);
}

fn hash_tool_requests(config: &Arc<Config>, toolset: &Toolset, hasher: &mut DefaultHasher) {
    // Hash requested tool versions
    for (backend, tvl) in &toolset.versions {
        backend.short.hash(hasher);
        for tv in &tvl.versions {
            tv.version.hash(hasher);
        }
    }

    // Hash which requested versions are actually installed
    // This ensures cache invalidation when tools are installed/uninstalled
    for (backend, tv) in toolset.list_current_installed_versions(config) {
        backend.id().hash(hasher);
        tv.version.hash(hasher);
        true.hash(hasher); // installed marker
    }
}
