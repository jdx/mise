use eyre::Result;

use crate::cli::settings::set::set;

/// Adds a setting to the configuration file
///
/// Used with an array setting, this will append the value to the array.
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct SettingsAdd {
    /// The setting to set
    #[clap()]
    pub setting: String,
    /// The value to set
    pub value: String,
}

impl SettingsAdd {
    pub fn run(self) -> Result<()> {
        set(&self.setting, &self.value, true)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings add disable_hints python_multi</bold>
"#
);

#[cfg(test)]
pub mod tests {
    use crate::test::reset;

    #[test]
    fn test_settings_add() {
        reset();
        assert_cli_snapshot!("settings", "add", "disable_hints", "a", @"");
        assert_cli_snapshot!("settings", "add", "disable_hints", "b", @"");
        assert_cli_snapshot!("settings", "get", "disable_hints", @r#"["a", "b"]"#);
    }
}
