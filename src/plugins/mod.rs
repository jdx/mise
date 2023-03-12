use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use color_eyre::eyre::WrapErr;
use color_eyre::eyre::{eyre, Result};
use console::style;
use indexmap::IndexMap;
use itertools::Itertools;
use regex::Regex;
use versions::Versioning;

pub use script_manager::{Script, ScriptManager};

use crate::cache::CacheManager;
use crate::cmd::cmd;
use crate::config::{Config, Settings};
use crate::env::PREFER_STALE;
use crate::errors::Error::PluginNotInstalled;
use crate::fake_asdf::get_path_with_fake_asdf;
use crate::file::{display_path, remove_dir_all, touch_dir};
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::lock_file::LockFile;
use crate::plugins::script_manager::Script::ParseLegacyFile;
use crate::ui::progress_report::{ProgressReport, PROG_TEMPLATE};
use crate::{dirs, file};

mod script_manager;

pub type PluginName = String;

/// This represents a plugin installed to ~/.local/share/rtx/plugins
#[derive(Debug, Clone)]
pub struct Plugin {
    pub name: PluginName,
    pub plugin_path: PathBuf,
    pub repo_url: Option<String>,
    pub repo_ref: Option<String>,
    cache_path: PathBuf,
    downloads_path: PathBuf,
    installs_path: PathBuf,
    script_man: ScriptManager,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_stable_cache: CacheManager<Option<String>>,
    alias_cache: CacheManager<Vec<(String, String)>>,
    legacy_filename_cache: CacheManager<Vec<String>>,
}

impl Plugin {
    pub fn new(name: &PluginName) -> Self {
        let plugin_path = dirs::PLUGINS.join(name);
        let cache_path = dirs::CACHE.join(name);
        let fresh_duration = if *PREFER_STALE {
            None
        } else {
            Some(Duration::from_secs(60 * 60 * 24))
        };
        Self {
            name: name.into(),
            script_man: ScriptManager::new(plugin_path.clone())
                .with_env("PATH".into(), get_path_with_fake_asdf()),
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
        }
    }

    pub fn list() -> Result<Vec<Self>> {
        Ok(file::dir_subdirs(&dirs::PLUGINS)?
            .iter()
            .map(Plugin::new)
            .collect())
    }

    pub fn is_installed(&self) -> bool {
        self.plugin_path.exists()
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
            remove_dir_all(dir).wrap_err_with(|| {
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

    pub fn latest_version(
        &self,
        settings: &Settings,
        query: Option<String>,
    ) -> Result<Option<String>> {
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

    fn latest_stable_version(&self, settings: &Settings) -> Result<Option<String>> {
        if let Some(latest) = self.get_latest_stable(settings)? {
            Ok(Some(latest))
        } else {
            self.latest_version(settings, Some("latest".into()))
        }
    }

    pub fn list_versions_matching(&self, settings: &Settings, query: &str) -> Result<Vec<String>> {
        let mut query = query;
        if query == "latest" {
            query = "[0-9]";
        }
        let version_regex = regex!(
            r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|(a|b|c)[0-9]+|snapshot|master)"
        );
        let query_regex =
            Regex::new((String::from(r"^\s*") + query).as_str()).expect("error parsing regex");
        let versions = self
            .list_remote_versions(settings)?
            .iter()
            .filter(|v| !version_regex.is_match(v))
            .filter(|v| query_regex.is_match(v))
            .cloned()
            .collect_vec();
        Ok(versions)
    }

    pub fn list_installed_versions(&self) -> Result<Vec<String>> {
        Ok(match self.installs_path.exists() {
            true => file::dir_subdirs(&self.installs_path)?
                .iter()
                .map(|v| Versioning::new(v).unwrap_or_default())
                .sorted()
                .map(|v| v.to_string())
                .collect(),
            false => vec![],
        })
    }

    pub fn clear_remote_version_cache(&self) -> Result<()> {
        self.remote_version_cache.clear()?;
        self.latest_stable_cache.clear()
    }
    pub fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions(settings))
            .wrap_err(format!(
                "Failed listing remote versions for plugin {}",
                style(&self.name).cyan().for_stderr()
            ))
    }

    pub fn get_aliases(&self, settings: &Settings) -> Result<IndexMap<String, String>> {
        if !self.has_list_alias_script() {
            return Ok(IndexMap::new());
        }
        let aliases = self
            .alias_cache
            .get_or_try_init(|| self.fetch_aliases(settings))
            .with_context(|| {
                format!(
                    "Failed fetching aliases for plugin {}",
                    style(&self.name).cyan().for_stderr()
                )
            })?
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ok(aliases)
    }

    pub fn legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        if !self.has_list_legacy_filenames_script() {
            return Ok(vec![]);
        }
        self.legacy_filename_cache
            .get_or_try_init(|| self.fetch_legacy_filenames(settings))
            .with_context(|| {
                format!(
                    "Failed fetching legacy filenames for plugin {}",
                    style(&self.name).cyan().for_stderr()
                )
            })
            .cloned()
    }
    fn get_latest_stable(&self, settings: &Settings) -> Result<Option<String>> {
        if !self.has_latest_stable_script() {
            return Ok(None);
        }
        self.latest_stable_cache
            .get_or_try_init(|| self.fetch_latest_stable(settings))
            .with_context(|| {
                format!(
                    "Failed fetching latest stable version for plugin {}",
                    style(&self.name).cyan().for_stderr()
                )
            })
            .cloned()
    }

