use std::collections::HashMap;
use std::fs;
use std::fs::{remove_file, File};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};
use console::style;
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::Lazy;
use versions::Versioning;

use crate::cache::CacheManager;
use crate::cmd::cmd;
use crate::config::{Config, Settings};
use crate::env::PREFER_STALE;
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::errors::Error::PluginNotInstalled;
use crate::file::remove_all;
use crate::file::{create_dir_all, display_path, remove_all_with_warning};
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::lock_file::LockFile;
use crate::plugins::external_plugin_cache::ExternalPluginCache;
use crate::plugins::rtx_plugin_toml::RtxPluginToml;
use crate::plugins::Script::{Download, ExecEnv, Install, ParseLegacyFile};
use crate::plugins::{Plugin, PluginName, Script, ScriptManager};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::ProgressReport;
use crate::{dirs, env, file};

/// This represents a plugin installed to ~/.local/share/rtx/plugins
#[derive(Debug)]
pub struct ExternalPlugin {
    pub name: PluginName,
    pub plugin_path: PathBuf,
    pub repo_url: Option<String>,
    pub toml: RtxPluginToml,
    cache_path: PathBuf,
    downloads_path: PathBuf,
    installs_path: PathBuf,
    script_man: ScriptManager,
    cache: ExternalPluginCache,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_stable_cache: CacheManager<Option<String>>,
    alias_cache: CacheManager<Vec<(String, String)>>,
    legacy_filename_cache: CacheManager<Vec<String>>,
}

impl ExternalPlugin {
    pub fn new(settings: &Settings, name: &PluginName) -> Self {
        let plugin_path = dirs::PLUGINS.join(name);
        let cache_path = dirs::CACHE.join(name);
        let toml_path = plugin_path.join("rtx.plugin.toml");
        let toml = RtxPluginToml::from_file(&toml_path).unwrap();
        let fresh_duration = if *PREFER_STALE {
            None
        } else {
            Some(Duration::from_secs(60 * 60 * 24))
        };
        Self {
            name: name.into(),
            script_man: build_script_man(settings, name, &plugin_path),
            downloads_path: dirs::DOWNLOADS.join(name),
            installs_path: dirs::INSTALLS.join(name),
            cache: ExternalPluginCache::default(),
            remote_version_cache: CacheManager::new(cache_path.join("remote_versions.msgpack.z"))
                .with_fresh_duration(fresh_duration)
                .with_fresh_file(plugin_path.clone())
                .with_fresh_file(plugin_path.join("bin/list-all")),
            latest_stable_cache: CacheManager::new(cache_path.join("latest_stable.msgpack.z"))
                .with_fresh_duration(fresh_duration)
                .with_fresh_file(plugin_path.clone())
                .with_fresh_file(plugin_path.join("bin/latest-stable")),
            alias_cache: CacheManager::new(cache_path.join("aliases.msgpack.z"))
                .with_fresh_file(plugin_path.clone())
                .with_fresh_file(plugin_path.join("bin/list-aliases")),
            legacy_filename_cache: CacheManager::new(cache_path.join("legacy_filenames.msgpack.z"))
                .with_fresh_file(plugin_path.clone())
                .with_fresh_file(plugin_path.join("bin/list-legacy-filenames")),
            plugin_path,
            cache_path,
            repo_url: None,
            toml,
        }
    }

    pub fn list(settings: &Settings) -> Result<Vec<Self>> {
        Ok(file::dir_subdirs(&dirs::PLUGINS)?
            .iter()
            .map(|name| Self::new(settings, name))
            .collect())
    }
    pub fn get_remote_url(&self) -> Option<String> {
        let git = Git::new(self.plugin_path.to_path_buf());
        git.get_remote_url()
    }

