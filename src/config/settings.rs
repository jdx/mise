use confique::env::parse::{list_by_colon, list_by_comma};
use confique::{Builder, Config, Partial};
use log::LevelFilter;
use serde_derive::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::time::Duration;

use crate::{duration, env};

#[derive(Config, Debug, Clone)]
pub struct Settings {
    #[config(env = "RTX_EXPERIMENTAL", default = false)]
    pub experimental: bool,
    #[config(env = "RTX_MISSING_RUNTIME_BEHAVIOR")]
    pub missing_runtime_behavior: MissingRuntimeBehavior,
    #[config(env = "RTX_ALWAYS_KEEP_DOWNLOAD", default = false)]
    pub always_keep_download: bool,
    #[config(env = "RTX_ALWAYS_KEEP_INSTALL", default = false)]
    pub always_keep_install: bool,
    #[config(env = "RTX_LEGACY_VERSION_FILE", default = true)]
    pub legacy_version_file: bool,
    #[config(env = "RTX_LEGACY_VERSION_FILE_DISABLE_TOOLS", default = [], parse_env = list_by_comma)]
    pub legacy_version_file_disable_tools: BTreeSet<String>,
    #[config(env = "RTX_PLUGIN_AUTOUPDATE_LAST_CHECK_DURATION")]
    pub plugin_autoupdate_last_check_duration: Duration,
    #[config(env = "RTX_TRUSTED_CONFIG_PATHS", default = [], parse_env = list_by_colon)]
    pub trusted_config_paths: BTreeSet<PathBuf>,
    #[config(env = "RTX_VERBOSE", default = false)]
    pub verbose: bool,
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
    pub yes: bool,
}

pub type SettingsPartial = <Settings as Config>::Partial;

impl Default for Settings {
    fn default() -> Self {
        Settings::default_builder().load().unwrap()
    }
}
impl Settings {
    pub fn default_builder() -> Builder<Self> {
        let mut partial = SettingsPartial::empty();
        partial.missing_runtime_behavior = Some(MissingRuntimeBehavior::Warn);
        partial.plugin_autoupdate_last_check_duration = Some(duration::WEEKLY);
        partial.yes = Some(*env::RTX_YES);
        if *env::RTX_LOG_LEVEL < LevelFilter::Info {
            partial.verbose = Some(true);
        }

        Settings::builder().preloaded(partial).env()
    }

    pub fn to_index_map(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert("experimental".to_string(), self.experimental.to_string());
        map.insert(
            "missing_runtime_behavior".to_string(),
            self.missing_runtime_behavior.to_string(),
        );
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
            (self.plugin_autoupdate_last_check_duration.as_secs() / 60).to_string(),
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
}

#[derive(Default, Clone)]
pub struct SettingsBuilder {
    pub experimental: Option<bool>,
    pub missing_runtime_behavior: Option<MissingRuntimeBehavior>,
    pub always_keep_download: Option<bool>,
    pub always_keep_install: Option<bool>,
    pub legacy_version_file: Option<bool>,
    pub legacy_version_file_disable_tools: BTreeSet<String>,
    pub plugin_autoupdate_last_check_duration: Option<Duration>,
    pub trusted_config_paths: BTreeSet<PathBuf>,
    pub verbose: Option<bool>,
    pub asdf_compat: Option<bool>,
    pub jobs: Option<usize>,
    pub shorthands_file: Option<PathBuf>,
    pub disable_default_shorthands: Option<bool>,
    pub disable_tools: BTreeSet<String>,
    pub raw: Option<bool>,
    pub yes: Option<bool>,
}

impl SettingsBuilder {
    pub fn merge(&mut self, other: Self) -> &mut Self {
        if other.experimental.is_some() {
            self.experimental = other.experimental;
        }
        if other.missing_runtime_behavior.is_some() {
            self.missing_runtime_behavior = other.missing_runtime_behavior;
        }
        if other.always_keep_download.is_some() {
            self.always_keep_download = other.always_keep_download;
        }
        if other.always_keep_install.is_some() {
            self.always_keep_install = other.always_keep_install;
        }
        if other.legacy_version_file.is_some() {
            self.legacy_version_file = other.legacy_version_file;
        }
        self.legacy_version_file_disable_tools
            .extend(other.legacy_version_file_disable_tools);
        if other.plugin_autoupdate_last_check_duration.is_some() {
            self.plugin_autoupdate_last_check_duration =
                other.plugin_autoupdate_last_check_duration;
        }
        self.trusted_config_paths.extend(other.trusted_config_paths);
        if other.verbose.is_some() {
            self.verbose = other.verbose;
        }
        if other.asdf_compat.is_some() {
            self.asdf_compat = other.asdf_compat;
        }
        if other.jobs.is_some() {
            self.jobs = other.jobs;
        }
        if other.shorthands_file.is_some() {
            self.shorthands_file = other.shorthands_file;
        }
        if other.disable_default_shorthands.is_some() {
            self.disable_default_shorthands = other.disable_default_shorthands;
        }
        self.disable_tools.extend(other.disable_tools);
        if other.raw.is_some() {
            self.raw = other.raw;
        }
        if other.yes.is_some() {
            self.yes = other.yes;
        }
        self
    }

