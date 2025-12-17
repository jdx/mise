use eyre::Result;

use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Add/update a shell alias
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, clap::Args)]
#[clap(visible_aliases = ["add", "create"], after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ShellAliasSet {
    /// The alias name
    #[clap(name = "shell_alias")]
    pub alias: String,
    /// The command to run
    pub command: String,
}

impl ShellAliasSet {
    pub async fn run(self) -> Result<()> {
        let mut global_config = Config::get().await?.global_config()?;
        global_config.set_shell_alias(&self.alias, &self.command)?;
        global_config.save()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise shell-alias set ll "ls -la"</bold>
    $ <bold>mise shell-alias set gs "git status"</bold>
"#
);
