use color_eyre::eyre::{Result, eyre};

use crate::args::BackendArg;
use crate::config::Config;

/// Show the version a tool's version alias stands for
///
/// Reads `[tool_alias.TOOL.versions]` from every loaded config file and the
/// aliases the tool's backend provides, such as node's `lts-*` aliases. It
/// prints the stored request, which may itself be a prefix.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise tool-alias get node lts
24"###,
        help = "Show the version node's lts alias stands for"
    ),
    example(
        r###"mise tool-alias set node project 20
mise tool-alias get node project
20"###,
        help = "Read back an alias you set"
    ),
    verbatim_doc_comment
)]
pub(super) struct ToolAliasGet {
    /// The tool to show the alias for
    #[usage(value_name = "TOOL")]
    pub tool: BackendArg,
    /// The alias to show
    pub alias: String,
}

impl ToolAliasGet {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        match config.all_aliases.get(&self.tool.short) {
            Some(alias) => match alias.versions.get(&self.alias) {
                Some(alias) => {
                    miseprintln!("{alias}");
                    Ok(())
                }
                None => Err(eyre!("Unknown alias: {}", &self.alias)),
            },
            None => Err(eyre!("Unknown tool: {}", &self.tool)),
        }
    }
}