    pub fn install(&self, config: &Config, pr: &mut ProgressReport, force: bool) -> Result<()> {
        self.decorate_progress_bar(pr, None);
        let repository = self
            .repo_url
            .clone()
            .or_else(|| config.get_repo_url(&self.name))
            .ok_or_else(|| eyre!("No repository found for plugin {}", self.name))?;
        let (repo_url, repo_ref) = Git::split_url_and_ref(&repository);
        debug!("install {} {:?}", self.name, repository);

        let _lock = self.get_lock(&self.plugin_path, force)?;

        if self.is_installed() {
            self.uninstall(pr)?;
        }

        let git = Git::new(self.plugin_path.to_path_buf());
        pr.set_message(format!("cloning {repo_url}"));
        git.clone(&repo_url)?;
        if let Some(ref_) = &repo_ref {
            pr.set_message(format!("checking out {ref_}"));
            git.update(Some(ref_.to_string()))?;
        }

        pr.set_message("loading plugin remote versions".into());
        if self.has_list_all_script() {
            self.list_remote_versions(&config.settings)?;
        }
        if self.has_list_alias_script() {
            pr.set_message("getting plugin aliases".into());
            self.get_aliases(&config.settings)?;
        }
        if self.has_list_legacy_filenames_script() {
            pr.set_message("getting plugin legacy filenames".into());
            self.legacy_filenames(&config.settings)?;
        }

        let sha = git.current_sha_short()?;
        pr.finish_with_message(format!(
            "{repo_url}#{}",
            style(&sha).bright().yellow().for_stderr(),
        ));
        Ok(())
    }

    pub fn uninstall(&self, pr: &ProgressReport) -> Result<()> {
        if !self.is_installed() {
            return Ok(());
        }
        pr.set_message("uninstalling".into());

        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            pr.set_message(format!("removing {}", &dir.to_string_lossy()));
            remove_all(dir).wrap_err_with(|| {
                format!(
                    "Failed to remove directory {}",
                    style(&dir.to_string_lossy()).cyan().for_stderr()
                )
            })
        };

        rmdir(&self.downloads_path)?;
        rmdir(&self.installs_path)?;
        rmdir(&self.plugin_path)?;

