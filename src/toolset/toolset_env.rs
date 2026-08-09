use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use eyre::Result;

use crate::backend::Backend;
use crate::config::env_directive::{EnvResolveOptions, EnvResults, ToolsFilter};
use crate::config::{Config, Settings};
use crate::env::{PATH_KEY, WARN_ON_MISSING_REQUIRED_ENV};
use crate::env_diff::EnvMap;
use crate::path_env::PathEnv;
use crate::registry::{REGISTRY, RegistryEnvPath};
use crate::toolset::env_cache::{CachedEnv, compute_settings_hash, get_file_mtime};
use crate::toolset::tool_request::ToolRequest;
use crate::toolset::{ToolVersion, Toolset};
use crate::{env, github, parallel, uv};

/// PATH with mise-managed install dirs filtered out. mise re-adds the current
/// toolset's bin dirs below, so a stale `installs/<tool>/<ver>/bin` left on PATH
/// (e.g. carried in from a frozen env snapshot) does not outrank the version that
/// `mise x`/`run`/`env` selects for the whole process tree. Mirrors the hook-env
/// reactivation filter from #10162. (#10345)
fn pristine_path_without_install_dirs() -> Vec<PathBuf> {
    let install_dirs = crate::path_env::mise_install_dirs();
    env::PATH
        .iter()
        .filter(|p| !crate::path_env::is_mise_install_path(p.as_path(), &install_dirs))
        .cloned()
        .collect()
}

fn merge_registry_env_paths(
    backend_env: Vec<(String, String, String)>,
    registry_paths: Vec<(String, PathBuf, String)>,
) -> Vec<(String, String, String)> {
    if registry_paths.is_empty() {
        return backend_env;
    }

    // env() gives the first tool's value precedence by reversing this list before
    // collecting it. Reproduce that precedence when a registry path augments an
    // environment variable that a backend already emitted.
    let mut effective_backend_env = BTreeMap::new();
    for (key, value, _) in backend_env.iter().rev() {
        effective_backend_env.insert(key.clone(), value.clone());
    }

    let mut paths_by_name: BTreeMap<String, (Vec<PathBuf>, String)> = BTreeMap::new();
    for (name, path, source) in registry_paths {
        let (paths, current_source) = paths_by_name.entry(name).or_default();
        paths.push(path);
        if current_source.is_empty() {
            *current_source = source;
        }
    }

    let names = paths_by_name.keys().cloned().collect::<HashSet<_>>();
    let mut merged = Vec::with_capacity(paths_by_name.len() + backend_env.len());
    for (name, (registry_paths, source)) in paths_by_name {
        let inherited = effective_backend_env
            .get(&name)
            .cloned()
            .or_else(|| env::PRISTINE_ENV.get(&name).cloned());
        let mut seen = HashSet::new();
        let paths = registry_paths
            .into_iter()
            .chain(inherited.iter().flat_map(env::split_paths))
            .filter(|path| seen.insert(path.clone()))
            .collect::<Vec<_>>();
        match env::join_paths(paths) {
            Ok(value) => merged.push((name, value.to_string_lossy().into_owned(), source)),
            Err(err) => warn!("failed to construct registry environment path: {err}"),
        }
    }
    merged.extend(
        backend_env
            .into_iter()
            .filter(|(name, _, _)| !names.contains(name)),
    );
    merged
}

type RegistryEnvCacheContext = BTreeMap<String, Vec<(Vec<String>, Vec<String>, Option<String>)>>;

fn extend_registry_env_cache_context(
    context: &mut RegistryEnvCacheContext,
    tool: &str,
    env_paths: &[RegistryEnvPath],
) {
    for entry in env_paths.iter().filter(|entry| entry.is_supported_os()) {
        context
            .entry(format!("{tool}:{}", entry.name))
            .or_default()
            .push((
                entry.paths.iter().map(|path| path.to_string()).collect(),
                entry.os.iter().map(|os| os.to_string()).collect(),
                env::PRISTINE_ENV.get(entry.name).cloned(),
            ));
    }
}

fn settings_hash_with_registry_env_context(
    mut settings_hash: String,
    context: &RegistryEnvCacheContext,
) -> String {
    if !context.is_empty() {
        settings_hash.push_str("\nregistry-env-paths:");
        settings_hash.push_str(&format!("{context:?}"));
    }
    settings_hash
}

