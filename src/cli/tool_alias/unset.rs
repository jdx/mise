use eyre::Result;

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Clear an alias for a tool/backend
///
/// This modifies the contents of ~/.config/mise/config.toml
#[derive(Debug, usage_rs::Args)]
#[usage(visible_aliases = ["rm", "remove", "delete", "del"], example(r###"mise tool-alias unset ripgrep
mise tool-alias unset node project"###), verbatim_doc_comment)]
pub(super) struct ToolAliasUnset {
    /// The tool/backend to remove the alias from
    #[usage(value_name = "TOOL")]
    pub tool: BackendArg,
    /// The alias to remove
    pub alias: Option<String>,
}

impl ToolAliasUnset {
    pub(super) async fn run(self) -> Result<()> {
        let mut global_config = Config::get().await?.global_config()?;
        match self.alias {
            None => {
                global_config.remove_backend_alias(&self.tool)?;
            }
            Some(ref alias) => {
                global_config.remove_alias(&self.tool, alias)?;
            }
        }
        global_config.save()
    }
}
