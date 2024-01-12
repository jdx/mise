use std::collections::{BTreeSet, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

#[allow(unused_imports)]
use confique::env::parse::{list_by_colon, list_by_comma};
use confique::{Config, Partial};
use miette::{IntoDiagnostic, Result};
use once_cell::sync::Lazy;
use serde::ser::Error;
use serde_derive::{Deserialize, Serialize};

use crate::{env, file};

#[derive(Config, Debug, Clone, Serialize)]
#[config(partial_attr(derive(Clone, Serialize, Default)))]
#[config(partial_attr(serde(deny_unknown_fields)))]
pub struct Settings {
    #[config(env = "MISE_ALL_COMPILE", default = false)]
    pub all_compile: bool,
    #[config(env = "MISE_ALWAYS_KEEP_DOWNLOAD", default = false)]
    pub always_keep_download: bool,
    #[config(env = "MISE_ALWAYS_KEEP_INSTALL", default = false)]
    pub always_keep_install: bool,
    #[config(env = "MISE_ASDF_COMPAT", default = false)]
    pub asdf_compat: bool,
    #[config(env = "MISE_COLOR", default = true)]
    pub color: bool,
    #[config(env = "MISE_DISABLE_DEFAULT_SHORTHANDS", default = false)]
    pub disable_default_shorthands: bool,
    #[config(env = "MISE_DISABLE_TOOLS", default = [], parse_env = list_by_comma)]
    pub disable_tools: BTreeSet<String>,
    #[config(env = "MISE_EXPERIMENTAL", default = false)]
    pub experimental: bool,
    #[config(env = "MISE_JOBS", default = 4)]
    pub jobs: usize,
    #[config(env = "MISE_LEGACY_VERSION_FILE", default = true)]
    pub legacy_version_file: bool,
    #[config(env = "MISE_LEGACY_VERSION_FILE_DISABLE_TOOLS", default = [], parse_env = list_by_comma)]
    pub legacy_version_file_disable_tools: BTreeSet<String>,
    #[config(env = "MISE_NODE_COMPILE", default = false)]
    pub node_compile: bool,
    #[config(env = "MISE_NOT_FOUND_AUTO_INSTALL", default = true)]
    pub not_found_auto_install: bool,
    #[config(env = "MISE_PARANOID", default = false)]
    pub paranoid: bool,
    #[config(env = "MISE_PLUGIN_AUTOUPDATE_LAST_CHECK_DURATION", default = "7d")]
    pub plugin_autoupdate_last_check_duration: String,
    #[config(env = "MISE_PYTHON_COMPILE", default = false)]
    pub python_compile: bool,
    #[config(env = "MISE_PYTHON_DEFAULT_PACKAGES_FILE")]
    pub python_default_packages_file: Option<PathBuf>,
    #[config(env = "MISE_PYTHON_PATCH_URL")]
    pub python_patch_url: Option<String>,
    #[config(env = "MISE_PYTHON_PATCHES_DIRECTORY")]
    pub python_precompiled_os: Option<String>,
    #[config(env = "MISE_PYTHON_PRECOMPILED_ARCH")]
    pub python_patches_directory: Option<PathBuf>,
    #[config(env = "MISE_PYTHON_PRECOMPILED_OS")]
    pub python_precompiled_arch: Option<String>,
    #[config(
        env = "MISE_PYENV_REPO",
        default = "https://github.com/pyenv/pyenv.git"
    )]
    pub python_pyenv_repo: String,
    #[config(env = "MISE_PYTHON_VENV_AUTO_CREATE", default = false)]
    pub python_venv_auto_create: bool,
    #[config(env = "MISE_RAW", default = false)]
    pub raw: bool,
    #[config(env = "MISE_SHORTHANDS_FILE")]
    pub shorthands_file: Option<PathBuf>,
    #[config(env = "MISE_TASK_OUTPUT")]
    pub task_output: Option<String>,
    #[config(env = "MISE_TRUSTED_CONFIG_PATHS", default = [], parse_env = list_by_colon)]
    pub trusted_config_paths: BTreeSet<PathBuf>,
    #[config(env = "MISE_QUIET", default = false)]
    pub quiet: bool,
    #[config(env = "MISE_VERBOSE", default = false)]
    pub verbose: bool,
    #[config(env = "MISE_YES", default = false)]
    pub yes: bool,

    // hidden settings
    #[config(env = "CI", default = false)]
    pub ci: bool,
    #[config(env = "MISE_CD")]
    pub cd: Option<String>,
    #[config(env = "MISE_DEBUG", default = false)]
    pub debug: bool,
    #[config(env = "MISE_ENV_FILE")]
    pub env_file: Option<PathBuf>,
    #[config(env = "MISE_TRACE", default = false)]
    pub trace: bool,
    #[config(env = "MISE_LOG_LEVEL", default = "info")]
    pub log_level: String,
}

