use clap::Subcommand;
use eyre::Result;

use crate::cli::args::BackendArg;

mod get;
mod ls;
mod set;
mod unset;

#[derive(Debug, clap::Args)]
#[clap(
    name = "tool-alias",
    about = "Manage tool version aliases.",
    alias = "alias",
    alias = "aliases"
)]
pub struct ToolAlias {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// filter aliases by plugin
    #[clap(short, long)]
    pub plugin: Option<BackendArg>,

    /// Don't show table header
    #[clap(long)]
    pub no_header: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Get(get::ToolAliasGet),
    Ls(ls::ToolAliasLs),
    Set(set::ToolAliasSet),
    Unset(unset::ToolAliasUnset),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Get(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run().await,
            Self::Unset(cmd) => cmd.run().await,
        }
    }
}

impl ToolAlias {
    pub async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::ToolAliasLs {
            tool: self.plugin,
            no_header: self.no_header,
        }));

        cmd.run().await
    }
}
