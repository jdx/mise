use clap::Subcommand;
use eyre::Result;

mod check;
mod get;

#[derive(Debug, clap::Args)]
#[clap(
    about = "Manage secrets stored in external providers",
    visible_alias = "secret"
)]
pub struct Secrets {
    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Check(check::Check),
    Get(get::Get),
}

impl Secrets {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Check(cmd) => cmd.run().await,
            Commands::Get(cmd) => cmd.run().await,
        }
    }
}
