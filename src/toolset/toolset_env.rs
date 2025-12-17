use std::path::PathBuf;
use std::sync::Arc;

use eyre::Result;

use crate::config::Config;
use crate::config::env_directive::{EnvResolveOptions, EnvResults, ToolsFilter};
use crate::env::{PATH_KEY, WARN_ON_MISSING_REQUIRED_ENV};
use crate::env_diff::EnvMap;
use crate::path_env::PathEnv;
use crate::toolset::Toolset;
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
        let (mut env, env_results) = self.final_env(config).await?;
        let mut path_env = PathEnv::from_iter(env::PATH.clone());
        for p in self.list_final_paths(config, env_results).await? {
            path_env.add(p.clone());
        }
        env.insert(PATH_KEY.to_string(), path_env.to_string());
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
