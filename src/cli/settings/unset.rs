use eyre::Result;

use crate::config::config_file::ConfigFile;
use crate::config::Config;

/// Clears a setting
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsUnset {
    /// The setting to remove
    pub setting: String,
}

impl SettingsUnset {
    pub fn run(self) -> Result<()> {
        let mut global_config = Config::try_get()?.global_config.clone();
        global_config.remove_setting(&self.setting);
        global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx settings unset legacy_version_file</bold>
"#
);

#[cfg(test)]
mod tests {

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
        color = true
        disable_default_shorthands = false
        disable_tools = []
        experimental = true
        jobs = 2
        legacy_version_file = true
        legacy_version_file_disable_tools = []
        not_found_auto_install = true
        plugin_autoupdate_last_check_duration = "20m"
        quiet = false
        raw = false
        shorthands_file = null
        task_output = null
        trusted_config_paths = []
        verbose = true
        yes = true
        "###);

        reset_config();
    }
}