    pub fn external_commands(&self) -> Result<Vec<Vec<String>>> {
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

    pub fn execute_external_command(&self, command: &str, args: Vec<String>) -> Result<()> {
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

    pub fn decorate_progress_bar(&self, pr: &mut ProgressReport) {
        pr.set_style(PROG_TEMPLATE.clone());
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            style(&self.name).cyan().for_stderr()
        ));
        pr.enable_steady_tick();
    }

    pub fn needs_autoupdate(&self, settings: &Settings) -> Result<bool> {
        if settings.plugin_autoupdate_last_check_duration == Duration::ZERO || !self.is_installed()
        {
            return Ok(false);
        }
        let updated_duration = self.plugin_path.metadata()?.modified()?.elapsed()?;
        Ok(updated_duration > settings.plugin_autoupdate_last_check_duration)
    }

    pub fn autoupdate(&self, _pr: &mut ProgressReport) -> Result<()> {
        trace!("autoupdate({})", self.name);
        // TODO: implement
        // pr.set_message("Checking for updates...".into());
        touch_dir(&self.plugin_path)?;
        Ok(())
    }

    fn fetch_remote_versions(&self, settings: &Settings) -> Result<Vec<String>> {
        let result = self
            .script_man
            .cmd(settings, &Script::ListAll)
            .stdout_capture()
            .stderr_capture()
            .unchecked()
            .run()
            .with_context(|| {
                let script = self.script_man.get_script_path(&Script::ListAll);
                format!("Failed to run {}", script.display())
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
        Ok(self
            .script_man
            .read(settings, &Script::ListLegacyFilenames, settings.verbose)?
            .split_whitespace()
            .map(|v| v.into())
            .collect())
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
        let aliases = stdout
            .lines()
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
            .collect();

        Ok(aliases)
    }

    pub fn parse_legacy_file(&self, legacy_file: &Path, settings: &Settings) -> Result<String> {
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

impl PartialEq for Plugin {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;

    use super::*;

    #[test]
    fn test_exact_match() {
        assert_cli!("plugin", "add", "tiny");
        let settings = Settings::default();
        let plugin = Plugin::new(&PluginName::from("tiny"));
        let version = plugin
            .latest_version(&settings, Some("1.0.0".into()))
            .unwrap()
            .unwrap();
        assert_str_eq!(version, "1.0.0");
        let version = plugin.latest_version(&settings, None).unwrap().unwrap();
        assert_str_eq!(version, "3.1.0");
    }

    #[test]
    fn test_latest_stable() {
        let settings = Settings::default();
        let plugin = Plugin::new(&PluginName::from("dummy"));
        let version = plugin.latest_version(&settings, None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
