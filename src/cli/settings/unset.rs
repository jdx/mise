use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::output::Output;

/// Clears a setting
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP.as_str(), verbatim_doc_comment)]
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

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx settings unset legacy_version_file
    "#, style("Examples:").bold().underlined()}
});

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
        experimental = true
        missing_runtime_behavior = autoinstall
        always_keep_download = true
        legacy_version_file = true
        plugin_autoupdate_last_check_duration = 20
        verbose = true
        asdf_compat = false
        jobs = 2
        disable_default_shorthands = false
        log_level = INFO
        shims_dir = ~/data/shims
        raw = false
        "###);

        reset_config();
    }
}
