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
    pub key: String,
    /// The value to set
    pub value: String,
    /// Use the local config file instead of the global one
    #[clap(long, short)]
    pub local: bool,
}

impl SettingsAdd {
    pub fn run(self) -> Result<()> {
        set(&self.key, &self.value, true, self.local)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise settings add disable_hints python_multi</bold>
"#
);
