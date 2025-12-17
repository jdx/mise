use eyre::Result;

use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Removes a shell alias
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["rm", "remove", "delete", "del"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ShellAliasUnset {
    /// The alias to remove
    #[clap(name = "shell_alias")]
    pub alias: String,
}

impl ShellAliasUnset {
    pub async fn run(self) -> Result<()> {
        let mut global_config = Config::get().await?.global_config()?;
        global_config.remove_shell_alias(&self.alias)?;
        global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise shell-alias unset ll</bold>
"#
);
