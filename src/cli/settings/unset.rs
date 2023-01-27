use color_eyre::eyre::Result;
use indoc::indoc;

use crate::cli::command::Command;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::output::Output;

/// Clears a setting
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsUnset {
    /// The setting to remove
    pub key: String,
}

impl Command for SettingsUnset {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let mut rtxrc = config.rtxrc;
        rtxrc.remove_setting(&self.key);
        rtxrc.save()
    }
}

const AFTER_LONG_HELP: &str = indoc! {r#"
    Examples:
      $ rtx settings unset legacy_version_file
    "#};

#[cfg(test)]
mod test {
    use insta::assert_snapshot;

    use crate::assert_cli;
    use crate::output::Output;
    use crate::test::reset_config;

    #[test]
    fn test_settings_unset() {
        reset_config();

        let Output { stdout, .. } = assert_cli!("settings");
        assert_snapshot!(stdout.content, @r###"
        missing_runtime_behavior = autoinstall
        always_keep_download = true
        legacy_version_file = true
        disable_plugin_short_name_repository = false
        plugin_autoupdate_last_check_duration = 20
        plugin_repository_last_check_duration = 20
        "###);

        assert_cli!("settings", "unset", "legacy_version_file");

        let Output { stdout, .. } = assert_cli!("settings");
        assert_snapshot!(stdout.content, @r###"
        missing_runtime_behavior = autoinstall
        always_keep_download = true
        legacy_version_file = true
        disable_plugin_short_name_repository = false
        plugin_autoupdate_last_check_duration = 20
        plugin_repository_last_check_duration = 20
        "###);

        reset_config();
    }
}