impl Toolset {
    fn list_registry_env_versions(
        &self,
        config: &Arc<Config>,
    ) -> Vec<(Arc<dyn Backend>, ToolVersion)> {
        self.list_current_installed_versions(config)
            .into_iter()
            .filter(|(_, tv)| !matches!(tv.request, ToolRequest::System { .. }))
            .collect()
    }

    pub async fn full_env(&self, config: &Arc<Config>) -> Result<EnvMap> {
        let mut env = env::PRISTINE_ENV.clone().into_iter().collect::<EnvMap>();
        env.extend(self.env_with_path(config).await?.clone());
        Ok(env)
    }

    /// Like full_env but skips `tools=true` env directives (load_post_env).
    /// Used for preinstall hooks where tool-dependent env vars aren't available yet,
    /// and for dependency_env where resolving tools=true modules on a partial toolset
    /// would trigger spurious errors from modules expecting the full PATH.
    pub async fn full_env_without_tools(&self, config: &Arc<Config>) -> Result<EnvMap> {
        let mut env = env::PRISTINE_ENV.clone().into_iter().collect::<EnvMap>();
        env.extend(self.env_with_path_without_tools(config).await?);
        Ok(env)
    }

    /// Like env_with_path but skips `tools=true` env directives.
    /// Used during tool installation where tool-dependent env vars
    /// may reference tools that aren't installed yet, and in
    /// dependency_env to avoid triggering module hooks on a partial PATH.
    pub async fn env_with_path_without_tools(&self, config: &Arc<Config>) -> Result<EnvMap> {
        let (mut env, add_paths) = self.env(config).await?;
        let mut path_env = PathEnv::from_iter(pristine_path_without_install_dirs());
        for p in config.path_dirs().await?.clone() {
            path_env.add(p);
        }
        for p in &add_paths {
            path_env.add(p.clone());
        }
        for p in self.list_paths(config).await {
            path_env.add(p);
        }
        env.insert(PATH_KEY.to_string(), path_env.to_string());
        Ok(env)
    }

    /// the full mise environment including all tool paths
    pub async fn env_with_path(&self, config: &Arc<Config>) -> Result<EnvMap> {
        // Try to load from cache if enabled
        if CachedEnv::is_enabled()
            && let Some(mut cached) = self.try_load_env_cache(config).await?
        {
            trace!("env_cache: using cached environment");
            github::oauth::inject_token_env(&mut cached);
            return Ok(cached);
        }

        let (mut env, env_results) = self.final_env(config).await?;
        let mut path_env = PathEnv::from_iter(pristine_path_without_install_dirs());
        // Use split paths so we save a cache compatible with env_with_path_and_split
        let (user_paths, tool_paths) = self
            .list_final_paths_split(config, env_results.clone())
            .await?;
        for p in user_paths.iter().chain(tool_paths.iter()) {
            path_env.add(p.clone());
        }
        env.insert(PATH_KEY.to_string(), path_env.to_string());

        // Save to cache if enabled and no uncacheable directives
        // Use save_env_cache_split to ensure cache is compatible with env_with_path_and_split
        if CachedEnv::is_enabled()
            && !env_results.has_uncacheable
            && let Err(e) =
                self.save_env_cache_split(config, &env, &user_paths, &tool_paths, &env_results)
        {
            debug!("env_cache: failed to save: {}", e);
        }

        // Inject GitHub OAuth token (if configured) after cache save so the
        // ephemeral token is never persisted to disk.
        github::oauth::inject_token_env(&mut env);

        Ok(env)
    }