        Ok(())
    }

    pub fn update(&self, gitref: Option<String>) -> Result<()> {
        let plugin_path = self.plugin_path.to_path_buf();
        if plugin_path.is_symlink() {
            warn!(
                "Plugin: {} is a symlink, not updating",
                style(&self.name).cyan().for_stderr()
            );
            return Ok(());
        }
        let git = Git::new(plugin_path);
        if !git.is_repo() {
            warn!(
                "Plugin {} is not a git repository, not updating",
                style(&self.name).cyan().for_stderr()
            );
            return Ok(());
        }
        // TODO: asdf_run_hook "pre_plugin_update"
        let (_pre, _post) = git.update(gitref)?;
        // TODO: asdf_run_hook "post_plugin_update"
        Ok(())
    }

    fn latest_stable_version(&self, settings: &Settings) -> Result<Option<String>> {
        if let Some(latest) = self.get_latest_stable(settings)? {
            Ok(Some(latest))
        } else {
            self.latest_version(settings, Some("latest".into()))
        }
    }

    fn get_latest_stable(&self, settings: &Settings) -> Result<Option<String>> {
        if !self.has_latest_stable_script() {
            return Ok(None);
        }
        self.latest_stable_cache
            .get_or_try_init(|| self.fetch_latest_stable(settings))
            .map_err(|err| {
                eyre!(
                    "Failed fetching latest stable version for plugin {}: {}",
                    style(&self.name).cyan().for_stderr(),
                    err
                )
            })
            .cloned()
    }

    fn fetch_remote_versions(&self, settings: &Settings) -> Result<Vec<String>> {
        let result = self
            .script_man
            .cmd(settings, &Script::ListAll)
            .stdout_capture()
            .stderr_capture()
            .unchecked()
            .run()
            .map_err(|err| {
                let script = self.script_man.get_script_path(&Script::ListAll);
                eyre!("Failed to run {}: {}", script.display(), err)
            })?;
        let stdout = String::from_utf8(result.stdout).unwrap();
        let stderr = String::from_utf8(result.stderr).unwrap().trim().to_string();

        let display_stderr = || {
            if !stderr.is_empty() {
                eprintln!("{stderr}");
            }
        };
        if !result.status.success() {
            return Err(eyre!(
                "error running {}: exited with code {}\n{}",
                Script::ListAll,
                result.status.code().unwrap_or_default(),
                stderr
            ))?;
        } else if settings.verbose {
            display_stderr();
        }

        Ok(stdout.split_whitespace().map(|v| v.into()).collect())
    }

    fn fetch_legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        let stdout =
            self.script_man
                .read(settings, &Script::ListLegacyFilenames, settings.verbose)?;
        Ok(self.parse_legacy_filenames(&stdout))
    }
    fn parse_legacy_filenames(&self, data: &str) -> Vec<String> {
        data.split_whitespace().map(|v| v.into()).collect()
    }
    fn fetch_latest_stable(&self, settings: &Settings) -> Result<Option<String>> {
        let latest_stable = self
            .script_man
            .read(settings, &Script::LatestStable, settings.verbose)?
            .trim()
            .to_string();
        Ok(if latest_stable.is_empty() {
            None
        } else {
            Some(latest_stable)
        })
    }

    fn has_list_all_script(&self) -> bool {
        self.script_man.script_exists(&Script::ListAll)
    }
    fn has_list_alias_script(&self) -> bool {
        self.script_man.script_exists(&Script::ListAliases)
    }
    fn has_list_legacy_filenames_script(&self) -> bool {
        self.script_man.script_exists(&Script::ListLegacyFilenames)
    }
    fn has_latest_stable_script(&self) -> bool {
        self.script_man.script_exists(&Script::LatestStable)
    }
    fn fetch_aliases(&self, settings: &Settings) -> Result<Vec<(String, String)>> {
        let stdout = self
            .script_man
            .read(settings, &Script::ListAliases, settings.verbose)?;
        Ok(self.parse_aliases(&stdout))
    }
    fn parse_aliases(&self, data: &str) -> Vec<(String, String)> {
        data.lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace().collect_vec();
                if parts.len() != 2 {
                    if !parts.is_empty() {
                        trace!("invalid alias line: {}", line);
                    }
                    return None;
                }
                Some((parts.remove(0).into(), parts.remove(0).into()))
            })
            .collect()
    }

    fn fetch_cached_legacy_file(&self, legacy_file: &Path) -> Result<Option<String>> {
        let fp = self.legacy_cache_file_path(legacy_file);
        if !fp.exists() || fp.metadata()?.modified()? < legacy_file.metadata()?.modified()? {
            return Ok(None);
        }

        Ok(Some(fs::read_to_string(fp)?.trim().into()))
    }

    fn legacy_cache_file_path(&self, legacy_file: &Path) -> PathBuf {
        self.cache_path
            .join("legacy")
            .join(hash_to_str(&legacy_file.to_string_lossy()))
            .with_extension("txt")
    }

    fn write_legacy_cache(&self, legacy_file: &Path, legacy_version: &str) -> Result<()> {
        let fp = self.legacy_cache_file_path(legacy_file);
        fs::create_dir_all(fp.parent().unwrap())?;
        fs::write(fp, legacy_version)?;
        Ok(())
    }

    fn get_lock(&self, path: &Path, force: bool) -> Result<Option<fslock::LockFile>> {
        let lock = if force {
            None
        } else {
            let lock = LockFile::new(path)
                .with_callback(|l| {
                    debug!("waiting for lock on {}", display_path(l));
                })
                .lock()?;
            Some(lock)
        };
        Ok(lock)
    }

    fn incomplete_file_path(&self, tv: &ToolVersion) -> PathBuf {
        self.cache_path(tv).join("incomplete")
    }

    fn create_install_dirs(&self, tv: &ToolVersion) -> Result<()> {
        let _ = remove_all_with_warning(self.install_path(tv));
        let _ = remove_all_with_warning(self.download_path(tv));
        let _ = remove_all_with_warning(self.cache_path(tv));
        let _ = remove_file(self.install_path(tv)); // removes if it is a symlink
        create_dir_all(self.install_path(tv))?;
        create_dir_all(self.download_path(tv))?;
        create_dir_all(self.cache_path(tv))?;
        File::create(self.incomplete_file_path(tv))?;
        Ok(())
    }
    fn cleanup_install_dirs_on_error(&self, settings: &Settings, tv: &ToolVersion) {
        let _ = remove_all_with_warning(self.install_path(tv));
        self.cleanup_install_dirs(settings, tv);
    }
    fn cleanup_install_dirs(&self, settings: &Settings, tv: &ToolVersion) {
        if !settings.always_keep_download {
            let _ = remove_all_with_warning(self.download_path(tv));
        }
    }

    fn fetch_bin_paths(&self, config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        let list_bin_paths = self.plugin_path.join("bin/list-bin-paths");
        let bin_paths = if matches!(tv.request, ToolVersionRequest::System(_)) {
            Vec::new()
        } else if list_bin_paths.exists() {
            let output = self
                .script_man_for_tv(config, tv)
                .cmd(&config.settings, &Script::ListBinPaths)
                .read()?;
            output.split_whitespace().map(|f| f.to_string()).collect()
        } else {
            vec!["bin".into()]
        };
        let bin_paths = bin_paths
            .into_iter()
            .map(|path| self.install_path(tv).join(path))
            .collect();
        Ok(bin_paths)
    }
    fn fetch_exec_env(&self, config: &Config, tv: &ToolVersion) -> Result<HashMap<String, String>> {
        let script = self.script_man_for_tv(config, tv).get_script_path(&ExecEnv);
        let ed = EnvDiff::from_bash_script(&script, &self.script_man_for_tv(config, tv).env)?;
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

    fn script_man_for_tv(&self, config: &Config, tv: &ToolVersion) -> ScriptManager {
        let mut sm = self.script_man.clone();
        for (key, value) in &tv.opts {
            let k = format!("RTX_TOOL_OPTS__{}", key.to_uppercase());
            sm = sm.with_env(k, value.clone());
        }
        if let Some(project_root) = &config.project_root {
            let project_root = project_root.to_string_lossy().to_string();
            sm = sm.with_env("RTX_PROJECT_ROOT".into(), project_root);
        }
        let install_type = match &tv.request {
            ToolVersionRequest::Version(_, _) | ToolVersionRequest::Prefix(_, _) => "version",
            ToolVersionRequest::Ref(_, _) => "ref",
            ToolVersionRequest::Path(_, _) => "path",
            ToolVersionRequest::System(_) => {
                panic!("should not be called for system tool")
            }
        };
        sm = sm
            .with_env(
                "RTX_INSTALL_PATH".into(),
                self.install_path(tv).to_string_lossy().to_string(),
            )
            .with_env(
                "ASDF_INSTALL_PATH".into(),
                self.install_path(tv).to_string_lossy().to_string(),
            )
            .with_env(
                "RTX_DOWNLOAD_PATH".into(),
                self.download_path(tv).to_string_lossy().to_string(),
            )
            .with_env(
                "ASDF_DOWNLOAD_PATH".into(),
                self.download_path(tv).to_string_lossy().to_string(),
            )
            .with_env("RTX_INSTALL_TYPE".into(), install_type.into())
            .with_env("ASDF_INSTALL_TYPE".into(), install_type.into())
            .with_env("RTX_INSTALL_VERSION".into(), tv.version.clone())
            .with_env("ASDF_INSTALL_VERSION".into(), tv.version.clone());
        sm
    }
}

