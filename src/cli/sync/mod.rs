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
    pub fn run(self) -> Result<()> {
        match self {
            Self::Node(cmd) => cmd.run(),
            Self::Python(cmd) => cmd.run(),
            Self::Ruby(cmd) => cmd.run(),
        }
    }
}

impl Sync {
    pub fn run(self) -> Result<()> {
        self.command.run()
    }
}
