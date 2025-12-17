use clap::Subcommand;
use eyre::Result;

mod get;
mod ls;
mod set;
mod unset;

#[derive(Debug, clap::Args)]
#[clap(name = "shell-alias", about = "Manage shell aliases.")]
pub struct ShellAlias {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Don't show table header
    #[clap(long)]
    pub no_header: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Get(get::ShellAliasGet),
    Ls(ls::ShellAliasLs),
    Set(set::ShellAliasSet),
    Unset(unset::ShellAliasUnset),
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

impl ShellAlias {
    pub async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::ShellAliasLs {
            no_header: self.no_header,
        }));

        cmd.run().await
    }
}
