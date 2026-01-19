use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use eyre::Result;

use crate::config::env_directive::{EnvResolveOptions, EnvResults, ToolsFilter};
use crate::config::{Config, Settings};
use crate::env::{PATH_KEY, WARN_ON_MISSING_REQUIRED_ENV};
use crate::env_diff::EnvMap;
use crate::path_env::PathEnv;
use crate::toolset::Toolset;
use crate::toolset::env_cache::{CachedEnv, compute_settings_hash, get_file_mtime};
use crate::toolset::tool_request::ToolRequest;
use crate::{env, parallel, uv};

impl Toolset {
    pub async fn full_env(&self, config: &Arc<Config>) -> Result<EnvMap> {
        let mut env = env::PRISTINE_ENV.clone().into_iter().collect::<EnvMap>();
        env.extend(self.env_with_path(config).await?.clone());
        Ok(env)
    }

    /// the full mise environment including all tool paths
    pub async fn env_with_path(&self, config: &Arc<Config>) -> Result<EnvMap> {
        // Try to load from cache if enabled
        if CachedEnv::is_enabled() {
            if let Some(cached) = self.try_load_env_cache(config)? {
                trace!("env_cache: using cached environment");
                return Ok(cached);
            }
        }

        let (mut env, env_results) = self.final_env(config).await?;
        let mut path_env = PathEnv::from_iter(env::PATH.clone());
        let paths = self.list_final_paths(config, env_results.clone()).await?;
        for p in &paths {
            path_env.add(p.clone());
        }
        env.insert(PATH_KEY.to_string(), path_env.to_string());

        // Save to cache if enabled and no uncacheable directives
        if CachedEnv::is_enabled() && !env_results.has_uncacheable {
            if let Err(e) = self.save_env_cache(config, &env, &paths, &env_results) {
                debug!("env_cache: failed to save: {}", e);
            }
        }

        Ok(env)
    }

    /// Try to load environment from cache
    fn try_load_env_cache(&self, config: &Arc<Config>) -> Result<Option<EnvMap>> {
        let cache_key = self.compute_env_cache_key(config)?;
        match CachedEnv::load(&cache_key)? {
            Some(cached) => {
                let mut env = cached.env;
                // Reconstruct PATH from cached paths
                let mut path_env = PathEnv::from_iter(env::PATH.clone());
                for p in cached.paths {
                    path_env.add(p);
                }
                env.insert(PATH_KEY.to_string(), path_env.to_string());
                Ok(Some(env))
            }
            None => Ok(None),
        }
    }

    /// Save environment to cache
    fn save_env_cache(
        &self,
        config: &Arc<Config>,
        env: &EnvMap,
        paths: &[PathBuf],
        env_results: &EnvResults,
    ) -> Result<()> {
        let cache_key = self.compute_env_cache_key(config)?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Collect all files to watch (config files + module watch_files + env_files)
        let mut watch_files: Vec<PathBuf> = config.config_files.keys().cloned().collect();
        watch_files.extend(env_results.watch_files.clone());
        watch_files.extend(env_results.env_files.clone());
        watch_files.extend(env_results.env_scripts.clone());

        // Get mtimes for watch files
        let watch_file_mtimes: Vec<u64> = watch_files
            .iter()
            .map(|p| get_file_mtime(p).unwrap_or(0))
            .collect();

        // Remove PATH from env before caching (we store paths separately)
        let env_without_path: BTreeMap<String, String> = env
            .iter()
            .filter(|(k, _)| k.as_str() != PATH_KEY.as_str())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let cached = CachedEnv {
            env: env_without_path,
            paths: paths.to_vec(),
            created_at: now,
            watch_files,
            watch_file_mtimes,
            mise_version: env!("CARGO_PKG_VERSION").to_string(),
            cache_key_debug: cache_key.clone(),
        };

        cached.save(&cache_key)
    }

    /// Compute the cache key for the current configuration
    fn compute_env_cache_key(&self, config: &Arc<Config>) -> Result<String> {
        // Collect config files with their mtimes
        let config_files: Vec<(PathBuf, u64)> = config
            .config_files
            .keys()
            .map(|p| (p.clone(), get_file_mtime(p).unwrap_or(0)))
            .collect();

        // Collect tool versions
        let tool_versions: Vec<(String, String)> = self
            .list_current_versions()
            .into_iter()
            .map(|(b, tv)| (b.id().to_string(), tv.version.clone()))
            .collect();

        // Get settings hash
        let settings_hash = compute_settings_hash();

        // Get base PATH
        let base_path = env::PATH
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join(":");

        Ok(CachedEnv::compute_cache_key(
            &config_files,
            &tool_versions,
            &settings_hash,
            &base_path,
        ))
    }

