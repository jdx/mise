use eyre::Result;

mod get;
mod ls;
mod set;
mod unset;

#[derive(Debug, usage_rs::Args)]
#[usage(name = "shell-alias", about = "Manage shell aliases.")]
pub(crate) struct ShellAlias {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Don't show table header
    #[usage(long)]
    pub no_header: bool,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Get(get::ShellAliasGet),
    Ls(ls::ShellAliasLs),
    Set(set::ShellAliasSet),
    Unset(unset::ShellAliasUnset),
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

impl ShellAlias {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::ShellAliasLs {
            no_header: self.no_header,
        }));

        cmd.run().await
    }
}
