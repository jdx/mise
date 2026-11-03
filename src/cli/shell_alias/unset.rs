use eyre::Result;

use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Remove a shell alias from the global config
///
/// Edits the global config (~/.config/mise/config.toml by default). An alias
/// defined in a project's mise.toml has to be removed there.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_aliases = ["rm", "remove", "delete", "del"],
    example(r###"mise shell-alias unset ll"###, help = "Remove ll from the global config"),
    verbatim_doc_comment
)]
pub(super) struct ShellAliasUnset {
    /// The alias to remove
    #[usage(name = "shell_alias")]
    pub alias: String,
}

impl ShellAliasUnset {
    pub(super) async fn run(self) -> Result<()> {
        let mut global_config = Config::get().await?.global_config()?;
        global_config.remove_shell_alias(&self.alias)?;
        global_config.save()
    }
}
