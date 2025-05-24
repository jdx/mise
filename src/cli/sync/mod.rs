use clap::Subcommand;
use eyre::Result;

mod node;
mod python;
mod ruby;

#[derive(Debug, clap::Args)]
#[clap(about = "Synchronize tools from other version managers with mise")]
pub struct Sync {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Node(node::SyncNode),
    Python(python::SyncPython),
    Ruby(ruby::SyncRuby),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Node(cmd) => cmd.run().await,
            Self::Python(cmd) => cmd.run().await,
            Self::Ruby(cmd) => cmd.run().await,
        }
    }
}

impl Sync {
    pub async fn run(self) -> Result<()> {
        self.command.run().await
    }
}
