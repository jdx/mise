use eyre::Result;

use crate::args::BackendArg;
use crate::config::Config;
use crate::config::config_file::ConfigFile;

/// Remove a backend alias or a version alias from the global config
///
/// With only TOOL, removes the tool's backend alias. With TOOL and ALIAS,
/// removes that version alias. Edits the global config
/// (~/.config/mise/config.toml by default).
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_aliases = ["rm", "remove", "delete", "del"],
    example(
        r###"mise tool-alias unset ripgrep"###,
        help = "Remove ripgrep's backend alias"
    ),
    example(
        r###"mise tool-alias unset node project"###,
        help = "Remove node's project version alias"
    ),
    verbatim_doc_comment
)]
pub(super) struct ToolAliasUnset {
    /// The tool to remove the alias from
    #[usage(value_name = "TOOL")]
    pub tool: BackendArg,
    /// The version alias to remove; omit it to remove the backend alias
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
