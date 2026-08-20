use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fmt::{Debug, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::external_plugin_cache::ExternalPluginCache;
use crate::backend::normalize_idiomatic_contents;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::env_directive::EnvResults;
use crate::config::{Config, Settings};
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvMap};
use crate::hash::hash_to_str;
use crate::install_context::InstallContext;
use crate::plugins::Script::{Download, ExecEnv, Install, ParseIdiomaticFile};
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::mise_plugin_toml::MisePluginToml;
use crate::plugins::{PluginType, Script, ScriptManager};
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{backend::Backend, plugins::PluginEnum, timeout};
use crate::{dirs, env, file};
use async_trait::async_trait;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use console::style;
use heck::ToKebabCase;

/// This represents a plugin installed to ~/.local/share/mise/plugins
pub struct AsdfBackend {
    pub ba: Arc<BackendArg>,
    pub name: String,
    pub plugin_path: PathBuf,
    pub repo_url: Option<String>,
    pub toml: MisePluginToml,
    plugin: Arc<AsdfPlugin>,
    plugin_enum: PluginEnum,
    cache: ExternalPluginCache,
    latest_stable_caches: Mutex<HashMap<String, Arc<CacheManager<Option<String>>>>>,
    alias_cache: CacheManager<Vec<(String, String)>>,
    idiomatic_filename_cache: CacheManager<Vec<String>>,
}

