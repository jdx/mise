use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use eyre::Result;
use std::sync::LazyLock as Lazy;

use crate::config::Config;
use crate::config::env_directive::EnvResults;
use crate::toolset::Toolset;
use crate::uv;
use itertools::Itertools;

// Cache Toolset::list_paths results across identical toolsets within a process.
// Keyed by project_root plus sorted list of backend@version pairs currently installed.
pub(super) static LIST_PATHS_CACHE: Lazy<DashMap<String, Vec<PathBuf>>> = Lazy::new(DashMap::new);

impl Toolset {
    pub async fn list_paths(&self, config: &Arc<Config>) -> Vec<PathBuf> {
        // Build a stable cache key based on project_root and current installed versions
        let mut key_parts = vec![];
        if let Some(root) = &config.project_root {
            key_parts.push(root.to_string_lossy().to_string());
        }
        let mut installed = self.list_current_installed_versions(config);

        let installed_strs: Vec<String> = installed
            .iter()
            .map(|(p, tv)| format!("{}@{}", p.id(), tv.version))
            .sorted()
            .collect();
        key_parts.extend(installed_strs);

        let cache_key = key_parts.join("|");
        if let Some(entry) = LIST_PATHS_CACHE.get(&cache_key) {
            trace!("toolset.list_paths hit cache");
            return entry.clone();
        }

        Self::sort_by_overrides(&mut installed).unwrap();

        let mut paths: Vec<PathBuf> = Vec::new();
        for (p, tv) in installed {
            let start = std::time::Instant::now();
            let new_paths = p.list_bin_paths(config, &tv).await.unwrap_or_else(|e| {
                warn!("Error listing bin paths for {tv}: {e:#}");
                Vec::new()
            });
            trace!(
                "toolset.list_paths {}@{} list_bin_paths took {}ms",
                p.id(),
                tv.version,
                start.elapsed().as_millis()
            );
            paths.extend(new_paths);
        }
        LIST_PATHS_CACHE.insert(cache_key, paths.clone());
        paths
            .into_iter()
            .filter(|p| p.parent().is_some()) // TODO: why?
            .collect()
    }

    /// same as list_paths but includes config.list_paths, venv paths, and MISE_ADD_PATHs from self.env()
    pub async fn list_final_paths(
        &self,
        config: &Arc<Config>,
        env_results: EnvResults,
    ) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();

        // Match the tera_env PATH ordering from final_env():
        // 1. Original system PATH is handled by PathEnv::from_iter() in env_with_path()

        // 2. Config path dirs
        paths.extend(config.path_dirs().await?.clone());

        // 3. UV venv path (if any) - ensure project venv takes precedence over tool and tool_add_paths
        if let Some(venv) = uv::uv_venv(config, self).await {
            paths.push(venv.venv_path.clone());
        }

        // 4. tool_add_paths (MISE_ADD_PATH/RTX_ADD_PATH from tools)
        paths.extend(env_results.tool_add_paths);

        // 5. Tool paths
        paths.extend(self.list_paths(config).await);

        // 6. env_results.env_paths (from load_post_env like _.path directives) - these go at the front
        let paths = env_results.env_paths.into_iter().chain(paths).collect();
        Ok(paths)
    }

    /// Returns paths separated by their source: (user_configured_paths, tool_paths)
    /// User-configured paths should never be filtered, while tool paths should be filtered
    /// if they duplicate entries in the original PATH.
    pub async fn list_final_paths_split(
        &self,
        config: &Arc<Config>,
        env_results: EnvResults,
    ) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
        // User-configured paths from env._.path directives
        // IMPORTANT: There are TWO sources of env paths:
        // 1. config.path_dirs() - from config.env_results() (cached, no tera context)
        // 2. env_results.env_paths - from ts.final_env() (fresh, with tera context applied)
        // env_results.env_paths must come FIRST for highest precedence
        let mut user_paths = env_results.env_paths;
        user_paths.extend(config.path_dirs().await?.clone());

        // Tool paths start empty
        let mut tool_paths = Vec::new();

        // UV venv path (if any) - these are tool-managed paths
        if let Some(venv) = uv::uv_venv(config, self).await {
            tool_paths.push(venv.venv_path.clone());
        }

        // tool_add_paths (MISE_ADD_PATH/RTX_ADD_PATH from tools)
        tool_paths.extend(env_results.tool_add_paths);

        // Tool installation paths
        tool_paths.extend(self.list_paths(config).await);

        Ok((user_paths, tool_paths))
    }
}