    /// Get environment with split paths (user_paths and tool_paths separate)
    /// This method uses the env cache when available and returns paths separately
    /// for proper handling in hook_env.
    pub async fn env_with_path_and_split(
        &self,
        config: &Arc<Config>,
    ) -> Result<(EnvMap, Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>)> {
        // Try to load from cache if enabled
        if CachedEnv::is_enabled()
            && let Some(cached) = self.try_load_env_cache_full(config).await?
        {
            trace!("env_cache: using cached environment with split paths");
            let mut env = cached.env;
            // Reconstruct PATH from cached paths
            let mut path_env = PathEnv::from_iter(pristine_path_without_install_dirs());
            for p in cached.user_paths.iter().chain(cached.tool_paths.iter()) {
                path_env.add(p.clone());
            }
            env.insert(PATH_KEY.to_string(), path_env.to_string());
            github::oauth::inject_token_env(&mut env);
            return Ok((
                env,
                cached.user_paths,
                cached.tool_paths,
                cached.watch_files,
            ));
        }

        // Compute fresh
        let (mut env, env_results) = self.final_env(config).await?;
        let (user_paths, tool_paths) = self
            .list_final_paths_split(config, env_results.clone())
            .await?;

        // Build PATH
        let mut path_env = PathEnv::from_iter(pristine_path_without_install_dirs());
        for p in user_paths.iter().chain(tool_paths.iter()) {
            path_env.add(p.clone());
        }
        env.insert(PATH_KEY.to_string(), path_env.to_string());

        // Save to cache if enabled and no uncacheable directives
        if CachedEnv::is_enabled()
            && !env_results.has_uncacheable
            && let Err(e) =
                self.save_env_cache_split(config, &env, &user_paths, &tool_paths, &env_results)
        {
            debug!("env_cache: failed to save: {}", e);
        }

        // Inject GitHub OAuth token (if configured) after cache save so the
        // ephemeral token is never persisted to disk.
        github::oauth::inject_token_env(&mut env);

        Ok((env, user_paths, tool_paths, env_results.watch_files))
    }

    /// Try to load environment from cache (returns full CachedEnv)
    pub(crate) async fn try_load_env_cache_full(
        &self,
        config: &Arc<Config>,
    ) -> Result<Option<CachedEnv>> {
        config.env_results().await?;
        let cache_key = self.compute_env_cache_key(config)?;
        CachedEnv::load(&cache_key)
    }

    /// Try to load environment from cache (returns reconstructed EnvMap)
    async fn try_load_env_cache(&self, config: &Arc<Config>) -> Result<Option<EnvMap>> {
        match self.try_load_env_cache_full(config).await? {
            Some(cached) => {
                let mut env = cached.env;
                // Reconstruct PATH from cached paths
                let mut path_env = PathEnv::from_iter(pristine_path_without_install_dirs());
                for p in cached.user_paths.into_iter().chain(cached.tool_paths) {
                    path_env.add(p);
                }
                env.insert(PATH_KEY.to_string(), path_env.to_string());
                Ok(Some(env))
            }
            None => Ok(None),
        }
    }

    /// Save environment to cache with split paths
    fn save_env_cache_split(
        &self,
        config: &Arc<Config>,
        env: &EnvMap,
        user_paths: &[PathBuf],
        tool_paths: &[PathBuf],
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

        // Add mise.lock files to watch_files
        for p in config.config_files.keys() {
            if let Some(parent) = p.parent() {
                let lockfile = parent.join("mise.lock");
                if lockfile.exists() {
                    watch_files.push(lockfile);
                }
            }
        }

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
            user_paths: user_paths.to_vec(),
            tool_paths: tool_paths.to_vec(),
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

        // Treat sibling mise.lock files as config inputs for cache invalidation
        // to ensure creation, deletion, and modification of lock files forces
        // a fresh env/watch_files computation.
        let config_lockfiles: Vec<(PathBuf, u64)> = config
            .config_files
            .keys()
            .filter_map(|p| {
                let lockfile = p.parent()?.join("mise.lock");
                let mtime = get_file_mtime(&lockfile)?;
                Some((lockfile, mtime))
            })
            .collect();

        // Collect tool versions
        let current_versions = self.list_current_versions();
        let tool_versions: Vec<(String, String)> = current_versions
            .iter()
            .map(|(b, tv)| (b.id().to_string(), tv.version.clone()))
            .collect();

        // Floating registry metadata can change independently of the config and
        // selected version. Include both the applicable declarations and the
        // inherited values they augment so the cache never reuses an environment
        // produced with stale runtime paths.
        let mut registry_env_context = BTreeMap::new();
        for (_, tv) in self.list_registry_env_versions(config) {
            let Some(tool) = REGISTRY.get(tv.ba().short.as_str()) else {
                continue;
            };
            extend_registry_env_cache_context(
                &mut registry_env_context,
                tool.short,
                tool.env_paths,
            );
        }

        let settings_hash =
            settings_hash_with_registry_env_context(compute_settings_hash(), &registry_env_context);

        // Get base PATH using platform-appropriate separator
        let base_path = std::env::join_paths(env::PATH.iter())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // Include the auto-sourced uv venv (uv.lock + resolved venv) in the key so a venv
        // dir and a sibling sharing the same config files don't collide on one
        // cache entry, which would leak the venv across directories.
        let mut uv_venv_inputs: Vec<(PathBuf, u64)> = Vec::new();
        if Settings::get().python.uv_venv_auto.should_source()
            && let Some(uv_root) = uv::uv_root()
        {
            let lock = uv_root.join("uv.lock");
            let venv = uv::uv_venv_path(config, &uv_root);
            let lock_mtime = get_file_mtime(&lock).unwrap_or(0);
            let venv_mtime = get_file_mtime(&venv).unwrap_or(0);
            uv_venv_inputs.push((lock, lock_mtime));
            uv_venv_inputs.push((venv, venv_mtime));
        }

        Ok(CachedEnv::compute_cache_key(
            &[config_files, config_lockfiles, uv_venv_inputs].concat(),
            &tool_versions,
            &settings_hash,
            &base_path,
        ))
    }

