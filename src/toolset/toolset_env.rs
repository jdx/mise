use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::Result;

use crate::config::env_directive::{EnvResolveOptions, EnvResults, ToolsFilter};
use crate::config::{Config, Settings};
use crate::env::{PATH_KEY, WARN_ON_MISSING_REQUIRED_ENV};
use crate::env_diff::EnvMap;
use crate::path_env::PathEnv;
use crate::toolset::Toolset;
use crate::toolset::env_cache::{CachedEnv, compute_cache_key};
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
        // Fast path: check if cached environment is available and valid
        // Skip cache if __MISE_FRESH_ENV is set (via --fresh-env flag)
        // Only enabled when experimental mode is on
        let settings = Settings::get();
        let fresh_env = std::env::var("__MISE_FRESH_ENV").is_ok();
        if settings.experimental && settings.env_cache && !fresh_env {
            let current_key = compute_cache_key(config, self);

            // First check if parent process provided a matching cache key
            if let Ok(parent_key) = std::env::var("__MISE_ENV_CACHE_KEY")
                && parent_key == current_key
                && let Some(cached) = CachedEnv::load(&current_key)
                && cached.is_valid()
            {
                trace!("using cached environment from parent");
                return Ok(cached.env);
            }

            // Check if we have a valid cache for this context
            if let Some(cached) = CachedEnv::load(&current_key)
                && cached.is_valid()
            {
                trace!("using cached environment from file");
                return Ok(cached.env);
            }
        }

        let (mut env, env_results) = self.final_env(config).await?;
        // Get config-level env results for redactions, env_files, and env_scripts
        let config_env_results = config.env_results().await?;

        // Don't cache if secrets/redactions are present (security)
        let has_secrets =
            !env_results.redactions.is_empty() || !config_env_results.redactions.is_empty();

        // Don't cache if _.source scripts are used (too dynamic - can have side effects)
        let has_scripts =
            !env_results.env_scripts.is_empty() || !config_env_results.env_scripts.is_empty();

        // Don't cache if templates are used (dynamic values like now(), uuid(), etc.)
        let has_templates = env_results.has_templates || config_env_results.has_templates;

        // Don't cache if modules are used (vfox plugins can be dynamic)
        let has_modules = env_results.has_modules || config_env_results.has_modules;

        // Collect all referenced files (from _.file directives) for cache invalidation
        let mut all_env_files = env_results.env_files.clone();
        all_env_files.extend(config_env_results.env_files.clone());

        let mut path_env = PathEnv::from_iter(env::PATH.clone());
        let paths = self.list_final_paths(config, env_results).await?;
        for p in &paths {
            path_env.add(p.clone());
        }
        env.insert(PATH_KEY.to_string(), path_env.to_string());

        // Cache the computed environment for future use (only when experimental is on)
        // Skip caching if:
        // - secrets are present (security - don't persist sensitive data)
        // - _.source scripts are used (too dynamic - can have side effects, read network/DB)
        // - templates are used (dynamic values like now(), uuid(), etc.)
        // - modules are used (vfox plugins can be dynamic)
        if settings.experimental
            && settings.env_cache
            && !has_secrets
            && !has_scripts
            && !has_templates
            && !has_modules
        {
            let cache_key = compute_cache_key(config, self);

            // Build referenced_files with mtimes for cache invalidation
            let referenced_files: Vec<(PathBuf, u128)> = all_env_files
                .into_iter()
                .filter_map(|path| {
                    path.metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .map(|mtime| {
                            let nanos = mtime
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos();
                            (path, nanos)
                        })
                })
                .collect();

            let cached = CachedEnv {
                paths,
                env: env.clone(),
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                referenced_files,
            };
            if let Err(e) = cached.save(&cache_key) {
                trace!("failed to save env cache: {e}");
            }
        }

        Ok(env)
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
