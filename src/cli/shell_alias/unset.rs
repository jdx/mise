use eyre::Result;

use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Remove a shell alias
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, usage_rs::Args)]
#[usage(visible_aliases = ["rm", "remove", "delete", "del"], example(r###"mise shell-alias unset ll"###), verbatim_doc_comment)]
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
