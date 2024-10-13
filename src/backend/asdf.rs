use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use console::style;
use itertools::Itertools;
use rayon::prelude::*;
use url::Url;

use crate::backend::external_plugin_cache::ExternalPluginCache;
use crate::backend::{ABackend, Backend, BackendList};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings, SETTINGS};
use crate::default_shorthands::DEFAULT_SHORTHANDS;
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::mise_plugin_toml::MisePluginToml;
use crate::plugins::Script::{Download, ExecEnv, Install, ParseLegacyFile};
use crate::plugins::{Plugin, PluginType, Script, ScriptManager};
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, http};

/// This represents a plugin installed to ~/.local/share/mise/plugins
pub struct AsdfBackend {
    pub ba: BackendArg,
    pub name: String,
    pub plugin_path: PathBuf,
    pub repo_url: Option<String>,
    pub toml: MisePluginToml,
    plugin: Box<AsdfPlugin>,
    cache: ExternalPluginCache,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_stable_cache: CacheManager<Option<String>>,
    alias_cache: CacheManager<Vec<(String, String)>>,
    legacy_filename_cache: CacheManager<Vec<String>>,
}

impl AsdfBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        let name = ba.short.to_string();
        let plugin_path = dirs::PLUGINS.join(&name);
        let mut toml_path = plugin_path.join("mise.plugin.toml");
        if plugin_path.join("rtx.plugin.toml").exists() {
            toml_path = plugin_path.join("rtx.plugin.toml");
        }
        let toml = MisePluginToml::from_file(&toml_path).unwrap();
        Self {
            cache: ExternalPluginCache::default(),
            remote_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/list-all"))
            .build(),
            latest_stable_cache: CacheManagerBuilder::new(
                ba.cache_path.join("latest_stable.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/latest-stable"))
            .build(),
            alias_cache: CacheManagerBuilder::new(ba.cache_path.join("aliases.msgpack.z"))
                .with_fresh_file(plugin_path.clone())
                .with_fresh_file(plugin_path.join("bin/list-aliases"))
                .build(),
            legacy_filename_cache: CacheManagerBuilder::new(
                ba.cache_path.join("legacy_filenames.msgpack.z"),
            )
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/list-legacy-filenames"))
            .build(),
            plugin_path,
            plugin: Box::new(AsdfPlugin::new(name.clone())),
            repo_url: None,
            toml,
            name,
            ba,
        }
    }
    pub fn plugin(&self) -> &dyn Plugin {
        &*self.plugin
    }

    pub fn list() -> Result<BackendList> {
        Ok(file::dir_subdirs(&dirs::PLUGINS)?
            .into_par_iter()
            // if metadata.lua exists it's a vfox plugin (hopefully)
            .filter(|name| !dirs::PLUGINS.join(name).join("metadata.lua").exists())
            .map(|name| Arc::new(Self::from_arg(name.into())) as ABackend)
            .collect())
    }

    fn fetch_versions(&self) -> Result<Option<Vec<String>>> {
        let settings = Settings::get();
        if !settings.use_versions_host {
            return Ok(None);
        }
        // ensure that we're using a default shorthand plugin
        let git = Git::new(&self.plugin_path);
        let normalized_remote = normalize_remote(&git.get_remote_url().unwrap_or_default())
            .unwrap_or("INVALID_URL".into());
        let shorthand_remote = DEFAULT_SHORTHANDS
            .get(self.name.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if normalized_remote != normalize_remote(&shorthand_remote).unwrap_or_default() {
            return Ok(None);
        }
        let settings = Settings::get();
        let raw_versions = match settings.paranoid {
            true => HTTP_FETCH.get_text(format!("https://mise-versions.jdx.dev/{}", self.name)),
            false => HTTP_FETCH.get_text(format!("http://mise-versions.jdx.dev/{}", self.name)),
        };
        let versions =
            // using http is not a security concern and enabling tls makes mise significantly slower
            match raw_versions {
                Err(err) if http::error_code(&err) == Some(404) => return Ok(None),
                res => res?,
            };
        let versions = versions
            .lines()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect_vec();
        match versions.is_empty() {
            true => Ok(None),
            false => Ok(Some(versions)),
        }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.fetch_versions() {
            Ok(Some(versions)) => return Ok(versions),
            Err(err) => warn!(
                "Failed to fetch remote versions for plugin {}: {}",
                style(&self.name).blue().for_stderr(),
                err
            ),
            _ => {}
        };
        self.plugin.fetch_remote_versions()
    }
    fn fetch_cached_legacy_file(&self, legacy_file: &Path) -> Result<Option<String>> {
        let fp = self.legacy_cache_file_path(legacy_file);
        if !fp.exists() || fp.metadata()?.modified()? < legacy_file.metadata()?.modified()? {
            return Ok(None);
        }

        Ok(Some(fs::read_to_string(fp)?.trim().into()))
    }

    fn legacy_cache_file_path(&self, legacy_file: &Path) -> PathBuf {
        self.ba
            .cache_path
            .join("legacy")
            .join(&self.name)
            .join(hash_to_str(&legacy_file.to_string_lossy()))
            .with_extension("txt")
    }

    fn write_legacy_cache(&self, legacy_file: &Path, legacy_version: &str) -> Result<()> {
        let fp = self.legacy_cache_file_path(legacy_file);
        file::create_dir_all(fp.parent().unwrap())?;
        file::write(fp, legacy_version)?;
        Ok(())
    }

    fn fetch_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<String>> {
        let list_bin_paths = self.plugin_path.join("bin/list-bin-paths");
        let bin_paths = if matches!(tv.request, ToolRequest::System(_)) {
            Vec::new()
        } else if list_bin_paths.exists() {
            let sm = self.script_man_for_tv(tv)?;
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
    fn fetch_exec_env(&self, ts: &Toolset, tv: &ToolVersion) -> Result<BTreeMap<String, String>> {
        let mut sm = self.script_man_for_tv(tv)?;
        for p in ts.list_paths() {
            sm.prepend_path(p);
        }
        let script = sm.get_script_path(&ExecEnv);
        let ed = EnvDiff::from_bash_script(&script, &sm.env)?;
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

    fn script_man_for_tv(&self, tv: &ToolVersion) -> Result<ScriptManager> {
        let config = Config::get();
        let mut sm = self.plugin.script_man.clone();
        for (key, value) in &tv.request.options() {
            let k = format!("RTX_TOOL_OPTS__{}", key.to_uppercase());
            sm = sm.with_env(k, value.clone());
            let k = format!("MISE_TOOL_OPTS__{}", key.to_uppercase());
            sm = sm.with_env(k, value.clone());
        }
        if let Some(project_root) = &config.project_root {
            let project_root = project_root.to_string_lossy().to_string();
            sm = sm.with_env("RTX_PROJECT_ROOT", project_root.clone());
            sm = sm.with_env("MISE_PROJECT_ROOT", project_root);
        }
        let install_type = match &tv.request {
            ToolRequest::Version { .. } | ToolRequest::Prefix { .. } => "version",
            ToolRequest::Ref { .. } => "ref",
            ToolRequest::Path(_, _) => "path",
            ToolRequest::Sub { .. } => "sub",
            ToolRequest::System(_) => {
                panic!("should not be called for system tool")
            }
        };
        let install_version = match &tv.request {
            ToolRequest::Ref { ref_: v, .. } => v, // should not have "ref:" prefix
            _ => &tv.version,
        };
        // add env vars from .mise.toml files
        for (key, value) in config.env()? {
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

impl Backend for AsdfBackend {
    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_plugin_type(&self) -> PluginType {
        PluginType::Asdf
    }

    fn get_dependencies(&self, tvr: &ToolRequest) -> Result<Vec<BackendArg>> {
        let out = match tvr.backend().name.as_str() {
            "poetry" | "pipenv" | "pipx" => vec!["python"],
            "elixir" => vec!["erlang"],
            _ => vec![],
        };
        Ok(out.into_iter().map(|s| s.into()).collect())
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .wrap_err_with(|| {
                eyre!(
                    "Failed listing remote versions for plugin {}",
                    style(&self.name).blue().for_stderr(),
                )
            })
            .cloned()
    }

    fn latest_stable_version(&self) -> Result<Option<String>> {
        if !self.plugin.has_latest_stable_script() {
            return self.latest_version(Some("latest".into()));
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
    }

    fn get_remote_url(&self) -> Option<String> {
        let git = Git::new(&self.plugin_path);
        git.get_remote_url().or_else(|| self.repo_url.clone())
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

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        if let Some(data) = &self.toml.list_legacy_filenames.data {
            return Ok(self.plugin.parse_legacy_filenames(data));
        }
        if !self.plugin.has_list_legacy_filenames_script() {
            return Ok(vec![]);
        }
        self.legacy_filename_cache
            .get_or_try_init(|| self.plugin.fetch_legacy_filenames())
            .wrap_err_with(|| {
                eyre!(
                    "Failed fetching legacy filenames for plugin {}",
                    style(&self.name).blue().for_stderr(),
                )
            })
            .cloned()
    }

    fn parse_legacy_file(&self, legacy_file: &Path) -> Result<String> {
        if let Some(cached) = self.fetch_cached_legacy_file(legacy_file)? {
            return Ok(cached);
        }
        trace!("parsing legacy file: {}", legacy_file.to_string_lossy());
        let script = ParseLegacyFile(legacy_file.to_string_lossy().into());
        let legacy_version = match self.plugin.script_man.script_exists(&script) {
            true => self.plugin.script_man.read(&script)?,
            false => fs::read_to_string(legacy_file)?,
        }
        .trim()
        .to_string();

        self.write_legacy_cache(legacy_file, &legacy_version)?;
        Ok(legacy_version)
    }

    fn plugin(&self) -> Option<&dyn Plugin> {
        Some(self.plugin())
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let mut sm = self.script_man_for_tv(&ctx.tv)?;

        for p in ctx.ts.list_paths() {
            sm.prepend_path(p);
        }

        let run_script = |script| sm.run_by_line(script, ctx.pr.as_ref());

        if sm.script_exists(&Download) {
            ctx.pr.set_message("downloading".into());
            run_script(&Download)?;
        }
        ctx.pr.set_message("installing".into());
        run_script(&Install)?;
        file::remove_dir(&self.ba.downloads_path)?;

        Ok(())
    }

    fn uninstall_version_impl(&self, pr: &dyn SingleReport, tv: &ToolVersion) -> Result<()> {
        if self.plugin_path.join("bin/uninstall").exists() {
            self.script_man_for_tv(tv)?
                .run_by_line(&Script::Uninstall, pr)?;
        }
        Ok(())
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        Ok(self
            .cache
            .list_bin_paths(self, tv, || self.fetch_bin_paths(tv))?
            .into_iter()
            .map(|path| tv.install_short_path().join(path))
            .collect())
    }

    fn exec_env(
        &self,
        config: &Config,
        ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        if matches!(tv.request, ToolRequest::System(_)) {
            return Ok(BTreeMap::new());
        }
        if !self.plugin.script_man.script_exists(&ExecEnv) || *env::__MISE_SCRIPT {
            // if the script does not exist, or we're already running from within a script,
            // the second is to prevent infinite loops
            return Ok(BTreeMap::new());
        }
        self.cache
            .exec_env(config, self, tv, || self.fetch_exec_env(ts, tv))
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

fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url.host_str().unwrap();
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use crate::test::reset;

    use super::*;

    #[test]
    fn test_debug() {
        reset();
        let plugin = AsdfBackend::from_arg("dummy".into());
        assert!(format!("{:?}", plugin).starts_with("AsdfPlugin { name: \"dummy\""));
    }
}
