use atty::Stream;
use color_eyre::eyre::{eyre, Result};
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::ui::color::Color;

/// Show a current setting
///
/// This is the contents of a single entry in ~/.config/rtx/config.toml
///
/// Note that aliases are also stored in this file
/// but managed separately with `rtx aliases get`
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP.as_str(), verbatim_doc_comment)]
pub struct SettingsGet {
    /// The setting to show
    pub key: String,
}

impl Command for SettingsGet {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match config.settings.to_index_map().get(&self.key) {
            Some(value) => Ok(rtxprintln!(out, "{}", value)),
            None => Err(eyre!("Unknown setting: {}", self.key)),
        }
    }
}

static COLOR: Lazy<Color> = Lazy::new(|| Color::new(Stream::Stdout));
static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx settings get legacy_version_file
      true
    "#, COLOR.header("Examples:")}
});

#[cfg(test)]
mod tests {
    use insta::{assert_display_snapshot, assert_snapshot};

    use crate::test::reset_config;
    use crate::{assert_cli, assert_cli_err};

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
