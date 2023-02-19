use std::fs;
use std::fs::remove_file;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use atty::Stream;
use atty::Stream::Stderr;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::{eyre, Result};
use console::style;
use indexmap::IndexMap;
use indicatif::ProgressStyle;
use itertools::Itertools;
use lazy_static::lazy_static;
use once_cell::sync::Lazy;
use regex::Regex;
use versions::Versioning;

use cache::PluginCache;
pub use script_manager::{InstallType, Script, ScriptManager};

use crate::cmd::cmd;
use crate::config::{MissingRuntimeBehavior, Settings};
use crate::errors::Error::PluginNotInstalled;
use crate::file::changed_within;
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::plugins::script_manager::Script::ParseLegacyFile;
use crate::shorthand::shorthand_to_repository;
use crate::ui::color::{cyan, Color};
use crate::ui::progress_report::ProgressReport;
use crate::ui::prompt;
use crate::{dirs, file};

mod cache;
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
    cache: Option<PluginCache>,
    script_man: ScriptManager,
}

impl Plugin {
    pub fn new(name: &PluginName) -> Self {
        let plugin_path = dirs::PLUGINS.join(name);
        Self {
            name: name.into(),
            cache_path: plugin_path.join(".rtxcache.msgpack"),
            script_man: ScriptManager::new(plugin_path.clone()),
            plugin_path,
            downloads_path: dirs::DOWNLOADS.join(name),
            installs_path: dirs::INSTALLS.join(name),
            cache: None,
        }
    }

    pub fn load(name: &PluginName, settings: &Settings) -> Result<Self> {
        let mut plugin = Self::new(name);
        if plugin.is_installed() {
            plugin.ensure_loaded(settings)?;
        }
        Ok(plugin)
    }

    pub fn load_ensure_installed(name: &PluginName, settings: &Settings) -> Result<Self> {
        let mut plugin = Self::new(name);
        if !plugin.ensure_installed(settings)? {
            Err(PluginNotInstalled(plugin.name.to_string()))?;
        }
        plugin.cache = Some(plugin.get_cache(settings)?);
        Ok(plugin)
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

    pub fn ensure_loaded(&mut self, settings: &Settings) -> Result<()> {
        self.cache = Some(self.get_cache(settings)?);
        Ok(())
    }

    pub fn get_remote_url(&self) -> Option<String> {
        let git = Git::new(self.plugin_path.to_path_buf());
        git.get_remote_url()
    }

    pub fn install(
        &mut self,
        settings: &Settings,
        repository: Option<&str>,
        mut pr: ProgressReport,
    ) -> Result<()> {
        static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
            ProgressStyle::with_template("{prefix}{wide_msg} {spinner:.blue} {elapsed:.dim.italic}")
                .unwrap()
        });
        pr.set_style(PROG_TEMPLATE.clone());
        pr.set_prefix(format!(
            "{} {} ",
            COLOR.dimmed("rtx"),
            COLOR.cyan(&self.name)
        ));
        pr.enable_steady_tick();
        let repository = repository
            .or_else(|| shorthand_to_repository(&self.name))
            .ok_or_else(|| eyre!("No repository found for plugin {}", self.name))?;
        debug!("install {} {:?}", self.name, repository);
        if self.is_installed() {
            pr.set_message("uninstalling existing plugin".into());
            self.uninstall()?;
        }

