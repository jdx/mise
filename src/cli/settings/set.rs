use color_eyre::eyre::{eyre, Result};
use indoc::indoc;

use crate::cli::command::Command;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::output::Output;

/// Add/update a setting
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["add", "create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsSet {
    /// The setting to set
    pub key: String,
    /// The value to set
    pub value: String,
}

impl Command for SettingsSet {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let mut rtxrc = config.rtxrc;
        let value: toml_edit::Value = match self.key.as_str() {
            "missing_runtime_behavior" => self.value.into(),
            "always_keep_download" => parse_bool(&self.value)?,
            "legacy_version_file" => parse_bool(&self.value)?,
            "disable_plugin_short_name_repository" => parse_bool(&self.value)?,
            "plugin_autoupdate_last_check_duration" => parse_i64(&self.value)?,
            "plugin_repository_last_check_duration" => parse_i64(&self.value)?,
            _ => return Err(eyre!("Unknown setting: {}", self.key)),
        };

        rtxrc.update_setting(&self.key, value);
        rtxrc.save()
    }
}

fn parse_bool(value: &str) -> Result<toml_edit::Value> {
    match value {
        "true" => Ok(true.into()),
        "false" => Ok(false.into()),
        _ => Err(eyre!("{} must be true or false", value)),
    }
}

fn parse_i64(value: &str) -> Result<toml_edit::Value> {
    match value.parse::<i64>() {
        Ok(value) => Ok(value.into()),
        Err(_) => Err(eyre!("{} must be a number", value)),
    }
}

const AFTER_LONG_HELP: &str = indoc! {r#"
    Examples:
      $ rtx settings set legacy_version_file true
    "#};

#[cfg(test)]
pub mod test {
    use insta::assert_snapshot;

    use crate::assert_cli;

    use crate::test::reset_config;

    #[test]
    fn test_settings_set() {
        reset_config();
        let stdout = assert_cli!("settings");
        assert_snapshot!(stdout);

        assert_cli!("settings", "set", "missing_runtime_behavior", "warn");
        assert_cli!("settings", "set", "legacy_version_file", "false");
        assert_cli!("settings", "set", "always_keep_download", "true");
        assert_cli!(
            "settings",
            "set",
            "disable_plugin_short_name_repository",
            "true"
        );
        assert_cli!(
            "settings",
            "set",
            "plugin_autoupdate_last_check_duration",
            "1"
        );
        assert_cli!(
            "settings",
            "set",
            "plugin_repository_last_check_duration",
            "2"
        );

        let stdout = assert_cli!("settings");
        assert_snapshot!(stdout);
        reset_config();
    }
}
