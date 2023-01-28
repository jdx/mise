use std::fs;
use std::fs::remove_file;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use color_eyre::eyre::WrapErr;
use color_eyre::eyre::{eyre, Result};
use itertools::Itertools;
use lazy_static::lazy_static;
use owo_colors::{OwoColorize, Stream};
use regex::Regex;
use versions::Mess;

use cache::PluginCache;
pub use script_manager::{InstallType, Script, ScriptManager};

use crate::cmd::cmd;
use crate::config::{MissingRuntimeBehavior, Settings};
use crate::errors::Error::PluginNotInstalled;
use crate::file::changed_within;
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::plugins::script_manager::Script::ParseLegacyFile;
use crate::shorthand_repository::ShorthandRepo;
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
            cache_path: plugin_path.join(".rtxcache.msgpack.gz"),
            script_man: ScriptManager::new(plugin_path.clone()),
            plugin_path,
            downloads_path: dirs::DOWNLOADS.join(name),
            installs_path: dirs::INSTALLS.join(name),
            cache: None,
        }
    }

    pub fn load(name: &PluginName) -> Result<Self> {
        let mut plugin = Self::new(name);
        if plugin.is_installed() {
            plugin.cache = Some(plugin.get_cache()?);
        }
        Ok(plugin)
    }

    pub fn load_ensure_installed(name: &PluginName, settings: &Settings) -> Result<Self> {
        let mut plugin = Self::new(name);
        if !plugin.ensure_installed(settings)? {
            Err(PluginNotInstalled(plugin.name.to_string()))?;
        }
        plugin.cache = Some(plugin.get_cache()?);
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

    pub fn get_remote_url(&self) -> Option<String> {
        let git = Git::new(self.plugin_path.to_path_buf());
        git.get_remote_url()
    }

    pub fn install(&self, repository: &String) -> Result<()> {
        debug!("install {} {:?}", self.name, repository);
        eprint!(
            "rtx: Installing plugin {}...",
            self.name.if_supports_color(Stream::Stderr, |t| t.cyan())
        );

        if self.is_installed() {
            self.uninstall()?;
        }

        let git = Git::new(self.plugin_path.to_path_buf());
        git.clone(repository)?;
        eprintln!(" done");

        Ok(())
    }

    pub fn ensure_installed(&self, settings: &Settings) -> Result<bool> {
        if self.is_installed() {
            return Ok(true);
        }

        let shr = ShorthandRepo::new(settings);
        match shr.lookup(&self.name) {
            Ok(repo) => match settings.missing_runtime_behavior {
                MissingRuntimeBehavior::AutoInstall => {
                    self.install(&repo)?;
                    Ok(true)
                }
                MissingRuntimeBehavior::Prompt => match prompt::prompt_for_install(&self.name) {
                    true => {
                        self.install(&repo)?;
                        Ok(true)
                    }
                    false => Ok(false),
                },
                MissingRuntimeBehavior::Warn => {
                    warn!("{}", PluginNotInstalled(self.name.clone()));
                    Ok(false)
                }
                MissingRuntimeBehavior::Ignore => {
                    debug!("{}", PluginNotInstalled(self.name.clone()));
                    Ok(false)
                }
            },
            Err(err) => match settings.missing_runtime_behavior {
                MissingRuntimeBehavior::Ignore => Ok(false),
                _ => {
                    warn!("{}", err);
                    Ok(false)
                }
            },
        }
    }

    pub fn update(&self, gitref: Option<String>) -> Result<()> {
        let plugin_path = self.plugin_path.to_path_buf();
        if plugin_path.is_symlink() {
            warn!("Plugin: {} is a symlink, not updating", self.name);
            return Ok(());
        }
        let git = Git::new(plugin_path);
        if !git.is_repo() {
            warn!("Plugin {} is not a git repository not updating", self.name);
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
                    dir.to_str()
                        .unwrap()
                        .if_supports_color(Stream::Stderr, |t| t.cyan())
                )
            })
        };

        rmdir(&self.downloads_path)?;
        rmdir(&self.installs_path)?;
        rmdir(&self.plugin_path)?;

        Ok(())
    }

    pub fn latest_version(&self, query: &str) -> Result<Option<String>> {
        let mut query = query;
        if query == "latest" {
            query = "[0-9]";
        }
        lazy_static! {
            static ref VERSION_REGEX: Regex = Regex::new(
                r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|(a|b|c)[0-9]+|snapshot|master)"
            ).unwrap();
        }
        let query = String::from(r"^\s*") + query;
        let query_regex = Regex::new(&query)?;
        let latest = self
            .get_cache()?
            .versions
            .iter()
            .filter(|v| !VERSION_REGEX.is_match(v))
            .filter(|v| query_regex.is_match(v))
            .last()
            .map(|v| v.into());

        Ok(latest)
    }

    pub fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(self.get_cache()?.legacy_filenames)
    }

    pub fn list_installed_versions(&self) -> Result<Vec<String>> {
        Ok(match self.installs_path.exists() {
            true => file::dir_subdirs(&self.installs_path)?
                .iter()
                .map(|v| Mess::new(v).unwrap())
                .sorted()
                .map(|v| v.to_string())
                .collect(),
            false => vec![],
        })
    }

    pub fn list_remote_versions(&self) -> Result<Vec<String>> {
        self.clear_cache();
        let cache = self.get_cache()?;

        Ok(cache.versions)
    }

    pub fn list_aliases(&self) -> Result<Vec<(String, String)>> {
        let cache = self.get_cache()?;
        Ok(cache.aliases)
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

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        Ok(self
            .script_man
            .cmd(Script::ListAll)
            .read()?
            .split_whitespace()
            .map(|v| v.into())
            .collect())
    }

    fn get_cache(&self) -> Result<PluginCache> {
        if let Some(cache) = self.cache.as_ref() {
            return Ok(cache.clone());
        }
        // lazy_static! {
        //     static ref CACHE: Mutex<HashMap<PluginName, Mutex<Arc<PluginCache>>>> =
        //         Mutex::new(HashMap::new());
        // }
        // let mut cache = CACHE.lock().expect("failed to get mutex");
        // let pc = match cache.get(&self.name) {
        //     Some(cached) => cached,
        //     None => {
        if !self.is_installed() {
            return Err(PluginNotInstalled(self.name.clone()).into());
        }
        let cp = &self.cache_path;
        // TODO: put this duration into settings
        let pc = match cp.exists() && changed_within(cp, Duration::from_secs(60 * 60 * 24))? {
            true => PluginCache::parse(cp)?,
            false => {
                let pc = self.build_cache()?;
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

    fn build_cache(&self) -> Result<PluginCache> {
        Ok(PluginCache {
            versions: self
                .fetch_remote_versions()
                .wrap_err_with(|| eyre!("fetching remote versions for {}", self.name))?,
            legacy_filenames: self
                .fetch_legacy_filenames()
                .wrap_err_with(|| eyre!("fetching legacy filenames for {}", self.name))?,
            aliases: self
                .fetch_aliases()
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

    fn fetch_legacy_filenames(&self) -> Result<Vec<String>> {
        if !self.script_man.script_exists(&Script::ListLegacyFilenames) {
            return Ok(vec![]);
        }
        Ok(self
            .script_man
            .read(Script::ListLegacyFilenames)?
            .split_whitespace()
            .map(|v| v.into())
            .collect())
    }

    fn fetch_aliases(&self) -> Result<Vec<(String, String)>> {
        if !self.script_man.script_exists(&Script::ListAliases) {
            return Ok(vec![]);
        }
        let stdout = self.script_man.read(Script::ListAliases)?;
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

    pub fn parse_legacy_file(&self, legacy_file: &Path) -> Result<String> {
        if let Some(cached) = self.fetch_cached_legacy_file(legacy_file)? {
            return Ok(cached);
        }
        trace!("parsing legacy file: {}", legacy_file.to_string_lossy());
        let legacy_version = self
            .script_man
            .read(ParseLegacyFile(legacy_file.to_string_lossy().into()))?
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

#[cfg(test)]
mod test {
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, env};

    use super::*;

    #[test]
    fn test_legacy_gemfile() {
        assert_cli!("plugin", "add", "ruby");
        let plugin = Plugin::load(&PluginName::from("ruby")).unwrap();
        let gemfile = env::HOME.join("fixtures/Gemfile");
        let version = plugin.parse_legacy_file(&gemfile).unwrap();
        assert_str_eq!(version, "3.0.5");

        // do it again to test the cache
        let version = plugin.parse_legacy_file(&gemfile).unwrap();
        assert_str_eq!(version, "3.0.5");
    }
}