fn build_script_man(settings: &Settings, name: &str, plugin_path: &Path) -> ScriptManager {
    let mut sm = ScriptManager::new(plugin_path.to_path_buf())
        .with_env("RTX_PLUGIN_NAME".into(), name.to_string())
        .with_env(
            "RTX_PLUGIN_PATH".into(),
            plugin_path.to_string_lossy().to_string(),
        );
    if let Some(shims_dir) = &settings.shims_dir {
        let shims_dir = shims_dir.to_string_lossy().to_string();
        sm = sm.with_env("RTX_SHIMS_DIR".into(), shims_dir);
    }

    sm
}

impl Eq for ExternalPlugin {}

impl PartialEq for ExternalPlugin {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Hash for ExternalPlugin {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl Plugin for ExternalPlugin {
    fn name(&self) -> &PluginName {
        &self.name
    }

    fn toml(&self) -> &RtxPluginToml {
        &self.toml
    }

    fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions(settings))
            .map_err(|err| {
                eyre!(
                    "Failed listing remote versions for plugin {}: {}",
                    style(&self.name).cyan().for_stderr(),
                    err
                )
            })
    }

    fn list_installed_versions(&self) -> Result<Vec<String>> {
        Ok(match self.installs_path.exists() {
            true => file::dir_subdirs(&self.installs_path)?
                .iter()
                .filter(|v| !is_runtime_symlink(&self.installs_path.join(v)))
                .map(|v| Versioning::new(v).unwrap_or_default())
                .sorted()
                .map(|v| v.to_string())
                .collect(),
            false => vec![],
        })
    }