    pub fn build(&self) -> Settings {
        let mut settings = Settings::default();
        settings.experimental = self.experimental.unwrap_or(settings.experimental);
        settings.missing_runtime_behavior = match env::RTX_MISSING_RUNTIME_BEHAVIOR
            .to_owned()
            .unwrap_or_default()
            .as_ref()
        {
            "autoinstall" => MissingRuntimeBehavior::AutoInstall,
            "warn" => MissingRuntimeBehavior::Warn,
            "ignore" => MissingRuntimeBehavior::Ignore,
            "prompt" => MissingRuntimeBehavior::Prompt,
            _ => self
                .missing_runtime_behavior
                .clone()
                .unwrap_or(settings.missing_runtime_behavior),
        };
        settings.always_keep_download = self
            .always_keep_download
            .unwrap_or(settings.always_keep_download);
        settings.always_keep_install = self
            .always_keep_install
            .unwrap_or(settings.always_keep_install);
        settings.legacy_version_file = self
            .legacy_version_file
            .unwrap_or(settings.legacy_version_file);
        settings
            .legacy_version_file_disable_tools
            .extend(self.legacy_version_file_disable_tools.clone());
        settings.plugin_autoupdate_last_check_duration = self
            .plugin_autoupdate_last_check_duration
            .unwrap_or(settings.plugin_autoupdate_last_check_duration);
        settings
            .trusted_config_paths
            .extend(self.trusted_config_paths.clone());
        settings.verbose = self.verbose.unwrap_or(settings.verbose);
        settings.asdf_compat = self.asdf_compat.unwrap_or(settings.asdf_compat);
        settings.jobs = self.jobs.unwrap_or(settings.jobs);
        settings.shorthands_file = self.shorthands_file.clone().or(settings.shorthands_file);
        settings.disable_default_shorthands = self
            .disable_default_shorthands
            .unwrap_or(settings.disable_default_shorthands);
        settings.disable_tools.extend(self.disable_tools.clone());
        settings.raw = self.raw.unwrap_or(settings.raw);
        settings.yes = self.yes.unwrap_or(settings.yes);

        if settings.raw {
            settings.verbose = true;
            settings.jobs = 1;
        }

        settings
    }
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.to_index_map().fmt(f)
    }
}

impl Debug for SettingsBuilder {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("SettingsBuilder");
        if let Some(experimental) = self.experimental {
            d.field("experimental", &experimental);
        }
        if let Some(missing_runtime_behavior) = &self.missing_runtime_behavior {
            d.field("missing_runtime_behavior", &missing_runtime_behavior);
        }
        if let Some(always_keep_download) = self.always_keep_download {
            d.field("always_keep_download", &always_keep_download);
        }
        if let Some(always_keep_install) = self.always_keep_install {
            d.field("always_keep_install", &always_keep_install);
        }
        if let Some(legacy_version_file) = self.legacy_version_file {
            d.field("legacy_version_file", &legacy_version_file);
        }
        if !self.legacy_version_file_disable_tools.is_empty() {
            d.field(
                "legacy_version_file_disable_tools",
                &self.legacy_version_file_disable_tools,
            );
        }
        if let Some(c) = self.plugin_autoupdate_last_check_duration {
            d.field("plugin_autoupdate_last_check_duration", &c);
        }
        if !self.trusted_config_paths.is_empty() {
            d.field("trusted_config_paths", &self.trusted_config_paths);
        }
        if let Some(verbose) = self.verbose {
            d.field("verbose", &verbose);
        }
        if let Some(asdf_compat) = self.asdf_compat {
            d.field("asdf_compat", &asdf_compat);
        }
        if let Some(jobs) = self.jobs {
            d.field("jobs", &jobs);
        }
        if let Some(shorthands_file) = &self.shorthands_file {
            d.field("shorthands_file", &shorthands_file);
        }
        if let Some(dds) = self.disable_default_shorthands {
            d.field("disable_default_shorthands", &dds);
        }
        if !self.disable_tools.is_empty() {
            d.field("disable_tools", &self.disable_tools);
        }
        if let Some(raw) = self.raw {
            d.field("raw", &raw);
        }
        if let Some(yes) = self.yes {
            d.field("yes", &yes);
        }
        d.finish()
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

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_settings_merge() {
        let mut s1 = SettingsBuilder::default();
        let s2 = SettingsBuilder {
            asdf_compat: Some(true),
            ..SettingsBuilder::default()
        };
        s1.merge(s2);

        assert_eq!(s1.asdf_compat, Some(true));
    }
}
