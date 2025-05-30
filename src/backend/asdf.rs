use std::fmt::{Debug, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Arc};

use crate::backend::backend_type::BackendType;
use crate::backend::external_plugin_cache::ExternalPluginCache;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
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
use color_eyre::eyre::{Result, WrapErr, eyre};
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
    latest_stable_cache: CacheManager<Option<String>>,
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
            latest_stable_cache: CacheManagerBuilder::new(
                ba.cache_path.join("latest_stable.msgpack.z"),
            )
            .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/latest-stable"))
            .build(),
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
        for (key, value) in tv.request.options().opts {
            let k = format!("RTX_TOOL_OPTS__{}", key.to_uppercase());
            sm = sm.with_env(k, value.clone());
            let k = format!("MISE_TOOL_OPTS__{}", key.to_uppercase());
            sm = sm.with_env(k, value.clone());
        }
        for (key, value) in tv.request.options().install_env {
            sm = sm.with_env(key, value.clone());
        }
        if let Some(project_root) = &config.project_root {
            let project_root = project_root.to_string_lossy().to_string();
            sm = sm.with_env("RTX_PROJECT_ROOT", project_root.clone());
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
            .with_env("RTX_DOWNLOAD_PATH", &download)
            .with_env("RTX_INSTALL_PATH", &install)
            .with_env("RTX_INSTALL_TYPE", install_type)
            .with_env("RTX_INSTALL_VERSION", install_version)
            .with_env("MISE_DOWNLOAD_PATH", download)
            .with_env("MISE_INSTALL_PATH", install)
            .with_env("MISE_INSTALL_TYPE", install_type)
            .with_env("MISE_INSTALL_VERSION", install_version);
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

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        self.plugin.fetch_remote_versions()
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> Result<Option<String>> {
        timeout::run_with_timeout_async(
            || async {
                if !self.plugin.has_latest_stable_script() {
                    return self.latest_version(config, Some("latest".into())).await;
                }
                self.latest_stable_cache
                    .get_or_try_init(|| self.plugin.fetch_latest_stable())
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

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
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

    fn parse_idiomatic_file(&self, idiomatic_file: &Path) -> Result<String> {
        if let Some(cached) = self.fetch_cached_idiomatic_file(idiomatic_file)? {
            return Ok(cached);
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
        .trim()
        .to_string();

        self.write_idiomatic_cache(idiomatic_file, &idiomatic_version)?;
        Ok(idiomatic_version)
    }

    fn plugin(&self) -> Option<&PluginEnum> {
        Some(&self.plugin_enum)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let mut sm = self.script_man_for_tv(&ctx.config, &tv).await?;

        for p in ctx.ts.list_paths(&ctx.config).await {
            sm.prepend_path(p);
        }

        let run_script = |script| sm.run_by_line(script, &ctx.pr);

        if sm.script_exists(&Download) {
            ctx.pr.set_message("bin/download".into());
            run_script(&Download)?;
        }
        ctx.pr.set_message("bin/install".into());
        run_script(&Install)?;
        file::remove_dir(&self.ba.downloads_path)?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        config: &Arc<Config>,
        pr: &Box<dyn SingleReport>,
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
        Ok(self
            .cache
            .list_bin_paths(config, self, tv, async || {
                self.fetch_bin_paths(config, tv).await
            })
            .await?
            .into_iter()
            .map(|path| tv.install_path().join(path))
            .collect())
    }

    async fn exec_env(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<EnvMap> {
        if matches!(tv.request, ToolRequest::System { .. }) {
            return Ok(BTreeMap::new());
        }
        if !self.plugin.script_man.script_exists(&ExecEnv) || *env::__MISE_SCRIPT {
            // if the script does not exist, or we're already running from within a script,
            // the second is to prevent infinite loops
            return Ok(BTreeMap::new());
        }
        self.cache
            .exec_env(config, self, tv, async || {
                self.fetch_exec_env(config, ts, tv).await
            })
            .await
    }
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

    #[tokio::test]
    async fn test_debug() {
        let _config = Config::get().await.unwrap();
        let plugin = AsdfBackend::from_arg("dummy".into());
        assert!(format!("{plugin:?}").starts_with("AsdfPlugin { name: \"dummy\""));
    }
}