    pub async fn env_from_tools(&self, config: &Arc<Config>) -> Vec<(String, String, String)> {
        let this = Arc::new(self.clone());
        let installed = self.list_registry_env_versions(config);
        let registry_env_paths = installed
            .iter()
            .flat_map(|(_, tv)| {
                REGISTRY
                    .get(tv.ba().short.as_str())
                    .into_iter()
                    .flat_map(move |tool| {
                        tool.env_paths
                            .iter()
                            .filter(|entry| entry.is_supported_os())
                            .flat_map(move |entry| {
                                entry.paths.iter().map(move |path| {
                                    let path = PathBuf::from(path);
                                    let path = if path.is_absolute() {
                                        path
                                    } else if path == std::path::Path::new(".") {
                                        tv.runtime_path()
                                    } else {
                                        tv.runtime_path().join(path)
                                    };
                                    (
                                        entry.name.to_string(),
                                        path,
                                        format!("registry:{}", tool.short),
                                    )
                                })
                            })
                    })
            })
            .collect::<Vec<_>>();
        let items: Vec<_> = installed
            .into_iter()
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

        let envs = envs
            .into_iter()
            .flatten()
            .filter(|(k, _, _)| k.to_uppercase() != "PATH")
            .collect();
        merge_registry_env_paths(envs, registry_env_paths)
    }

