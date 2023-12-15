use confique::env::parse::{list_by_colon, list_by_comma};
use eyre::Result;

use std::collections::BTreeSet;
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, Once, RwLock};

use confique::{Builder, Config, Partial};
use log::LevelFilter;
use serde::ser::Error;
use serde_derive::Serialize;

use crate::env;

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
        if settings.raw {
            settings.verbose = true;
            settings.jobs = 1;
        }
        if !settings.color {
            console::set_colors_enabled(false);
            console::set_colors_enabled_stderr(false);
        }
        let settings = Arc::new(Self::default_builder().load()?);
        *SETTINGS.write().unwrap() = Some(settings.clone());
        Ok(settings)
    }
    pub fn add_partial(partial: SettingsPartial) {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let mut p = SettingsPartial::empty();
            if *env::CI {
                p.yes = Some(true);
            }
            if *env::RTX_LOG_LEVEL > LevelFilter::Info {
                p.verbose = Some(true);
            }
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
    pub fn default_builder() -> Builder<Self> {
        let mut b = Self::builder().env();
        for partial in PARTIALS.read().unwrap().iter() {
            b = b.preloaded(partial.clone());
        }
        b
    }

    #[cfg(test)]
    pub fn reset() {
        PARTIALS.write().unwrap().clear();
        *SETTINGS.write().unwrap() = None;
    }

    pub fn ensure_experimental(&self) -> Result<()> {
        let msg = "This command is experimental. Enable it with `rtx config set experimental 1`";
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
