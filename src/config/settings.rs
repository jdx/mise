use crate::config::{system_config_files, DEFAULT_CONFIG_FILENAMES};
use crate::file::FindUp;
use crate::{config, dirs, env, file};
#[allow(unused_imports)]
use confique::env::parse::{list_by_colon, list_by_comma};
use confique::{Config, Partial};
use eyre::{bail, Result};
use indexmap::{indexmap, IndexMap};
use once_cell::sync::Lazy;
use serde::ser::Error;
use serde_derive::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::iter::once;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use url::Url;

pub static SETTINGS: Lazy<Arc<Settings>> = Lazy::new(Settings::get);

// settings are generated from settings.toml in the project root
// make sure you run `mise run render` after updating settings.toml
include!(concat!(env!("OUT_DIR"), "/settings.rs"));

pub enum SettingsType {
    Bool,
    String,
    Integer,
    Duration,
    Path,
    Url,
    ListString,
    ListPath,
}

pub struct SettingsMeta {
    // pub key: String,
    pub type_: SettingsType,
    // pub description: String,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, Default, strum::EnumString, strum::Display,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SettingsStatusMissingTools {
    /// never show the warning
    Never,
    /// hide this warning if the user hasn't installed at least 1 version of the tool before
    #[default]
    IfOtherVersionsInstalled,
    /// always show the warning if tools are missing
    Always,
}

pub type SettingsPartial = <Settings as Config>::Partial;

static BASE_SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);
static CLI_SETTINGS: Mutex<Option<SettingsPartial>> = Mutex::new(None);
static DEFAULT_SETTINGS: Lazy<SettingsPartial> = Lazy::new(|| {
    let mut s = SettingsPartial::empty();
    s.python_default_packages_file = Some(env::HOME.join(".default-python-packages"));
    if let Some("alpine" | "nixos") = env::LINUX_DISTRO.as_ref().map(|s| s.as_str()) {
        if !cfg!(test) {
            s.all_compile = Some(true);
        }
    }
    s
});

#[derive(Serialize, Deserialize)]
pub struct SettingsFile {
    #[serde(default)]
    pub settings: SettingsPartial,
}

impl Settings {
    pub fn get() -> Arc<Self> {
        Self::try_get().unwrap()
    }
    pub fn try_get() -> Result<Arc<Self>> {
        if let Some(settings) = BASE_SETTINGS.read().unwrap().as_ref() {
            return Ok(settings.clone());
        }

        // Initial pass to obtain cd option
        let mut sb = Self::builder()
            .preloaded(CLI_SETTINGS.lock().unwrap().clone().unwrap_or_default())
            .env();

        let mut settings = sb.load()?;
        if let Some(mut cd) = settings.cd {
            static ORIG_PATH: Lazy<std::io::Result<PathBuf>> = Lazy::new(env::current_dir);
            if cd.is_relative() {
                cd = ORIG_PATH.as_ref()?.join(cd);
            }
            env::set_current_dir(cd)?;
        }

        // Reload settings after current directory option processed
        sb = Self::builder()
            .preloaded(CLI_SETTINGS.lock().unwrap().clone().unwrap_or_default())
            .env();
        for file in Self::all_settings_files() {
            sb = sb.preloaded(file);
        }
        sb = sb.preloaded(DEFAULT_SETTINGS.clone());

        settings = sb.load()?;
        if settings.raw {
            settings.jobs = 1;
        }
        if settings.debug {
            settings.log_level = "debug".to_string();
        }
        if settings.trace {
            settings.log_level = "trace".to_string();
        }
        if settings.quiet {
            settings.log_level = "error".to_string();
        }
        if settings.log_level == "trace" || settings.log_level == "debug" {
            settings.verbose = true;
            settings.debug = true;
            if settings.log_level == "trace" {
                settings.trace = true;
            }
        }
        if settings.verbose {
            settings.quiet = false;
            if settings.log_level != "trace" {
                settings.log_level = "debug".to_string();
            }
        }
        if !settings.color {
            console::set_colors_enabled(false);
            console::set_colors_enabled_stderr(false);
        } else if *env::COLOR_NONTTY_OK {
            console::set_colors_enabled(true);
            console::set_colors_enabled_stderr(true);
        }
        if settings.ci {
            settings.yes = true;
        }
        if settings.all_compile {
            settings.node.compile = Some(true);
            if settings.python.compile.is_none() {
                settings.python.compile = Some(true);
            }
        }
        settings.set_hidden_configs();
        let settings = Arc::new(settings);
        *BASE_SETTINGS.write().unwrap() = Some(settings.clone());
        trace!("Settings: {:#?}", settings);
        Ok(settings)
    }