pub type SettingsPartial = <Settings as Config>::Partial;

static SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);
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
        if let Some(settings) = SETTINGS.read().unwrap().as_ref() {
            return Ok(settings.clone());
        }
        let file_1 = Self::config_settings().unwrap_or_else(|e| {
            eprintln!("Error loading settings file: {}", e);
            Default::default()
        });
        let file_2 = Self::deprecated_settings_file().unwrap_or_else(|e| {
            eprintln!("Error loading settings file: {}", e);
            Default::default()
        });
        let mut settings = Self::builder()
            .preloaded(CLI_SETTINGS.lock().unwrap().clone().unwrap_or_default())
            .env()
            .preloaded(file_1)
            .preloaded(file_2)
            .preloaded(DEFAULT_SETTINGS.clone())
            .load()
            .into_diagnostic()?;
        if let Some(cd) = &settings.cd {
            static ORIG_PATH: Lazy<std::io::Result<PathBuf>> = Lazy::new(env::current_dir);
            let mut cd = PathBuf::from(cd);
            if cd.is_relative() {
                cd = ORIG_PATH.as_ref().into_diagnostic()?.join(cd);
            }
            env::set_current_dir(cd).into_diagnostic()?;
        }
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
            settings.log_level = "warn".to_string();
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
        }
        if settings.ci {
            settings.yes = true;
        }
        if settings.all_compile {
            settings.node_compile = true;
            settings.python_compile = true;
        }
        let settings = Arc::new(settings);
        *SETTINGS.write().unwrap() = Some(settings.clone());
        Ok(settings)
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
        if let Some(cd) = m.get_one::<String>("cd") {
            s.cd = Some(cd.to_string());
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
        if !global_config.exists() {
            return Ok(Default::default());
        }
        let raw = file::read_to_string(global_config)?;
        let settings_file: SettingsFile = toml::from_str(&raw).into_diagnostic()?;
        Ok(settings_file.settings)
    }

    fn deprecated_settings_file() -> Result<SettingsPartial> {
        // TODO: show warning and merge with config file in a few weeks
        let settings_file = &*env::MISE_SETTINGS_FILE;
        if !settings_file.exists() {
            return Ok(Default::default());
        }
        Self::from_file(settings_file)
    }

    pub fn from_file(path: &PathBuf) -> Result<SettingsPartial> {
        let raw = file::read_to_string(path)?;
        let settings: SettingsPartial = toml::from_str(&raw).into_diagnostic()?;
        Ok(settings)
    }

    pub fn hidden_configs() -> HashSet<&'static str> {
        static HIDDEN_CONFIGS: Lazy<HashSet<&'static str>> =
            Lazy::new(|| ["ci", "cd", "debug", "env_file", "trace", "log_level"].into());
        HIDDEN_CONFIGS.clone()
    }

    pub fn reset(cli_settings: Option<SettingsPartial>) {
        *CLI_SETTINGS.lock().unwrap() = cli_settings;
        *SETTINGS.write().unwrap() = None;
    }

    pub fn ensure_experimental(&self) -> Result<()> {
        let msg =
            "This command is experimental. Enable it with `mise settings set experimental true`";
        ensure!(self.experimental, msg);
        Ok(())
    }

    pub fn trusted_config_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.trusted_config_paths.iter().map(file::replace_path)
    }
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match serde_json::to_string_pretty(self) {
            Ok(s) => write!(f, "{}", s),
            Err(e) => std::fmt::Result::Err(std::fmt::Error::custom(e)),
        }
    }
}
