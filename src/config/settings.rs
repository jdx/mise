use std::time::Duration;

use indexmap::IndexMap;

use crate::config::AliasMap;
use crate::env;
use crate::plugins::PluginName;

#[derive(Debug, Clone)]
pub struct Settings {
    pub missing_runtime_behavior: MissingRuntimeBehavior,
    pub always_keep_download: bool,
    pub legacy_version_file: bool,
    pub disable_plugin_short_name_repository: bool,
    pub plugin_autoupdate_last_check_duration: Duration,
    pub plugin_repository_last_check_duration: Duration,
    pub aliases: IndexMap<PluginName, IndexMap<String, String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            missing_runtime_behavior: MissingRuntimeBehavior::Prompt,
            always_keep_download: false,
            legacy_version_file: false,
            disable_plugin_short_name_repository: false,
            plugin_autoupdate_last_check_duration: Duration::from_secs(60 * 60 * 24 * 7),
            plugin_repository_last_check_duration: Duration::from_secs(60 * 60 * 24 * 7),
            aliases: IndexMap::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct SettingsBuilder {
    pub missing_runtime_behavior: Option<MissingRuntimeBehavior>,
    pub always_keep_download: Option<bool>,
    pub legacy_version_file: Option<bool>,
    pub disable_plugin_short_name_repository: Option<bool>,
    pub plugin_autoupdate_last_check_duration: Option<Duration>,
    pub plugin_repository_last_check_duration: Option<Duration>,
    pub aliases: Option<AliasMap>,
}

impl SettingsBuilder {
    // pub fn new(missing_runtime_behavior: Option<MissingRuntimeBehavior>) -> Self {
    //     Self {
    //         missing_runtime_behavior: missing_runtime_behavior,
    //     }
    // }

    pub fn _merge(&mut self, other: Self) -> &mut Self {
        if other.missing_runtime_behavior.is_some() {
            self.missing_runtime_behavior = other.missing_runtime_behavior;
        }
        if other.always_keep_download.is_some() {
            self.always_keep_download = other.always_keep_download;
        }
        if other.legacy_version_file.is_some() {
            self.legacy_version_file = other.legacy_version_file;
        }
        if other.disable_plugin_short_name_repository.is_some() {
            self.disable_plugin_short_name_repository = other.disable_plugin_short_name_repository;
        }
        if other.plugin_autoupdate_last_check_duration.is_some() {
            self.plugin_autoupdate_last_check_duration =
                other.plugin_autoupdate_last_check_duration;
        }
        if other.plugin_repository_last_check_duration.is_some() {
            self.plugin_repository_last_check_duration =
                other.plugin_repository_last_check_duration;
        }
        if other.aliases.is_some() {
            self.aliases = other.aliases;
        }
        self
    }

    pub fn build(&self) -> Settings {
        let mut settings = Settings::default();
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
        settings.legacy_version_file = self
            .legacy_version_file
            .unwrap_or(settings.legacy_version_file);
        settings.disable_plugin_short_name_repository = self
            .disable_plugin_short_name_repository
            .unwrap_or(settings.disable_plugin_short_name_repository);
        settings.plugin_repository_last_check_duration = self
            .plugin_repository_last_check_duration
            .unwrap_or(settings.plugin_repository_last_check_duration);
        settings.plugin_autoupdate_last_check_duration = self
            .plugin_autoupdate_last_check_duration
            .unwrap_or(settings.plugin_autoupdate_last_check_duration);
        settings.aliases = self.aliases.clone().unwrap_or(settings.aliases);

        settings
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MissingRuntimeBehavior {
    AutoInstall,
    Prompt,
    Warn,
    Ignore,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_merge() {
        let mut s1 = SettingsBuilder::default();
        let s2 = SettingsBuilder {
            missing_runtime_behavior: Some(MissingRuntimeBehavior::AutoInstall),
            ..SettingsBuilder::default()
        };
        s1._merge(s2);

        assert_eq!(
            s1.missing_runtime_behavior,
            Some(MissingRuntimeBehavior::AutoInstall)
        );
    }
}