    /// Sets deprecated settings to new names
    fn set_hidden_configs(&mut self) {
        if let Some(false) = self.asdf {
            self.disable_backends.push("asdf".to_string());
        }
        if let Some(false) = self.vfox {
            self.disable_backends.push("vfox".to_string());
        }
        if let Some(disable_default_shorthands) = self.disable_default_shorthands {
            self.disable_default_registry = disable_default_shorthands;
        }
        if let Some(cargo_binstall) = self.cargo_binstall {
            self.cargo.binstall = cargo_binstall;
        }
        if let Some(pipx_uvx) = self.pipx_uvx {
            self.pipx.uvx = pipx_uvx;
        }
        if let Some(python_compile) = self.python_compile {
            self.python.compile = Some(python_compile);
        }
        if let Some(python_default_packages_file) = &self.python_default_packages_file {
            self.python.default_packages_file = Some(python_default_packages_file.clone());
        }
        if let Some(python_patch_url) = &self.python_patch_url {
            self.python.patch_url = Some(python_patch_url.clone());
        }
        if let Some(python_patches_directory) = &self.python_patches_directory {
            self.python.patches_directory = Some(python_patches_directory.clone());
        }
        if let Some(python_precompiled_arch) = &self.python_precompiled_arch {
            self.python.precompiled_arch = Some(python_precompiled_arch.clone());
        }
        if let Some(python_precompiled_os) = &self.python_precompiled_os {
            self.python.precompiled_os = Some(python_precompiled_os.clone());
        }
        if let Some(python_pyenv_repo) = &self.python_pyenv_repo {
            self.python.pyenv_repo = python_pyenv_repo.clone();
        }
        if let Some(python_venv_auto_create) = self.python_venv_auto_create {
            self.python.venv_auto_create = python_venv_auto_create;
        }
    }

    pub fn add_cli_matches(m: &clap::ArgMatches) {
        let mut s = SettingsPartial::empty();
        for arg in &*env::ARGS.read().unwrap() {
            if arg == "--" {
                break;
            }
            if arg == "--raw" {
                s.raw = Some(true);
            }
        }
        if let Some(cd) = m.get_one::<PathBuf>("cd") {
            s.cd = Some(cd.clone());
        }
        if let Some(true) = m.get_one::<bool>("yes") {
            s.yes = Some(true);
        }
        if let Some(true) = m.get_one::<bool>("quiet") {
            s.quiet = Some(true);
        }
        if let Some(true) = m.get_one::<bool>("trace") {
            s.log_level = Some("trace".to_string());
        }
        if let Some(true) = m.get_one::<bool>("debug") {
            s.log_level = Some("debug".to_string());
        }
        if let Some(log_level) = m.get_one::<String>("log-level") {
            s.log_level = Some(log_level.to_string());
        }
        if *m.get_one::<u8>("verbose").unwrap() > 0 {
            s.verbose = Some(true);
        }
        if *m.get_one::<u8>("verbose").unwrap() > 1 {
            s.log_level = Some("trace".to_string());
        }
        Self::reset(Some(s));
    }

    fn config_settings() -> Result<SettingsPartial> {
        let global_config = &*env::MISE_GLOBAL_CONFIG_FILE;
        let filename = global_config
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        // if the file doesn't exist or is actually a .tool-versions config
        if !global_config.exists()
            || filename == *env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME
            || filename == ".tool-versions"
        {
            return Ok(Default::default());
        }
        Self::parse_settings_file(global_config)
    }

    fn deprecated_settings_file() -> Result<SettingsPartial> {
        // TODO: show warning and merge with config file in a few weeks
        let settings_file = &*env::MISE_SETTINGS_FILE;
        if !settings_file.exists() {
            return Ok(Default::default());
        }
        Self::from_file(settings_file)
    }

    fn parse_settings_file(path: &PathBuf) -> Result<SettingsPartial> {
        let raw = file::read_to_string(path)?;
        let settings_file: SettingsFile = toml::from_str(&raw)?;
        Ok(settings_file.settings)
    }

