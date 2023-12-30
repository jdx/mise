use confique::env::parse::{list_by_colon, list_by_comma};
use eyre::Result;

use std::collections::{BTreeSet, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, Once, RwLock};

use confique::{Builder, Config, Partial};
use once_cell::sync::Lazy;

use crate::env;
use serde::ser::Error;
use serde_derive::Serialize;

#[derive(Config, Debug, Clone, Serialize)]
#[config(partial_attr(derive(Clone, Serialize)))]
pub struct Settings {
    #[config(env = "RTX_EXPERIMENTAL", default = false)]
    pub experimental: bool,
    #[config(env = "RTX_COLOR", default = true)]
    pub color: bool,
    #[config(env = "RTX_ALWAYS_KEEP_DOWNLOAD", default = false)]
    pub always_keep_download: bool,
    #[config(env = "RTX_ALWAYS_KEEP_INSTALL", default = false)]
    pub always_keep_install: bool,
    #[config(env = "RTX_LEGACY_VERSION_FILE", default = true)]
    pub legacy_version_file: bool,
    #[config(env = "RTX_LEGACY_VERSION_FILE_DISABLE_TOOLS", default = [], parse_env = list_by_comma)]
    pub legacy_version_file_disable_tools: BTreeSet<String>,
    #[config(env = "RTX_PLUGIN_AUTOUPDATE_LAST_CHECK_DURATION", default = "7d")]
    pub plugin_autoupdate_last_check_duration: String,
    #[config(env = "RTX_TRUSTED_CONFIG_PATHS", default = [], parse_env = list_by_colon)]
    pub trusted_config_paths: BTreeSet<PathBuf>,
    #[config(env = "RTX_LOG_LEVEL", default = "info")]
    pub log_level: String,
    #[config(env = "RTX_TRACE", default = false)]
    pub trace: bool,
    #[config(env = "RTX_DEBUG", default = false)]
    pub debug: bool,
    #[config(env = "RTX_VERBOSE", default = false)]
    pub verbose: bool,
    #[config(env = "RTX_QUIET", default = false)]
    pub quiet: bool,
    #[config(env = "RTX_ASDF_COMPAT", default = false)]
    pub asdf_compat: bool,
    #[config(env = "RTX_JOBS", default = 4)]
    pub jobs: usize,
    #[config(env = "RTX_SHORTHANDS_FILE")]
    pub shorthands_file: Option<PathBuf>,
    #[config(env = "RTX_DISABLE_DEFAULT_SHORTHANDS", default = false)]
    pub disable_default_shorthands: bool,
    #[config(env = "RTX_DISABLE_TOOLS", default = [], parse_env = list_by_comma)]
    pub disable_tools: BTreeSet<String>,
    #[config(env = "RTX_RAW", default = false)]
    pub raw: bool,
    #[config(env = "RTX_YES", default = false)]
    pub yes: bool,
    #[config(env = "RTX_TASK_OUTPUT")]
    pub task_output: Option<String>,
    #[config(env = "RTX_NOT_FOUND_AUTO_INSTALL", default = true)]
    pub not_found_auto_install: bool,
    #[config(env = "CI", default = false)]
    pub ci: bool,
    pub cd: Option<String>,
}

pub type SettingsPartial = <Settings as Config>::Partial;

impl Default for Settings {
    fn default() -> Self {
        Settings::default_builder().load().unwrap()
    }
}

static PARTIALS: RwLock<Vec<SettingsPartial>> = RwLock::new(Vec::new());
static SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);

impl Settings {
    pub fn get() -> Arc<Self> {
        Self::try_get().unwrap()
    }
    pub fn try_get() -> Result<Arc<Self>> {
        if let Some(settings) = SETTINGS.read().unwrap().as_ref() {
            return Ok(settings.clone());
        }
        let mut settings = Self::default_builder().load()?;
        if let Some(cd) = &settings.cd {
            static ORIG_PATH: Lazy<std::io::Result<PathBuf>> = Lazy::new(env::current_dir);
            let mut cd = PathBuf::from(cd);
            if cd.is_relative() {
                cd = ORIG_PATH.as_ref()?.join(cd);
            }
            env::set_current_dir(cd)?;
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
        let settings = Arc::new(settings);
        *SETTINGS.write().unwrap() = Some(settings.clone());
        Ok(settings)
    }
    pub fn add_partial(partial: SettingsPartial) {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let mut p = SettingsPartial::empty();
            for arg in &*env::ARGS.read().unwrap() {
                if arg == "--" {
                    break;
                }
                if arg == "--raw" {
                    p.raw = Some(true);
                }
            }
            PARTIALS.write().unwrap().push(p);
        });
        PARTIALS.write().unwrap().push(partial);
        *SETTINGS.write().unwrap() = None;
    }
    pub fn add_cli_matches(m: &clap::ArgMatches) {
        let mut s = SettingsPartial::empty();
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
        Self::add_partial(s);
    }

    pub fn default_builder() -> Builder<Self> {
        let mut b = Self::builder().env();
        for partial in PARTIALS.read().unwrap().iter() {
            b = b.preloaded(partial.clone());
        }
        b
    }
    pub fn hidden_configs() -> HashSet<&'static str> {
        static HIDDEN_CONFIGS: Lazy<HashSet<&'static str>> =
            Lazy::new(|| ["ci", "cd", "debug", "trace", "log_level"].into());
        HIDDEN_CONFIGS.clone()
    }

    #[cfg(test)]
    pub fn reset() {
        PARTIALS.write().unwrap().clear();
        *SETTINGS.write().unwrap() = None;
    }

    pub fn ensure_experimental(&self) -> Result<()> {
        let msg =
            "This command is experimental. Enable it with `rtx settings set experimental true`";
        ensure!(self.experimental, msg);
        Ok(())
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