    pub async fn env_from_tools(&self, config: &Arc<Config>) -> Vec<(String, String, String)> {
        let this = Arc::new(self.clone());
        let items: Vec<_> = self
            .list_current_installed_versions(config)
            .into_iter()
            .filter(|(_, tv)| !matches!(tv.request, ToolRequest::System { .. }))
            .map(|(b, tv)| (config.clone(), this.clone(), b, tv))
            .collect();

        let envs = parallel::parallel(items, |(config, this, b, tv)| async move {
            let backend_id = b.id().to_string();
            match b.exec_env(&config, &this, &tv).await {
                Ok(env) => Ok(env
                    .into_iter()
                    .map(|(k, v)| (k, v, backend_id.clone()))
                    .collect::<Vec<_>>()),
                Err(e) => {
                    warn!("Error running exec-env: {:#}", e);
                    Ok(Vec::new())
                }
            }
        })
        .await
        .unwrap_or_default();

        envs.into_iter()
            .flatten()
            .filter(|(k, _, _)| k.to_uppercase() != "PATH")
            .collect()
    }

    pub(super) async fn env(&self, config: &Arc<Config>) -> Result<(EnvMap, Vec<PathBuf>)> {
        time!("env start");
        let entries = self
            .env_from_tools(config)
            .await
            .into_iter()
            .map(|(k, v, _)| (k, v))
            .collect::<Vec<(String, String)>>();

        // Collect and process MISE_ADD_PATH values into paths
        let paths_to_add: Vec<PathBuf> = entries
            .iter()
            .filter(|(k, _)| k == "MISE_ADD_PATH" || k == "RTX_ADD_PATH")
            .flat_map(|(_, v)| env::split_paths(v))
            .collect();

        let mut env: EnvMap = entries
            .into_iter()
            .filter(|(k, _)| k != "RTX_ADD_PATH")
            .filter(|(k, _)| k != "MISE_ADD_PATH")
            .filter(|(k, _)| !k.starts_with("RTX_TOOL_OPTS__"))
            .filter(|(k, _)| !k.starts_with("MISE_TOOL_OPTS__"))
            .rev()
            .collect();

        env.extend(config.env().await?.clone());
        if let Some(venv) = uv::uv_venv(config, self).await {
            for (k, v) in venv.env.clone() {
                env.insert(k, v);
            }
        }
        time!("env end");
        Ok((env, paths_to_add))
    }

    pub async fn final_env(&self, config: &Arc<Config>) -> Result<(EnvMap, EnvResults)> {
        let (mut env, add_paths) = self.env(config).await?;
        let mut tera_env = env::PRISTINE_ENV.clone().into_iter().collect::<EnvMap>();
        tera_env.extend(env.clone());
        let mut path_env = PathEnv::from_iter(env::PATH.clone());

        for p in config.path_dirs().await?.clone() {
            path_env.add(p);
        }
        for p in &add_paths {
            path_env.add(p.clone());
        }
        for p in self.list_paths(config).await {
            path_env.add(p);
        }
        tera_env.insert(PATH_KEY.to_string(), path_env.to_string());
        let mut ctx = config.tera_ctx.clone();
        ctx.insert("env", &tera_env);
        let mut env_results = self.load_post_env(config, ctx, &tera_env).await?;

        // Store add_paths separately to maintain consistent PATH ordering
        env_results.tool_add_paths = add_paths;

        env.extend(
            env_results
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.0.clone())),
        );
        Ok((env, env_results))
    }

    pub(super) async fn load_post_env(
        &self,
        config: &Arc<Config>,
        ctx: tera::Context,
        env: &EnvMap,
    ) -> Result<EnvResults> {
        if Settings::no_env() || Settings::get().no_env.unwrap_or(false) {
            return Ok(EnvResults::default());
        }
        let entries = config
            .config_files
            .iter()
            .rev()
            .map(|(source, cf)| {
                cf.env_entries()
                    .map(|ee| ee.into_iter().map(|e| (e, source.clone())))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        // trace!("load_env: entries: {:#?}", entries);
        let env_results = EnvResults::resolve(
            config,
            ctx,
            env,
            entries,
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::ToolsOnly,
                warn_on_missing_required: *WARN_ON_MISSING_REQUIRED_ENV,
            },
        )
        .await?;
        if log::log_enabled!(log::Level::Trace) {
            trace!("{env_results:#?}");
        } else if !env_results.is_empty() {
            debug!("{env_results:?}");
        }
        Ok(env_results)
    }
}
