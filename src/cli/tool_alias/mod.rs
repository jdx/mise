use eyre::Result;

use crate::args::BackendArg;

mod get;
mod ls;
mod set;
mod unset;

/// Manage tool backend and version aliases
///
/// A backend alias makes a tool name install from another backend:
/// `[tool_alias] ripgrep = "aqua:BurntSushi/ripgrep"`. A version alias names a
/// version request: `[tool_alias.node.versions] project = "20"` lets you write
/// `node@project`.
///
/// With no subcommand, lists version aliases (same as `mise tool-alias ls`; the
/// flags below are passed to it). See https://mise.jdx.dev/dev-tools/aliases.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    name = "tool-alias",
    alias = "alias",
    alias = "aliases",
    verbatim_doc_comment
)]
pub(crate) struct ToolAlias {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Filter aliases by tool
    #[usage(short = 'p', long = "tool", alias = "plugin", value_name = "TOOL")]
    pub tool: Option<BackendArg>,

    /// Do not print the table header
    #[usage(long)]
    pub no_header: bool,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Get(get::ToolAliasGet),
    Ls(ls::ToolAliasLs),
    Set(set::ToolAliasSet),
    Unset(unset::ToolAliasUnset),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Get(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run().await,
            Self::Unset(cmd) => cmd.run().await,
        }
    }
}

impl ToolAlias {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::ToolAliasLs {
            tool: self.tool,
            no_header: self.no_header,
        }));

        cmd.run().await
    }
}
