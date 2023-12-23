use eyre::Result;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::config::{Config, Settings};

/// Show current settings
///
/// This is the contents of ~/.config/rtx/config.toml
///
/// Note that aliases are also stored in this file
/// but managed separately with `rtx aliases`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsLs {}

impl SettingsLs {
    pub fn run(self) -> Result<()> {
        Config::try_get()?;
        let settings = Settings::try_get()?;
        let json = settings.to_string();
        let doc: BTreeMap<String, Value> = serde_json::from_str(&json)?;
        for (key, value) in doc {
            if Settings::hidden_configs().contains(key.as_str()) {
                continue;
            }
            rtxprintln!("{} = {}", key, value);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx settings</bold>
  legacy_version_file = false
"#
);

#[cfg(test)]
mod tests {

    use crate::test::reset_config;

    #[test]
    fn test_settings_ls() {
        reset_config();
        assert_cli_snapshot!("settings");
    }
}
