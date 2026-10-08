use eyre::{Result, eyre};

use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Set a shell alias in the global config
///
/// Writes the global config (~/.config/mise/config.toml by default). To share
/// an alias with a project, add it under `[shell_alias]` in the project's
/// mise.toml.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_aliases = ["add", "create"],
    example(r###"mise shell-alias set ll "ls -la""###, help = "Define ll in every directory"),
    example(r###"mise shell-alias set gs="git status""###, help = "Use the ALIAS=COMMAND form"),
    verbatim_doc_comment
)]
pub(super) struct ShellAliasSet {
    /// The alias name
    #[usage(name = "shell_alias")]
    pub alias: String,
    /// The command to run (or pass ALIAS=COMMAND)
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