    pub async fn env(&self, config: &Arc<Config>) -> Result<(EnvMap, Vec<PathBuf>)> {
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
        let mut path_env = PathEnv::from_iter(pristine_path_without_install_dirs());

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
        ctx.insert("tools", &self.build_tools_tera_map(config));
        let mut env_results = self
            .load_post_env(config, ctx, &tera_env, ToolsFilter::ToolsOnly)
            .await?;

        // Include watch_files from tools=false plugins so the env cache tracks all
        // plugin watch_files, not just tools=true ones. env_results_cached()
        // returns Some here because self.env(config) above always initialises
        // config.env via config.env_results().
        if let Some(non_tool_env) = config.env_results_cached() {
            env_results
                .watch_files
                .extend(non_tool_env.watch_files.clone());
        }

        // Store add_paths separately to maintain consistent PATH ordering
        env_results.tool_add_paths = add_paths;

        env.extend(
            env_results
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.0.clone())),
        );

        // Apply redactions from tools-only env vars (e.g. redact=true + tools=true)
        if !env_results.redactions.is_empty() {
            config.add_redactions_excluding(
                env_results.redactions.iter().cloned(),
                &env,
                &env_results.redaction_exclusions,
            );
        }

        Ok((env, env_results))
    }

    pub(super) async fn load_post_env(
        &self,
        config: &Arc<Config>,
        ctx: tera::Context,
        env: &EnvMap,
        tools_filter: ToolsFilter,
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
        let env_results = EnvResults::resolve_with_toolset(
            config,
            ctx,
            env,
            entries,
            EnvResolveOptions {
                vars: false,
                tools: tools_filter,
                warn_on_missing_required: *WARN_ON_MISSING_REQUIRED_ENV,
            },
            // `_.python.venv` needs the *active* python, which is only knowable here: a
            // `--tool python@3.12` override lives in this toolset and never reaches `Config`.
            Some(self),
        )
        .await?;
        if log::log_enabled!(log::Level::Trace) {
            trace!("{env_results:#?}");
        } else if !env_results.is_empty() {
            debug!("{env_results:?}");
        }
        Ok(env_results)
    }

    /// Resolve only `tools = true` `[env]` *value* directives (plain
    /// `KEY = value` templates such as `{{ tools.python.path }}`) against this
    /// toolset's currently-installed tools, layered on top of `base_env`, and
    /// return just those vars. Env *modules* are skipped (see
    /// [`ToolsFilter::ToolsOnlyVals`]).
    ///
    /// Deliberately lean: it builds only the `tools.*` tera map (cheap; no
    /// `exec_env`) rather than recomputing the full env, so `dependency_env` can
    /// call it per-install without the cost/recursion of `final_env`. Used so a
    /// dependent tool's install picks up vars like `CLOUDSDK_PYTHON` during a
    /// combined `mise install`, mirroring what a re-activated shell exports
    /// between separate installs. (#10282)
    pub async fn tool_val_env(&self, config: &Arc<Config>, base_env: &EnvMap) -> Result<EnvMap> {
        let mut ctx = config.tera_ctx.clone();
        ctx.insert("env", base_env);
        ctx.insert("tools", &self.build_tools_tera_map(config));
        let env_results = self
            .load_post_env(config, ctx, base_env, ToolsFilter::ToolsOnlyVals)
            .await?;
        Ok(env_results
            .env
            .into_iter()
            .map(|(k, (v, _))| (k, v))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_registry_env_paths_prepends_preserves_and_deduplicates() {
        let existing = env::join_paths(["/existing", "/duplicate"])
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let merged = merge_registry_env_paths(
            vec![
                ("NEKOPATH".to_string(), existing, "backend:neko".to_string()),
                (
                    "OTHER".to_string(),
                    "value".to_string(),
                    "backend".to_string(),
                ),
            ],
            vec![
                (
                    "NEKOPATH".to_string(),
                    PathBuf::from("/runtime/neko"),
                    "registry:neko".to_string(),
                ),
                (
                    "NEKOPATH".to_string(),
                    PathBuf::from("/duplicate"),
                    "registry:neko".to_string(),
                ),
            ],
        );

        let value = merged
            .iter()
            .find(|(name, _, _)| name == "NEKOPATH")
            .unwrap();
        assert_eq!(value.2, "registry:neko");
        assert_eq!(
            env::split_paths(&value.1).collect::<Vec<_>>(),
            [
                PathBuf::from("/runtime/neko"),
                PathBuf::from("/duplicate"),
                PathBuf::from("/existing"),
            ]
        );
        assert!(merged.iter().any(|entry| entry.0 == "OTHER"));
    }

    #[test]
    fn test_registry_env_cache_context_preserves_duplicate_declarations() {
        let original = [
            RegistryEnvPath {
                name: "LIBRARY_PATH",
                paths: &["lib/first"],
                os: &[],
            },
            RegistryEnvPath {
                name: "LIBRARY_PATH",
                paths: &["lib/second"],
                os: &[],
            },
        ];
        let changed = [
            RegistryEnvPath {
                name: "LIBRARY_PATH",
                paths: &["lib/changed"],
                os: &[],
            },
            RegistryEnvPath {
                name: "LIBRARY_PATH",
                paths: &["lib/second"],
                os: &[],
            },
        ];

        let cache_key = |entries: &[RegistryEnvPath]| {
            let mut context = RegistryEnvCacheContext::new();
            extend_registry_env_cache_context(&mut context, "example", entries);
            let settings_hash =
                settings_hash_with_registry_env_context("settings".to_string(), &context);
            (
                context,
                CachedEnv::compute_cache_key(&[], &[], &settings_hash, ""),
            )
        };

        let (original_context, original_key) = cache_key(&original);
        let (changed_context, changed_key) = cache_key(&changed);

        assert_eq!(original_context["example:LIBRARY_PATH"].len(), 2);
        assert_eq!(original_context["example:LIBRARY_PATH"][0].0, ["lib/first"]);
        assert_eq!(
            original_context["example:LIBRARY_PATH"][1].0,
            ["lib/second"]
        );
        assert_ne!(original_key, changed_key);
        assert_ne!(original_context, changed_context);
    }
}
