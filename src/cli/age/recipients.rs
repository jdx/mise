use clap::Subcommand;
use eyre::Result;

mod add;
mod ls;

/// Manage age encryption recipients
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Recipients {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Ls(ls::RecipientsLs),
    Add(add::RecipientsAdd),
}

impl Recipients {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Ls(cmd) => cmd.run().await,
            Commands::Add(cmd) => cmd.run().await,
        }
    }
}