        let git = Git::new(self.plugin_path.to_path_buf());
        pr.set_message(format!("cloning {repository}"));
        git.clone(repository)?;
        pr.set_message("loading plugin".into());
        self.ensure_loaded(settings)?;
        let sha = git.current_sha_short()?;
        pr.finish_with_message(format!(
            "{} {repository}@{}",
            COLOR.green("✓"),
            COLOR.bright_yellow(&sha),
        ));
        Ok(())
    }

    pub fn ensure_installed(&mut self, settings: &Settings) -> Result<bool> {
        static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stderr));
        if self.is_installed() {
            return Ok(true);
        }

        match shorthand_to_repository(&self.name) {
            Some(repo) => match settings.missing_runtime_behavior {
                MissingRuntimeBehavior::AutoInstall => {
                    self.install(settings, Some(repo), ProgressReport::new(settings.verbose))?;
                    Ok(true)
                }
                MissingRuntimeBehavior::Prompt => {
                    match prompt::prompt_for_install(&format!("plugin {}", COLOR.cyan(&self.name)))
                    {
                        true => {
                            self.install(
                                settings,
                                Some(repo),
                                ProgressReport::new(settings.verbose),
                            )?;
                            Ok(true)
                        }
                        false => Ok(false),
                    }
                }
                MissingRuntimeBehavior::Warn => {
                    warn!("{}", PluginNotInstalled(self.name.clone()));
                    Ok(false)
                }
                MissingRuntimeBehavior::Ignore => {
                    debug!("{}", PluginNotInstalled(self.name.clone()));
                    Ok(false)
                }
            },
            None => match settings.missing_runtime_behavior {
                MissingRuntimeBehavior::Ignore => Ok(false),
                _ => {
                    warn!("Plugin not found: {}", COLOR.cyan(&self.name));
                    Ok(false)
                }
            },
        }
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
                    cyan(Stderr, &dir.to_string_lossy())
                )
            })
        };

        rmdir(&self.downloads_path)?;
        rmdir(&self.installs_path)?;
        rmdir(&self.plugin_path)?;

        Ok(())
    }

    pub fn latest_version(&self, query: &str) -> Option<String> {
        let matches = self.list_versions_matching(query);
        match matches.contains(&query.to_string()) {
            true => Some(query.to_string()),
            false => matches.last().map(|v| v.to_string()),
        }
    }

    pub fn list_versions_matching(&self, query: &str) -> Vec<String> {
        let mut query = query;
        if query == "latest" {
            query = "[0-9]";
        }
        lazy_static! {
            static ref VERSION_REGEX: Regex = Regex::new(
                r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|(a|b|c)[0-9]+|snapshot|master)"
            ).unwrap();
        }
        let query_regex =
            Regex::new((String::from(r"^\s*") + query).as_str()).expect("error parsing regex");
        self.cache
            .as_ref()
            .expect("plugin not loaded")
            .versions
            .iter()
            .filter(|v| !VERSION_REGEX.is_match(v))
            .filter(|v| query_regex.is_match(v))
            .cloned()
            .collect_vec()
    }

    pub fn legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        Ok(self.get_cache(settings)?.legacy_filenames)
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

    pub fn list_remote_versions(&self, settings: &Settings) -> Result<Vec<String>> {
        self.clear_cache();
        let cache = self.get_cache(settings)?;

        Ok(cache.versions)
    }

    pub fn get_aliases(&self) -> IndexMap<String, String> {
        self.cache
            .as_ref()
            .expect("plugin not loaded")
            .aliases
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
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
            .run()?;
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

    fn get_cache(&self, settings: &Settings) -> Result<PluginCache> {
        if let Some(cache) = self.cache.as_ref() {
            return Ok(cache.clone());
        }
        if !self.is_installed() {
            return Err(PluginNotInstalled(self.name.clone()).into());
        }
        let cp = &self.cache_path;
        // TODO: put this duration into settings
        let pc = match cp.exists() && changed_within(cp, Duration::from_secs(60 * 60 * 24))? {
            true => PluginCache::parse(cp)?,
            false => {
                let pc = self.build_cache(settings)?;
                pc.write(cp).unwrap_or_else(|e| {
                    warn!(
                        "Failed to write plugin cache to {}: {}",
                        cp.to_string_lossy(),
                        e
                    );
                });

                pc
            }
        };

        Ok(pc)
    }

    fn build_cache(&self, settings: &Settings) -> Result<PluginCache> {
        Ok(PluginCache {
            versions: self
                .fetch_remote_versions(settings)
                .wrap_err_with(|| eyre!("fetching remote versions for {}", self.name))?,
            legacy_filenames: self
                .fetch_legacy_filenames(settings)
                .wrap_err_with(|| eyre!("fetching legacy filenames for {}", self.name))?,
            aliases: self
                .fetch_aliases(settings)
                .wrap_err_with(|| eyre!("fetching aliases for {}", self.name))?,
        })
    }

    fn clear_cache(&self) {
        if self.cache_path.exists() {
            remove_file(&self.cache_path).unwrap_or_else(|e| {
                debug!("failed to remove cache file: {}", e);
            });
        }
    }

    fn fetch_legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        if !self.script_man.script_exists(&Script::ListLegacyFilenames) {
            return Ok(vec![]);
        }
        Ok(self
            .script_man
            .read(Script::ListLegacyFilenames, settings.verbose)?
            .split_whitespace()
            .map(|v| v.into())
            .collect())
    }

    fn fetch_aliases(&self, settings: &Settings) -> Result<Vec<(String, String)>> {
        if !self.script_man.script_exists(&Script::ListAliases) {
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
        dirs::LEGACY_CACHE
            .join(&self.name)
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

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stderr));

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, env};

    use super::*;

    #[test]
    fn test_legacy_gemfile() {
        assert_cli!("plugin", "add", "ruby");
        let settings = Settings::default();
        let plugin = Plugin::load(&PluginName::from("ruby"), &settings).unwrap();
        let gemfile = env::HOME.join("fixtures/Gemfile");
        let version = plugin.parse_legacy_file(&gemfile, &settings).unwrap();
        assert_str_eq!(version, "3.0.5");

        // do it again to test the cache
        let version = plugin.parse_legacy_file(&gemfile, &settings).unwrap();
        assert_str_eq!(version, "3.0.5");
    }

    #[test]
    fn test_exact_match() {
        assert_cli!("plugin", "add", "python");
        let settings = Settings::default();
        let plugin = Plugin::load(&PluginName::from("python"), &settings).unwrap();
        let version = plugin.latest_version("3.9.1").unwrap();
        assert_str_eq!(version, "3.9.1");
    }
}
