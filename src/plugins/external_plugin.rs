use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};
use console::style;
use indexmap::IndexMap;
use itertools::Itertools;
use versions::Versioning;

use crate::cache::CacheManager;
use crate::cmd::cmd;
use crate::config::{Config, Settings};
use crate::env::PREFER_STALE;
use crate::errors::Error::PluginNotInstalled;
use crate::fake_asdf::get_path_with_fake_asdf;
use crate::file::display_path;
use crate::file::remove_all;
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::lock_file::LockFile;
use crate::plugins::rtx_plugin_toml::RtxPluginToml;
use crate::plugins::Script::ParseLegacyFile;
use crate::plugins::{Plugin, PluginName, Script, ScriptManager};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::ui::progress_report::ProgressReport;
use crate::{dirs, file};

/// This represents a plugin installed to ~/.local/share/rtx/plugins
#[derive(Debug, Clone)]
pub struct ExternalPlugin {
    pub name: PluginName,
    pub plugin_path: PathBuf,
    pub repo_url: Option<String>,
    pub repo_ref: Option<String>,
    pub toml: RtxPluginToml,
    cache_path: PathBuf,
    downloads_path: PathBuf,
    installs_path: PathBuf,
    script_man: ScriptManager,
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
            repo_ref: None,
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
        self.decorate_progress_bar(pr);
        let repository = self
            .repo_url
            .as_ref()
            .or_else(|| config.get_shorthands().get(&self.name))
            .ok_or_else(|| eyre!("No repository found for plugin {}", self.name))?;
        debug!("install {} {:?}", self.name, repository);

        let _lock = self.get_lock(force)?;

        if self.is_installed() {
            self.uninstall(pr)?;
        }

        let git = Git::new(self.plugin_path.to_path_buf());
        pr.set_message(format!("cloning {repository}"));
        git.clone(repository)?;
        if let Some(ref_) = &self.repo_ref {
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
            "{repository}#{}",
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

    fn get_lock(&self, force: bool) -> Result<Option<fslock::LockFile>> {
        let lock = if force {
            None
        } else {
            let lock = LockFile::new(&self.plugin_path)
                .with_callback(|l| {
                    debug!("waiting for lock on {}", display_path(l));
                })
                .lock()?;
            Some(lock)
        };
        Ok(lock)
    }
}

fn build_script_man(settings: &Settings, name: &str, plugin_path: &Path) -> ScriptManager {
    let mut sm = ScriptManager::new(plugin_path.to_path_buf())
        .with_env("PATH".into(), get_path_with_fake_asdf())
        .with_env(
            "RTX_DATA_DIR".into(),
            dirs::ROOT.to_string_lossy().into_owned(),
        )
        .with_env("__RTX_SCRIPT".into(), "1".into())
        .with_env("RTX_PLUGIN_NAME".into(), name.to_string());
    if let Some(shims_dir) = &settings.shims_dir {
        let shims_dir = shims_dir.to_string_lossy().to_string();
        sm = sm.with_env("RTX_SHIMS_DIR".into(), shims_dir);
    }

    sm
}

impl PartialEq for ExternalPlugin {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Plugin for ExternalPlugin {
    fn name(&self) -> &PluginName {
        &self.name
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

    fn clear_remote_version_cache(&self) -> Result<()> {
        self.remote_version_cache.clear()?;
        self.latest_stable_cache.clear()
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
}
