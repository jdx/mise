use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::output::Output;

/// Clears a setting
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsUnset {
    /// The setting to remove
    pub key: String,
}

impl Command for SettingsUnset {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        config.global_config.remove_setting(&self.key);
        config.global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx settings unset legacy_version_file</bold>
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use crate::assert_cli;
    use crate::test::reset_config;

    #[test]
    fn test_settings_unset() {
        reset_config();

        assert_cli!("settings", "unset", "legacy_version_file");

        let stdout = assert_cli!("settings");
        assert_snapshot!(stdout, @r###"
        always_keep_download = true
        always_keep_install = true
        asdf_compat = false
        disable_default_shorthands = false
        disable_tools = []
        experimental = true
        jobs = 2
        legacy_version_file = true
        legacy_version_file_disable_tools = []
        log_level = INFO
        missing_runtime_behavior = autoinstall
        plugin_autoupdate_last_check_duration = 20
        raw = false
        trusted_config_paths = []
        verbose = true
        yes = true
        "###);

        reset_config();
    }
}
