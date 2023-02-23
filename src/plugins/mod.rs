use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use color_eyre::eyre::WrapErr;
use color_eyre::eyre::{eyre, Result};
use console::style;
use indexmap::IndexMap;
use indicatif::ProgressStyle;
use itertools::Itertools;

use once_cell::sync::Lazy;
use regex::Regex;
use versions::Versioning;

pub use script_manager::{InstallType, Script, ScriptManager};

use crate::cache::CacheManager;
use crate::cmd::cmd;
use crate::config::{Config, Settings};
use crate::env::RTX_PREFER_STALE;
use crate::errors::Error::PluginNotInstalled;
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::plugins::script_manager::Script::ParseLegacyFile;
use crate::ui::progress_report::ProgressReport;
use crate::{dirs, file};

mod script_manager;

pub type PluginName = String;

/// This represents a plugin installed to ~/.local/share/rtx/plugins
#[derive(Debug, Clone)]
pub struct Plugin {
    pub name: PluginName,
    pub plugin_path: PathBuf,
    cache_path: PathBuf,
    downloads_path: PathBuf,
    installs_path: PathBuf,
    script_man: ScriptManager,
    remote_version_cache: CacheManager<Vec<String>>,
    alias_cache: CacheManager<Vec<(String, String)>>,
    legacy_filename_cache: CacheManager<Vec<String>>,
}

impl Plugin {
    pub fn new(name: &PluginName) -> Self {
        let plugin_path = dirs::PLUGINS.join(name);
        let cache_path = dirs::CACHE.join(name);
        let fresh_duration = if *RTX_PREFER_STALE {
            None
        } else {
            Some(Duration::from_secs(60 * 60 * 24))
        };
        Self {
            name: name.into(),
            script_man: ScriptManager::new(plugin_path.clone()),
            downloads_path: dirs::DOWNLOADS.join(name),
            installs_path: dirs::INSTALLS.join(name),
            remote_version_cache: CacheManager::new(
                cache_path.join("remote_versions.msgpack.zlib"),
            )
            .with_fresh_duration(fresh_duration)
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/list-all")),
            alias_cache: CacheManager::new(cache_path.join("aliases.msgpack.zlib"))
                .with_fresh_file(plugin_path.clone())
                .with_fresh_file(plugin_path.join("bin/list-aliases")),
            legacy_filename_cache: CacheManager::new(
                cache_path.join("legacy_filenames.msgpack.zlib"),
            )
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/list-legacy-filenames")),
            plugin_path,
            cache_path,
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

    pub fn install(
        &self,
        config: &Config,
        repository: Option<&String>,
        mut pr: ProgressReport,
    ) -> Result<()> {
        static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
            ProgressStyle::with_template("{prefix}{wide_msg} {spinner:.blue} {elapsed:.dim.italic}")
                .unwrap()
        });
        pr.set_style(PROG_TEMPLATE.clone());
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            style(&self.name).cyan().for_stderr()
        ));
        pr.enable_steady_tick();
        let repository = repository
            .or_else(|| config.get_shorthands().get(&self.name))
            .ok_or_else(|| eyre!("No repository found for plugin {}", self.name))?;
        debug!("install {} {:?}", self.name, repository);
        if self.is_installed() {
            pr.set_message("uninstalling existing plugin".into());
            self.uninstall()?;
        }

        let git = Git::new(self.plugin_path.to_path_buf());
        pr.set_message(format!("cloning {repository}"));
        git.clone(repository)?;

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
            "{} {repository}@{}",
            style("✓").green().for_stderr(),
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

    pub fn uninstall(&self) -> Result<()> {
        debug!("uninstall {}", self.name);

        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            fs::remove_dir_all(dir).wrap_err_with(|| {
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

    pub fn latest_version(&self, settings: &Settings, query: &str) -> Result<Option<String>> {
        let matches = self.list_versions_matching(settings, query)?;
        let v = match matches.contains(&query.to_string()) {
            true => Some(query.to_string()),
            false => matches.last().map(|v| v.to_string()),
        };
        Ok(v)
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
        self.remote_version_cache.clear()
    }
    pub fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions(settings))
    }

    pub fn get_aliases(&self, settings: &Settings) -> Result<IndexMap<String, String>> {
        let aliases = self
            .alias_cache
            .get_or_try_init(|| self.fetch_aliases(settings))?
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ok(aliases)
    }

    pub fn legacy_filenames(&self, settings: &Settings) -> Result<&Vec<String>> {
        self.legacy_filename_cache
            .get_or_try_init(|| self.fetch_legacy_filenames(settings))
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

    fn fetch_remote_versions(&self, settings: &Settings) -> Result<Vec<String>> {
        let result = self
            .script_man
            .cmd(Script::ListAll)
            .stdout_capture()
            .stderr_capture()
            .unchecked()
            .run()
            .with_context(|| {
                let script = self.script_man.get_script_path(&Script::ListAll);
                format!("failed to run {}", script.display())
            })?;
        let stdout = String::from_utf8(result.stdout).unwrap();
        let stderr = String::from_utf8(result.stderr).unwrap().trim().to_string();

        let display_stderr = || {
            if !stderr.is_empty() {
                eprintln!("{stderr}");
            }
        };
        if !result.status.success() {
            display_stderr();
            return Err(eyre!(
                "error running {}: exited with code {}",
                Script::ListAll,
                result.status.code().unwrap_or_default()
            ))?;
        } else if settings.verbose {
            display_stderr();
        }

        Ok(stdout.split_whitespace().map(|v| v.into()).collect())
    }

    fn fetch_legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        if !self.has_list_legacy_filenames_script() {
            return Ok(vec![]);
        }
        Ok(self
            .script_man
            .read(Script::ListLegacyFilenames, settings.verbose)?
            .split_whitespace()
            .map(|v| v.into())
            .collect())
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
    fn fetch_aliases(&self, settings: &Settings) -> Result<Vec<(String, String)>> {
        if !self.has_list_alias_script() {
            return Ok(vec![]);
        }
        let stdout = self
            .script_man
            .read(Script::ListAliases, settings.verbose)?;
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
            true => self.script_man.read(script, settings.verbose)?,
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
        let version = plugin.latest_version(&settings, "1.0.0").unwrap().unwrap();
        assert_str_eq!(version, "1.0.0");
    }
}
