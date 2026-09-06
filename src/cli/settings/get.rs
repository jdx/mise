use crate::config;
use crate::config::Settings;
use crate::config::settings::SETTINGS_META;
use eyre::bail;

/// Show the effective value of a setting
///
/// Includes defaults, configuration, and environment overrides. With `--local`,
/// read only the selected local config's explicit settings; an unset key is an error.
/// Use `mise config get settings.KEY --file path/to/mise.toml` to inspect one file.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise settings get jobs
mise settings get python.compile"###
    ),
    verbatim_doc_comment
)]
pub(super) struct SettingsGet {
    /// The setting to show
    pub setting: String,
    /// Use the local config file instead of the global one
    #[usage(long, short)]
    pub local: bool,
}

impl SettingsGet {
    pub(super) fn run(self) -> eyre::Result<()> {
        let settings = if self.local {
            let partial = Settings::parse_settings_file(&config::local_toml_config_path())
                .unwrap_or_default();
            Settings::partial_as_dict(&partial)?
        } else {
            Settings::get().as_dict()?
        };
        let mut value = toml::Value::Table(settings);
        let mut key = Some(self.setting.as_str());
        while let Some(k) = key {
            let k = k
                .split_once('.')
                .map(|(a, b)| (a, Some(b)))
                .unwrap_or((k, None));
            if let Some(v) = value.as_table().and_then(|t| t.get(k.0)) {
                key = k.1;
                value = v.clone()
            } else if is_known_setting(&self.setting) {
                bail!("Setting [{}] is not set", self.setting);
            } else {
                bail!("Unknown setting: {}", self.setting);
            }
        }
        match value {
            toml::Value::String(s) => miseprintln!("{s}"),
            value => miseprintln!("{value}"),
        }

        Ok(())
    }
}

fn is_known_setting(key: &str) -> bool {
    if SETTINGS_META.contains_key(key) {
        return true;
    }
    let prefix = format!("{key}.");
    SETTINGS_META.keys().any(|k| k.starts_with(&prefix))
}
