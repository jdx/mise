use clap::Subcommand;
use eyre::Result;

mod ls;
mod show;

/// Manage age encryption keys
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Keys {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Ls(ls::KeysLs),
    Show(show::KeysShow),
}

impl Keys {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Ls(cmd) => cmd.run().await,
            Commands::Show(cmd) => cmd.run().await,
        }
    }
}
