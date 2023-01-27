use std::fmt::{Display, Formatter};
use std::fs;
use std::fs::{remove_file, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use color_eyre::eyre::WrapErr;
use color_eyre::eyre::{eyre, Result};
use duct::Expression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use itertools::Itertools;
use lazy_static::lazy_static;
use owo_colors::{OwoColorize, Stream};
use regex::Regex;
use serde_derive::{Deserialize, Serialize};
use versions::Mess;

use crate::cli::args::runtime::RuntimeArg;
use crate::cmd::cmd;
use crate::config::settings::{MissingRuntimeBehavior, Settings};
use crate::errors::Error::PluginNotInstalled;
use crate::file::changed_within;
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::shorthand_repository::ShorthandRepo;
use crate::ui::prompt::prompt;
use crate::{dirs, env, file};
use crate::{fake_asdf, ui};

pub type PluginName = String;

#[derive(Debug, Clone)]
pub struct Plugin {
    pub name: PluginName,
    pub plugin_path: PathBuf,
    cache_path: PathBuf,
    downloads_path: PathBuf,
    installs_path: PathBuf,
    cache: Option<PluginCache>,
}

#[derive(Debug, Clone)]
pub enum PluginSource {
    ToolVersions(PathBuf),
    RtxRc(PathBuf),
    LegacyVersionFile(PathBuf),
    Argument(RuntimeArg),
    Environment(String, String),
}

impl Plugin {
    pub fn new(name: &PluginName) -> Self {
        let plugin_path = dirs::PLUGINS.join(name);
        Self {
            name: name.into(),
            cache_path: plugin_path.join(".rtxcache.msgpack.gz"),
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

    pub fn install(&self, repository: &String) -> Result<()> {
        debug!("install {} {:?}", self.name, repository);
        eprint!(
            "rtx: Installing plugin {}...",
            self.name.if_supports_color(Stream::Stderr, |t| t.cyan())
        );
        fake_asdf::setup(&fake_asdf::get_path(dirs::ROOT.as_path()))?;

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
                MissingRuntimeBehavior::Prompt => match prompt_for_install(&self.name) {
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
        let git = Git::new(self.plugin_path.to_path_buf());
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

    fn run_script(&self, script: &str, args: Vec<String>) -> Result<Expression> {
        if !self.is_installed() {
            return Err(PluginNotInstalled(self.name.clone()).into());
        }
        Ok(cmd(self.plugin_path.join("bin").join(script), args)
            .env("RTX", "1")
            .env("RTX_EXE", env::RTX_EXE.as_path()))
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        let stdout = self
            .run_script("list-all", vec![])?
            .read()
            .wrap_err_with(|| eyre!("error running list-all script for {}", self.name))?;
        let versions = stdout.split_whitespace().map(|v| v.into()).collect();

        Ok(versions)
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
        if !self
            .plugin_path
            .join("bin")
            .join("list-legacy-filenames")
            .is_file()
        {
            return Ok(vec![]);
        }
        let stdout = self
            .run_script("list-legacy-filenames", vec![])?
            .read()
            .wrap_err_with(|| eyre!("running list-legacy-filenames script for {}", self.name))?;
        let versions = stdout.split_whitespace().map(|v| v.into()).collect();

        Ok(versions)
    }

    fn fetch_aliases(&self) -> Result<Vec<(String, String)>> {
        if !self.plugin_path.join("bin").join("list-aliases").is_file() {
            return Ok(vec![]);
        }
        let stdout = self
            .run_script("list-aliases", vec![])?
            .read()
            .wrap_err_with(|| eyre!("running list-aliases script for {}", self.name))?;
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
            .run_script(
                "parse-legacy-file",
                vec![legacy_file.to_string_lossy().into()],
            )?
            .read()
            .wrap_err_with(|| {
                eyre!(
                    "error parsing legacy file: {}",
                    legacy_file.to_string_lossy()
                )
            })?
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

fn prompt_for_install(thing: &str) -> bool {
    match ui::is_tty() {
        true => {
            eprint!(
                "rtx: Would you like to install plugin {}? [Y/n] ",
                thing.cyan()
            );
            matches!(prompt().to_lowercase().as_str(), "" | "y" | "yes")
        }
        false => false,
    }
}

impl PartialEq for Plugin {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct PluginCache {
    versions: Vec<String>,
    legacy_filenames: Vec<String>,
    aliases: Vec<(String, String)>,
}

impl PluginCache {
    fn parse(path: &Path) -> Result<Self> {
        trace!("reading plugin cache from {}", path.to_string_lossy());
        let mut gz = GzDecoder::new(File::open(path)?);
        let mut bytes = Vec::new();
        gz.read_to_end(&mut bytes)?;
        Ok(rmp_serde::from_slice(&bytes)?)
    }

    fn write(&self, path: &Path) -> Result<()> {
        trace!("writing plugin cache to {}", path.to_string_lossy());
        let mut gz = GzEncoder::new(File::create(path)?, Compression::fast());
        gz.write_all(&rmp_serde::to_vec_named(self)?[..])?;

        Ok(())
    }
}

impl Display for PluginSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            PluginSource::ToolVersions(path) => write!(f, "{}", display_path(path)),
            PluginSource::RtxRc(path) => write!(f, "{}", display_path(path)),
            PluginSource::LegacyVersionFile(path) => write!(f, "{}", display_path(path)),
            PluginSource::Argument(arg) => write!(f, "--runtime {arg}"),
            PluginSource::Environment(k, v) => write!(f, "{k}={v}"),
        }
    }
}

fn display_path(path: &Path) -> String {
    let home = dirs::HOME.to_string_lossy();
    path.to_string_lossy().replace(home.as_ref(), "~")
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;

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
