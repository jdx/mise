use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

/// Show current settings
///
/// This is the contents of ~/.config/rtx/config.toml
///
/// Note that aliases are also stored in this file
/// but managed separately with `rtx aliases`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsLs {}

impl Command for SettingsLs {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        for (key, value) in config.settings.to_index_map() {
            rtxprintln!(out, "{} = {}", key, value);
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
    use insta::assert_snapshot;

    use crate::assert_cli;
    use crate::test::reset_config;

    #[test]
    fn test_settings_ls() {
        reset_config();
        let stdout = assert_cli!("settings");
        assert_snapshot!(stdout);
    }
}
