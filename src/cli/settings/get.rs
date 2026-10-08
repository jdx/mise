use crate::config;
use crate::config::settings::SETTINGS_META;
use crate::config::{Settings, SettingsExt};
use eyre::bail;

/// Show the effective value of a setting
///
/// Includes defaults, config files, and environment overrides. With `--local`,
/// reads only the explicit settings in the nearest project config, and a
/// setting that is not set there is an error. An optional setting with no
/// default that is not set anywhere, such as `python.compile`, is also reported
/// as not set and exits with an error. To inspect one file, use
/// `mise config get settings.KEY --file path/to/mise.toml`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise settings get jobs"###,
        help = "Show the effective number of parallel jobs"
    ),
    example(
        r###"mise settings get --local experimental"###,
        help = "Show the value set in the project config"
    ),
    verbatim_doc_comment
)]
pub(super) struct SettingsGet {
    /// The setting to show
    pub setting: String,
    /// Read only the explicit settings in the nearest project config
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
