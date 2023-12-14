use confique::env::parse::{list_by_colon, list_by_comma};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::sync::{Once, RwLock};

use confique::{Builder, Config, Partial};
use log::LevelFilter;
use serde_derive::{Deserialize, Serialize};

use crate::env;

#[derive(Config, Debug, Clone)]
#[config(partial_attr(derive(Debug, Clone)))]
pub struct Settings {
    #[config(env = "RTX_EXPERIMENTAL", default = false)]
    pub experimental: bool,
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

impl Settings {
    pub fn add_partial(partial: SettingsPartial) {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let mut p = SettingsPartial::empty();
            if *env::CI {
                p.yes = Some(true);
            }
            if *env::RTX_LOG_LEVEL < LevelFilter::Info {
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
    }
    pub fn default_builder() -> Builder<Self> {
        let mut b = Self::builder();
        for partial in PARTIALS.read().unwrap().iter() {
            b = b.preloaded(partial.clone());
        }
        b.env()
    }

    pub fn to_index_map(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert("experimental".to_string(), self.experimental.to_string());
        map.insert(
            "always_keep_download".to_string(),
            self.always_keep_download.to_string(),
        );
        map.insert(
            "always_keep_install".to_string(),
            self.always_keep_install.to_string(),
        );
        map.insert(
            "legacy_version_file".to_string(),
            self.legacy_version_file.to_string(),
        );
        map.insert(
            "legacy_version_file_disable_tools".to_string(),
            format!(
                "{:?}",
                self.legacy_version_file_disable_tools
                    .iter()
                    .collect::<Vec<_>>()
            ),
        );
        map.insert(
            "plugin_autoupdate_last_check_duration".to_string(),
            self.plugin_autoupdate_last_check_duration.to_string(),
        );
        map.insert(
            "trusted_config_paths".to_string(),
            format!("{:?}", self.trusted_config_paths.iter().collect::<Vec<_>>()),
        );
        map.insert("verbose".into(), self.verbose.to_string());
        map.insert("asdf_compat".into(), self.asdf_compat.to_string());
        map.insert("jobs".into(), self.jobs.to_string());
        if let Some(shorthands_file) = &self.shorthands_file {
            map.insert(
                "shorthands_file".into(),
                shorthands_file.to_string_lossy().to_string(),
            );
        }
        map.insert(
            "disable_default_shorthands".into(),
            self.disable_default_shorthands.to_string(),
        );
        map.insert(
            "disable_tools".into(),
            format!("{:?}", self.disable_tools.iter().collect::<Vec<_>>()),
        );
        map.insert("raw".into(), self.raw.to_string());
        map.insert("yes".into(), self.yes.to_string());
        map
    }

    #[cfg(test)]
    pub fn reset() {
        PARTIALS.write().unwrap().clear();
    }
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.to_index_map().fmt(f)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MissingRuntimeBehavior {
    AutoInstall,
    Prompt,
    Warn,
    Ignore,
}

impl Display for MissingRuntimeBehavior {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MissingRuntimeBehavior::AutoInstall => write!(f, "autoinstall"),
            MissingRuntimeBehavior::Prompt => write!(f, "prompt"),
            MissingRuntimeBehavior::Warn => write!(f, "warn"),
            MissingRuntimeBehavior::Ignore => write!(f, "ignore"),
        }
    }
}