    fn all_settings_files() -> Vec<SettingsPartial> {
        config::load_config_paths(&DEFAULT_CONFIG_FILENAMES)
            .iter()
            .filter(|p| {
                let filename = p.file_name().unwrap_or_default().to_string_lossy();
                filename != *env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME
                    && filename != ".tool-versions"
            })
            .map(Self::parse_settings_file)
            .chain(once(Self::config_settings()))
            .chain(once(Self::deprecated_settings_file()))
            .chain(system_config_files().iter().map(Self::parse_settings_file))
            .filter_map(|cfg| match cfg {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    eprintln!("Error loading settings file: {}", e);
                    None
                }
            })
            .collect()
    }

    pub fn from_file(path: &PathBuf) -> Result<SettingsPartial> {
        let raw = file::read_to_string(path)?;
        let settings: SettingsPartial = toml::from_str(&raw)?;
        Ok(settings)
    }

    pub fn hidden_configs() -> &'static HashSet<&'static str> {
        static HIDDEN_CONFIGS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
            [
                "ci",
                "cd",
                "debug",
                "env_file",
                "trace",
                "log_level",
                "python_venv_auto_create",
            ]
            .into()
        });
        &HIDDEN_CONFIGS
    }

    pub fn reset(cli_settings: Option<SettingsPartial>) {
        *CLI_SETTINGS.lock().unwrap() = cli_settings;
        *BASE_SETTINGS.write().unwrap() = None;
    }

    pub fn ensure_experimental(&self, what: &str) -> Result<()> {
        if !self.experimental {
            bail!("{what} is experimental. Enable it with `mise settings set experimental true`");
        }
        Ok(())
    }

    pub fn trusted_config_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.trusted_config_paths.iter().map(file::replace_path)
    }

    pub fn global_tools_file(&self) -> PathBuf {
        env::var_path("MISE_GLOBAL_CONFIG_FILE")
            .or_else(|| env::var_path("MISE_CONFIG_FILE"))
            .unwrap_or_else(|| {
                if self.asdf_compat {
                    env::HOME.join(&*env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME)
                } else {
                    dirs::CONFIG.join("config.toml")
                }
            })
    }

    pub fn env_files(&self) -> Vec<PathBuf> {
        let mut files = vec![];
        if let Some(cwd) = &*dirs::CWD {
            if let Some(env_file) = &self.env_file {
                let env_file = env_file.to_string_lossy().to_string();
                for p in FindUp::new(cwd, &[env_file]) {
                    files.push(p);
                }
            }
        }
        files.into_iter().rev().collect()
    }

    pub fn as_dict(&self) -> eyre::Result<toml::Table> {
        let s = toml::to_string(self)?;
        let table = toml::from_str(&s)?;
        Ok(table)
    }

    pub fn cache_prune_age_duration(&self) -> Option<Duration> {
        if self.cache_prune_age == "0" {
            return None;
        }
        Some(humantime::parse_duration(&self.cache_prune_age).unwrap())
    }

    pub fn fetch_remote_versions_timeout(&self) -> Duration {
        humantime::parse_duration(&self.fetch_remote_versions_timeout).unwrap()
    }

    /// duration that remote version cache is kept for
    /// for "fast" commands (represented by PREFER_STALE), these are always
    /// cached. For "slow" commands like `mise ls-remote` or `mise install`:
    /// - if MISE_FETCH_REMOTE_VERSIONS_CACHE is set, use that
    /// - if MISE_FETCH_REMOTE_VERSIONS_CACHE is not set, use HOURLY
    pub fn fetch_remote_versions_cache(&self) -> Option<Duration> {
        if *env::PREFER_STALE {
            None
        } else {
            Some(humantime::parse_duration(&self.fetch_remote_versions_cache).unwrap())
        }
    }
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match toml::to_string_pretty(self) {
            Ok(s) => write!(f, "{}", s),
            Err(e) => Err(std::fmt::Error::custom(e)),
        }
    }
}

pub fn ensure_experimental(what: &str) -> Result<()> {
    Settings::get().ensure_experimental(what)
}

pub const DEFAULT_NODE_MIRROR_URL: &str = "https://nodejs.org/dist/";

impl SettingsNode {
    pub fn mirror_url(&self) -> Url {
        let s = self
            .mirror_url
            .clone()
            .or(env::var("NODE_BUILD_MIRROR_URL").ok())
            .unwrap_or_else(|| DEFAULT_NODE_MIRROR_URL.to_string());
        Url::parse(&s).unwrap()
    }
}

impl SettingsStatus {
    pub fn missing_tools(&self) -> SettingsStatusMissingTools {
        SettingsStatusMissingTools::from_str(&self.missing_tools).unwrap()
    }
}
