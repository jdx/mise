use crate::config::Settings;
use eyre::bail;

/// Show a current setting
///
/// This is the contents of a single entry in ~/.config/mise/config.toml
///
/// Note that aliases are also stored in this file
/// but managed separately with `mise aliases get`
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsGet {
    /// The setting to show
    pub setting: String,
}

impl SettingsGet {
    pub fn run(self) -> eyre::Result<()> {
        let settings = Settings::try_get()?;
        let mut value = toml::Value::Table(settings.as_dict()?);
        let mut key = Some(self.setting.as_str());
        while let Some(k) = key {
            let k = k
                .split_once('.')
                .map(|(a, b)| (a, Some(b)))
                .unwrap_or((k, None));
            if let Some(v) = value.as_table().and_then(|t| t.get(k.0)) {
                key = k.1;
                value = v.clone()
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings get legacy_version_file</bold>
    true
"#
);
