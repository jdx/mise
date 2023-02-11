use atty::Stream;
use color_eyre::eyre::Result;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::output::Output;
use crate::ui::color::Color;

/// Clears a setting
///
/// This modifies the contents of ~/.config/rtx/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases=["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP.as_str(), verbatim_doc_comment)]
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

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx settings unset legacy_version_file
    "#, COLOR.header("Examples:")}
});

#[cfg(test)]
mod test {
    use insta::assert_snapshot;

    use crate::assert_cli;

    use crate::test::reset_config;

    #[test]
    fn test_settings_unset() {
        reset_config();

        assert_cli!("settings", "unset", "legacy_version_file");

        let stdout = assert_cli!("settings");
        assert_snapshot!(stdout, @r###"
        missing_runtime_behavior = autoinstall
        always_keep_download = true
        legacy_version_file = true
        plugin_autoupdate_last_check_duration = 20
        verbose = true
        "###);

        reset_config();
    }
}