impl AsdfBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        let name = ba.tool_name.clone();
        let plugin_path = dirs::PLUGINS.join(ba.short.to_kebab_case());
        let plugin = AsdfPlugin::new(name.clone(), plugin_path.clone());
        let mut toml_path = plugin_path.join("mise.plugin.toml");
        if plugin_path.join("rtx.plugin.toml").exists() {
            toml_path = plugin_path.join("rtx.plugin.toml");
        }
        let toml = MisePluginToml::from_file(&toml_path).unwrap();
        let plugin = Arc::new(plugin);
        let plugin_enum = PluginEnum::Asdf(plugin.clone());
        Self {
            cache: ExternalPluginCache::default(),
            latest_stable_caches: Mutex::new(HashMap::new()),
            alias_cache: CacheManagerBuilder::new(ba.cache_path.join("aliases.msgpack.z"))
                .with_fresh_file(plugin_path.clone())
                .with_fresh_file(plugin_path.join("bin/list-aliases"))
                .build(),
            idiomatic_filename_cache: CacheManagerBuilder::new(
                ba.cache_path.join("idiomatic_filenames.msgpack.z"),
            )
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/list-legacy-filenames"))
            .build(),
            plugin_path,
            plugin,
            plugin_enum,
            repo_url: None,
            toml,
            name,
            ba: Arc::new(ba),
        }
    }

    fn fetch_cached_idiomatic_file(&self, idiomatic_file: &Path) -> Result<Option<String>> {
        let fp = self.idiomatic_cache_file_path(idiomatic_file);
        if !fp.exists() || fp.metadata()?.modified()? < idiomatic_file.metadata()?.modified()? {
            return Ok(None);
        }

        Ok(Some(fs::read_to_string(fp)?.trim().into()))
    }

    fn idiomatic_cache_file_path(&self, idiomatic_file: &Path) -> PathBuf {
        self.ba
            .cache_path
            .join("idiomatic")
            .join(&self.name)
            .join(hash_to_str(&idiomatic_file.to_string_lossy()))
            .with_extension("txt")
    }

    fn write_idiomatic_cache(&self, idiomatic_file: &Path, idiomatic_version: &str) -> Result<()> {
        let fp = self.idiomatic_cache_file_path(idiomatic_file);
        file::create_dir_all(fp.parent().unwrap())?;
        file::write(fp, idiomatic_version)?;
        Ok(())
    }

    fn version_listing_cache_context(env_results: &EnvResults) -> Option<String> {
        if env_results.env.is_empty()
            && env_results.env_remove.is_empty()
            && env_results.env_paths.is_empty()
        {
            return None;
        }
        let env = env_results
            .env
            .iter()
            .map(|(key, (value, _))| (key, value))
            .collect::<BTreeMap<_, _>>();
        Some(hash_to_str(&(
            env,
            &env_results.env_remove,
            &env_results.env_paths,
        )))
    }

    fn script_man_for_version_listing(&self, env_results: &EnvResults) -> Result<ScriptManager> {
        let mut sm = self.plugin.script_man.clone();
        for key in &env_results.env_remove {
            sm = sm.without_env(key);
        }
        for (key, (value, _)) in &env_results.env {
            sm = sm.with_env(key, value);
        }
        if !env_results.env_paths.is_empty() {
            let path_key = OsString::from(&*env::PATH_KEY);
            let current_path = sm.env.get(&path_key).cloned().unwrap_or_default();
            let mut paths = env_results.env_paths.clone();
            if !current_path.is_empty() {
                paths.extend(env::split_paths(&current_path));
            }
            sm = sm.with_env(path_key, env::join_paths(paths)?);
        }
        Ok(sm)
    }

    fn latest_stable_cache(&self, context: Option<&str>) -> Arc<CacheManager<Option<String>>> {
        let map_key = context.unwrap_or_default().to_string();
        self.latest_stable_caches
            .lock()
            .unwrap()
            .entry(map_key)
            .or_insert_with(|| {
                let mut cm =
                    CacheManagerBuilder::new(self.ba.cache_path.join("latest_stable.msgpack.z"))
                        .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                        .with_fresh_file(self.plugin_path.clone())
                        .with_fresh_file(self.plugin_path.join("bin/latest-stable"));
                if let Some(context) = context {
                    cm = cm.with_cache_key(context.to_string());
                }
                Arc::new(cm.build())
            })
            .clone()
    }

    async fn fetch_bin_paths(&self, config: &Arc<Config>, tv: &ToolVersion) -> Result<Vec<String>> {
        let list_bin_paths = self.plugin_path.join("bin/list-bin-paths");
        let bin_paths = if matches!(tv.request, ToolRequest::System { .. }) {
            Vec::new()
        } else if list_bin_paths.exists() {
            let sm = self.script_man_for_tv(config, tv).await?;
            // TODO: find a way to enable this without deadlocking
            // for (t, tv) in ts.list_current_installed_versions(config) {
            //     if t.name == self.name {
            //         continue;
            //     }
            //     for p in t.list_bin_paths(config, ts, &tv)? {
            //         sm.prepend_path(p);
            //     }
            // }
            Settings::ensure_not_safe("executing asdf plugin scripts")?;
            let output = sm.cmd(&Script::ListBinPaths).read()?;
            output
                .split_whitespace()
                .map(|f| {
                    if f == "." {
                        String::new()
                    } else {
                        f.to_string()
                    }
                })
                .collect()
        } else {
            vec!["bin".into()]
        };
        Ok(bin_paths)
    }
    async fn fetch_exec_env(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<EnvMap> {
        let mut sm = self.script_man_for_tv(config, tv).await?;
        for p in ts.list_paths(config).await {
            sm.prepend_path(p);
        }
        let script = sm.get_script_path(&ExecEnv);
        let dir = dirs::CWD.clone().unwrap_or_default();
        let ed = EnvDiff::from_bash_script(&script, &dir, &sm.env, &Default::default())?;
        let env = ed
            .to_patches()
            .into_iter()
            .filter_map(|p| match p {
                EnvDiffOperation::Add(key, value) => Some((key, value)),
                EnvDiffOperation::Change(key, value) => Some((key, value)),
                _ => None,
            })
            .collect();
        Ok(env)
    }

    async fn script_man_for_tv(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<ScriptManager> {
        let mut sm = self.plugin.script_man.clone();
        for (key, value) in tv.request.options().opts_as_strings() {
            let k = format!("MISE_TOOL_OPTS__{}", key.to_uppercase());
            sm = sm.with_env(k, value);
        }
        for (key, value) in tv.install_env() {
            sm = match value.into_string() {
                Some(value) => sm.with_env(key, value),
                None => sm.without_env(key),
            };
        }
        if let Some(project_root) = &config.project_root {
            let project_root = project_root.to_string_lossy().to_string();
            sm = sm.with_env("MISE_PROJECT_ROOT", project_root);
        }
        let install_type = match &tv.request {
            ToolRequest::Version { .. } | ToolRequest::Prefix { .. } => "version",
            ToolRequest::Ref { .. } => "ref",
            ToolRequest::Path { .. } => "path",
            ToolRequest::Sub { .. } => "sub",
            ToolRequest::System { .. } => {
                panic!("should not be called for system tool")
            }
        };
        let install_version = match &tv.request {
            ToolRequest::Ref { ref_: v, .. } => v, // should not have "ref:" prefix
            _ => &tv.version,
        };
        // add env vars from mise.toml files
        for (key, value) in config.env().await? {
            sm = sm.with_env(key, value.clone());
        }
        let install = tv.install_path().to_string_lossy().to_string();
        let download = tv.download_path().to_string_lossy().to_string();
        sm = sm
            .with_env("ASDF_DOWNLOAD_PATH", &download)
            .with_env("ASDF_INSTALL_PATH", &install)
            .with_env("ASDF_INSTALL_TYPE", install_type)
            .with_env("ASDF_INSTALL_VERSION", install_version)
            .with_env("MISE_DOWNLOAD_PATH", download)
            .with_env("MISE_INSTALL_PATH", install)
            .with_env("MISE_INSTALL_TYPE", install_type)
            .with_env(env::MISE_INSTALL_VERSION_ENV_VAR, install_version);
        Ok(sm)
    }
}

impl Eq for AsdfBackend {}

impl PartialEq for AsdfBackend {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Hash for AsdfBackend {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

#[async_trait]
impl Backend for AsdfBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Asdf
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_plugin_type(&self) -> Option<PluginType> {
        Some(PluginType::Asdf)
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    /// ASDF plugins handle their own downloads through plugin scripts.
    /// Lockfile URLs are not applicable since installation is delegated to plugin scripts.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    async fn remote_version_cache_context(&self, config: &Arc<Config>) -> Result<Option<String>> {
        Ok(Self::version_listing_cache_context(
            config.env_results().await?,
        ))
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let env_results = config.env_results().await?;
        let sm = self.script_man_for_version_listing(env_results)?;
        let versions = self.plugin.fetch_remote_versions(&sm)?;
        Ok(versions
            .into_iter()
            .map(|v| VersionInfo {
                version: v,
                ..Default::default()
            })
            .collect())
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> Result<Option<String>> {
        let env_results = config.env_results().await?;
        let context = Self::version_listing_cache_context(env_results);
        let sm = self.script_man_for_version_listing(env_results)?;
        let cache = self.latest_stable_cache(context.as_deref());
        timeout::run_with_timeout_async(
            || async {
                if !self.plugin.has_latest_stable_script() {
                    return Ok(None);
                }
                cache
                    .get_or_try_init(|| self.plugin.fetch_latest_stable(&sm))
                    .wrap_err_with(|| {
                        eyre!(
                            "Failed fetching latest stable version for plugin {}",
                            style(&self.name).blue().for_stderr(),
                        )
                    })
                    .cloned()
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        if let Some(data) = &self.toml.list_aliases.data {
            return Ok(self.plugin.parse_aliases(data).into_iter().collect());
        }
        if !self.plugin.has_list_alias_script() {
            return Ok(BTreeMap::new());
        }
        let aliases = self
            .alias_cache
            .get_or_try_init(|| self.plugin.fetch_aliases())
            .wrap_err_with(|| {
                eyre!(
                    "Failed fetching aliases for plugin {}",
                    style(&self.name).blue().for_stderr(),
                )
            })?
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ok(aliases)
    }

    async fn _idiomatic_filenames(&self) -> Result<Vec<String>> {
        if let Some(data) = &self.toml.list_idiomatic_filenames.data {
            return Ok(self.plugin.parse_idiomatic_filenames(data));
        }
        if !self.plugin.has_list_idiomatic_filenames_script() {
            return Ok(vec![]);
        }
        self.idiomatic_filename_cache
            .get_or_try_init(|| self.plugin.fetch_idiomatic_filenames())
            .wrap_err_with(|| {
                eyre!(
                    "Failed fetching idiomatic filenames for plugin {}",
                    style(&self.name).blue().for_stderr(),
                )
            })
            .cloned()
    }

    async fn _parse_idiomatic_file(&self, idiomatic_file: &Path) -> Result<Vec<String>> {
        if let Some(cached) = self.fetch_cached_idiomatic_file(idiomatic_file)? {
            return Ok(cached.split_whitespace().map(|s| s.to_string()).collect());
        }
        trace!(
            "parsing idiomatic file: {}",
            idiomatic_file.to_string_lossy()
        );
        let script = ParseIdiomaticFile(idiomatic_file.to_string_lossy().into());
        let idiomatic_version = match self.plugin.script_man.script_exists(&script) {
            true => self.plugin.script_man.read(&script)?,
            false => fs::read_to_string(idiomatic_file)?,
        }
        .to_string();
        let idiomatic_version = normalize_idiomatic_contents(&idiomatic_version);

        self.write_idiomatic_cache(idiomatic_file, &idiomatic_version)?;
        if idiomatic_version.is_empty() {
            return Ok(vec![]);
        }
        Ok(idiomatic_version
            .split_whitespace()
            .map(|s| s.to_string())
            .collect())
    }

    fn plugin(&self) -> Option<&PluginEnum> {
        Some(&self.plugin_enum)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let mut sm = self.script_man_for_tv(&ctx.config, &tv).await?;

        // `ctx.ts` is the unresolved install toolset during a combined install, so it
        // does not expose tools that just finished installing. Resolve this tool's
        // declared dependencies separately so asdf install scripts can execute them
        // on the first install (#4384). Keep the existing active-tool paths after the
        // dependencies for compatibility, and preserve each toolset's path order.
        let dependency_paths = self
            .install_dependency_toolset(&ctx.config, &tv)
            .await?
            .list_paths(&ctx.config)
            .await;
        let active_paths = ctx.ts.list_paths(&ctx.config).await;
        let mut seen = HashSet::new();
        let paths: Vec<_> = dependency_paths
            .into_iter()
            .chain(active_paths)
            .filter(|path| seen.insert(path.clone()))
            .collect();
        for p in paths.into_iter().rev() {
            sm.prepend_path(p);
        }

        let run_script = |script| sm.run_by_line(script, ctx.pr.as_ref());

        if sm.script_exists(&Download) {
            ctx.pr.set_message("bin/download".into());
            run_script(&Download)?;
        }
        ctx.pr.set_message("bin/install".into());
        run_script(&Install)?;
        verify_install_script_output(&self.ba.short, &tv.install_path())?;
        file::remove_dir(&self.ba.downloads_path)?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        config: &Arc<Config>,
        pr: &dyn SingleReport,
        tv: &ToolVersion,
    ) -> Result<()> {
        if self.plugin_path.join("bin/uninstall").exists() {
            self.script_man_for_tv(config, tv)
                .await?
                .run_by_line(&Script::Uninstall, pr)?;
        }
        Ok(())
    }

    async fn list_bin_paths(&self, config: &Arc<Config>, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        let runtime_path = tv.runtime_path();
        Ok(self
            .cache
            .list_bin_paths(config, self, tv, async || {
                self.fetch_bin_paths(config, tv).await
            })
            .await?
            .into_iter()
            .map(|path| runtime_path.join(path))
            .collect())
    }

    async fn exec_env(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<EnvMap> {
        let total_start = std::time::Instant::now();
        if matches!(tv.request, ToolRequest::System { .. }) {
            return Ok(BTreeMap::new());
        }
        if !self.plugin.script_man.script_exists(&ExecEnv) || *env::__MISE_SCRIPT {
            // if the script does not exist, or we're already running from within a script,
            // the second is to prevent infinite loops
            return Ok(BTreeMap::new());
        }
        let res = self
            .cache
            .exec_env(config, self, tv, async || {
                self.fetch_exec_env(config, ts, tv).await
            })
            .await;
        trace!(
            "exec_env cache.get_or_try_init_async for {} finished in {}ms",
            self.name,
            total_start.elapsed().as_millis()
        );
        res
    }
}

/// Verifies that `bin/install` actually put something in `$ASDF_INSTALL_PATH`.
///
/// `bin/install` is a plugin-supplied shell script, and one that installs nothing can still exit 0
/// — a missing `set -e`, a build that fails inside a pipeline, a download that 404s. mise would
/// otherwise report `✓ installed`, record the version in install state, and keep resolving to an
/// empty directory afterwards (#5288).
///
/// Emptiness is exact rather than a guess: `Backend::create_install_dirs` recreates the install
/// path immediately before the script runs, and the `incomplete` marker lives under the cache dir,
/// so anything present afterwards came from the plugin. It is also all that can be checked — asdf
/// plugins install into `bin/`, `libexec/`, an unpacked tarball root, or wherever the tool puts
/// things, so a stricter test (spm requires an executable in `bin/`) would reject working plugins.
///
/// There is no `system` case to exclude: anything reaching here has already been through
/// `script_man_for_tv`, which panics for `ToolRequest::System`.
fn verify_install_script_output(tool: &str, install_path: &Path) -> Result<()> {
    if file::ls(install_path)?.is_empty() {
        bail!(
            "{tool}'s bin/install exited successfully but installed nothing into {}; check that the plugin installs into $ASDF_INSTALL_PATH and that it fails when the build fails",
            file::display_path(install_path)
        );
    }
    Ok(())
}

impl Debug for AsdfBackend {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsdfPlugin")
            .field("name", &self.name)
            .field("plugin_path", &self.plugin_path)
            .field("cache_path", &self.ba.cache_path)
            .field("downloads_path", &self.ba.downloads_path)
            .field("installs_path", &self.ba.installs_path)
            .field("repo_url", &self.repo_url)
            .finish()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::ffi::OsString;

    #[test]
    fn test_verify_install_script_output() {
        let temp = tempfile::tempdir().unwrap();
        let install_path = temp.path().join("1.0.0");

        // a missing install path is the same failure as an empty one: nothing was installed
        let err = verify_install_script_output("tiny", &install_path)
            .unwrap_err()
            .to_string();
        assert!(err.contains("installed nothing into"), "{err}");
        std::fs::create_dir_all(&install_path).unwrap();
        assert!(verify_install_script_output("tiny", &install_path).is_err());

        // anything at all is enough — deliberately not an executable and not `bin/`, because asdf
        // plugins choose their own layout and this must not become spm's stricter check
        std::fs::create_dir(install_path.join("libexec")).unwrap();
        verify_install_script_output("tiny", &install_path).unwrap();
    }

    fn env_results(entries: &[(&str, &str)], removals: &[&str], paths: &[&str]) -> EnvResults {
        let mut results = EnvResults::default();
        for (key, value) in entries {
            results.env.insert(
                (*key).to_string(),
                ((*value).to_string(), PathBuf::from("mise.toml")),
            );
        }
        results.env_remove = removals.iter().map(|key| (*key).to_string()).collect();
        results.env_paths = paths.iter().map(PathBuf::from).collect();
        results
    }

    #[test]
    fn version_listing_cache_context_is_stable_and_tracks_changes() {
        let first = env_results(
            &[("TOKEN", "secret"), ("CHANNEL", "stable")],
            &["REMOVE_ME"],
            &["/first/bin"],
        );
        let reordered = env_results(
            &[("CHANNEL", "stable"), ("TOKEN", "secret")],
            &["REMOVE_ME"],
            &["/first/bin"],
        );
        let changed = env_results(
            &[("TOKEN", "other"), ("CHANNEL", "stable")],
            &["REMOVE_ME"],
            &["/first/bin"],
        );
        let changed_removal = env_results(
            &[("TOKEN", "secret"), ("CHANNEL", "stable")],
            &["OTHER"],
            &["/first/bin"],
        );
        let changed_path = env_results(
            &[("TOKEN", "secret"), ("CHANNEL", "stable")],
            &["REMOVE_ME"],
            &["/other/bin"],
        );

        assert_eq!(
            AsdfBackend::version_listing_cache_context(&first),
            AsdfBackend::version_listing_cache_context(&reordered)
        );
        assert_ne!(
            AsdfBackend::version_listing_cache_context(&first),
            AsdfBackend::version_listing_cache_context(&changed)
        );
        assert_ne!(
            AsdfBackend::version_listing_cache_context(&first),
            AsdfBackend::version_listing_cache_context(&changed_removal)
        );
        assert_ne!(
            AsdfBackend::version_listing_cache_context(&first),
            AsdfBackend::version_listing_cache_context(&changed_path)
        );
        assert_eq!(
            AsdfBackend::version_listing_cache_context(&EnvResults::default()),
            None
        );
    }

    #[test]
    fn version_listing_script_manager_applies_config_env() {
        let backend = AsdfBackend::from_arg("dummy".into());
        let results = env_results(
            &[("MISE_TEST_VERSION_CHANNEL", "private")],
            &["REMOVE_ME"],
            &["/first/bin", "/second/bin"],
        );

        let sm = backend.script_man_for_version_listing(&results).unwrap();

        assert_eq!(
            sm.env.get(&OsString::from("MISE_TEST_VERSION_CHANNEL")),
            Some(&OsString::from("private"))
        );
        assert!(!sm.env.contains_key(&OsString::from("REMOVE_ME")));
        let paths = env::split_paths(sm.env.get(&OsString::from(&*env::PATH_KEY)).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            &paths[..2],
            &[PathBuf::from("/first/bin"), PathBuf::from("/second/bin")]
        );
    }

    #[test]
    fn version_listing_script_manager_can_add_paths_after_path_removal() {
        let backend = AsdfBackend::from_arg("dummy".into());
        let results = env_results(&[], &["PATH"], &["/only/bin"]);

        let sm = backend.script_man_for_version_listing(&results).unwrap();
        let paths = env::split_paths(sm.env.get(&OsString::from(&*env::PATH_KEY)).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(paths, vec![PathBuf::from("/only/bin")]);
    }

    #[tokio::test]
    async fn test_debug() {
        let _config = Config::get().await.unwrap();
        let plugin = AsdfBackend::from_arg("dummy".into());
        assert!(format!("{plugin:?}").starts_with("AsdfPlugin { name: \"dummy\""));
    }
}
