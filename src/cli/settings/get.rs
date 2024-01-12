use std::collections::BTreeMap;

use miette::{IntoDiagnostic, Result};
use serde_json::Value;

use crate::config::{Config, Settings};

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
    pub fn run(self) -> Result<()> {
        Config::try_get()?;
        let settings = Settings::try_get()?;
        let json = settings.to_string();
        let doc: BTreeMap<String, Value> = serde_json::from_str(&json).into_diagnostic()?;
        match doc.get(&self.setting) {
            Some(value) => Ok(miseprintln!("{}", value)),
            None => Err(miette!("Unknown setting: {}", self.setting)),
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise settings get legacy_version_file</bold>
  true
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset_config;

    #[test]
    fn test_settings_get() {
        reset_config();
        let stdout = assert_cli!("settings", "get", "legacy_version_file");
        assert_snapshot!(stdout, @r###"
        true
        "###);
    }

    #[test]
    fn test_settings_get_unknown() {
        let err = assert_cli_err!("settings", "get", "unknown");
        assert_display_snapshot!(err, @"Unknown setting: unknown");
    }
}
