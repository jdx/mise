use eyre::{Result, eyre};

use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Add/update a shell alias
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, usage_rs::Args)]
#[usage(visible_aliases = ["add", "create"], example(r###"mise shell-alias set ll "ls -la"
mise shell-alias set gs "git status""###), verbatim_doc_comment)]
pub(super) struct ShellAliasSet {
    /// The alias name
    #[usage(name = "shell_alias")]
    pub alias: String,
    /// The command to run (optional if provided as ALIAS=COMMAND)
    pub command: Option<String>,
}

impl ShellAliasSet {
    pub(super) async fn run(self) -> Result<()> {
        let (alias, command) = match self.command {
            Some(v) => (self.alias, v),
            None => {
                let (k, v) = self.alias.split_once('=').ok_or_else(|| {
                    eyre!("Usage: mise shell-alias set <ALIAS>=<COMMAND> or mise shell-alias set <ALIAS> <COMMAND>")
                })?;
                (k.to_string(), v.to_string())
            }
        };
        let mut global_config = Config::get().await?.global_config()?;
        global_config.set_shell_alias(&alias, &command)?;
        global_config.save()
    }
}