    fn latest_version(&self, settings: &Settings, query: Option<String>) -> Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_versions_matching(settings, &query)?;
                let v = match matches.contains(&query) {
                    true => Some(query),
                    false => matches.last().map(|v| v.to_string()),
                };
                Ok(v)
            }
            None => self.latest_stable_version(settings),
        }
    }

    fn latest_installed_version(&self) -> Result<Option<String>> {
        let installed_symlink = self.installs_path.join("latest");
        if installed_symlink.exists() {
            let target = installed_symlink.read_link()?;
            let version = target
                .file_name()
                .ok_or_else(|| eyre!("Invalid symlink target"))?
                .to_string_lossy()
                .to_string();
            Ok(Some(version))
        } else {
            Ok(None)
        }
    }

    fn is_installed(&self) -> bool {
        self.plugin_path.exists()
    }

    fn get_aliases(&self, settings: &Settings) -> Result<IndexMap<String, String>> {
        if let Some(data) = &self.toml.list_aliases.data {
            return Ok(self.parse_aliases(data).into_iter().collect());
        }
        if !self.has_list_alias_script() {
            return Ok(IndexMap::new());
        }
        let aliases = self
            .alias_cache
            .get_or_try_init(|| self.fetch_aliases(settings))
            .map_err(|err| {
                eyre!(
                    "Failed fetching aliases for plugin {}: {}",
                    style(&self.name).cyan().for_stderr(),
                    err
                )
            })?
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ok(aliases)
    }

    fn legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        if let Some(data) = &self.toml.list_legacy_filenames.data {
            return Ok(self.parse_legacy_filenames(data));
        }
        if !self.has_list_legacy_filenames_script() {
            return Ok(vec![]);
        }
        self.legacy_filename_cache
            .get_or_try_init(|| self.fetch_legacy_filenames(settings))
            .map_err(|err| {
                eyre!(
                    "Failed fetching legacy filenames for plugin {}: {}",
                    style(&self.name).cyan().for_stderr(),
                    err
                )
            })
            .cloned()
    }

    fn parse_legacy_file(&self, legacy_file: &Path, settings: &Settings) -> Result<String> {
        if let Some(cached) = self.fetch_cached_legacy_file(legacy_file)? {
            return Ok(cached);
        }
        trace!("parsing legacy file: {}", legacy_file.to_string_lossy());
        let script = ParseLegacyFile(legacy_file.to_string_lossy().into());
        let legacy_version = match self.script_man.script_exists(&script) {
            true => self.script_man.read(settings, &script, settings.verbose)?,
            false => fs::read_to_string(legacy_file)?,
        }
        .trim()
        .to_string();

        self.write_legacy_cache(legacy_file, &legacy_version)?;
        Ok(legacy_version)
    }

    fn external_commands(&self) -> Result<Vec<Vec<String>>> {
        let command_path = self.plugin_path.join("lib/commands");
        if !self.is_installed() || !command_path.exists() {
            return Ok(vec![]);
        }
        let mut commands = vec![];
        for command in file::dir_files(&command_path)? {
            if !command.starts_with("command-") || !command.ends_with(".bash") {
                continue;
            }
            let mut command = command
                .strip_prefix("command-")
                .unwrap()
                .strip_suffix(".bash")
                .unwrap()
                .split('-')
                .map(|s| s.to_string())
                .collect::<Vec<String>>();
            command.insert(0, self.name.clone());
            commands.push(command);
        }
        Ok(commands)
    }

    fn execute_external_command(&self, command: &str, args: Vec<String>) -> Result<()> {
        if !self.is_installed() {
            return Err(PluginNotInstalled(self.name.clone()).into());
        }
        let result = cmd(
            self.plugin_path
                .join("lib/commands")
                .join(format!("command-{command}.bash")),
            args,
        )
        .unchecked()
        .run()?;
        exit(result.status.code().unwrap_or(1));
    }

    fn is_version_installed(&self, tv: &ToolVersion) -> bool {
        match tv.request {
            ToolVersionRequest::System(_) => true,
            _ => self.install_path(tv).exists() && !self.incomplete_file_path(tv).exists(),
        }
    }

    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &mut ProgressReport,
        force: bool,
    ) -> Result<()> {
        self.decorate_progress_bar(pr, Some(tv));

        let settings = &config.settings;
        let _lock = self.get_lock(&self.install_path(tv), force)?;
        self.create_install_dirs(tv)?;

        let run_script = |script| {
            self.script_man_for_tv(config, tv).run_by_line(
                settings,
                script,
                |output| {
                    self.cleanup_install_dirs_on_error(settings, tv);
                    pr.error();
                    if !settings.verbose && !output.trim().is_empty() {
                        pr.println(output);
                    }
                },
                |line| {
                    if !line.trim().is_empty() {
                        pr.set_message(line.into());
                    }
                },
                |line| {
                    if !line.trim().is_empty() {
                        pr.println(line.into());
                    }
                },
            )
        };

        if self.script_man_for_tv(config, tv).script_exists(&Download) {
            pr.set_message("downloading".into());
            run_script(&Download)?;
        }
        pr.set_message("installing".into());
        run_script(&Install)?;
        self.cleanup_install_dirs(settings, tv);

        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::ROOT.to_path_buf()];
        touch_dirs.extend(config.config_files.keys().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                debug!("error touching config file: {:?} {:?}", path, err);
            }
        }
        if let Err(err) = remove_file(self.incomplete_file_path(tv)) {
            debug!("error removing incomplete file: {:?}", err);
        }
        pr.finish();

        Ok(())
    }

    fn uninstall_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
        dryrun: bool,
    ) -> Result<()> {
        pr.set_message(format!("uninstall {}", self.name()));

        if self.plugin_path.join("bin/uninstall").exists() {
            self.script_man_for_tv(config, tv)
                .run(&config.settings, &Script::Uninstall)?;
        }
        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            pr.set_message(format!("removing {}", display_path(dir)));
            if dryrun {
                return Ok(());
            }
            remove_all_with_warning(dir)
        };
        rmdir(&self.install_path(tv))?;
        rmdir(&self.download_path(tv))?;
        Ok(())
    }

    fn list_bin_paths(&self, config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        self.cache
            .list_bin_paths(config, self, tv, || self.fetch_bin_paths(config, tv))
    }

    fn exec_env(&self, config: &Config, tv: &ToolVersion) -> Result<HashMap<String, String>> {
        if matches!(tv.request, ToolVersionRequest::System(_)) {
            return Ok(EMPTY_HASH_MAP.clone());
        }
        if !self.script_man.script_exists(&ExecEnv) || *env::__RTX_SCRIPT {
            // if the script does not exist, or we're already running from within a script,
            // the second is to prevent infinite loops
            return Ok(EMPTY_HASH_MAP.clone());
        }
        self.cache
            .exec_env(config, self, tv, || self.fetch_exec_env(config, tv))
    }

    fn which(&self, config: &Config, tv: &ToolVersion, bin_name: &str) -> Result<Option<PathBuf>> {
        let bin_paths = self.list_bin_paths(config, tv)?;
        for bin_path in bin_paths {
            let bin_path = bin_path.join(bin_name);
            if bin_path.exists() {
                return Ok(Some(bin_path));
            }
        }
        Ok(None)
    }
}

static EMPTY_HASH_MAP: Lazy<HashMap<String, String>> = Lazy::new(HashMap::new);
