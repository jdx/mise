use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::config::Config;

mod node;
mod python;

#[derive(Debug, clap::Args)]
#[clap(about = "Add tool versions from external tools to rtx")]
pub struct Sync {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Node(node::SyncNode),
    Python(python::SyncPython),
}

impl Commands {
    pub fn run(self, config: Config) -> Result<()> {
        match self {
            Self::Node(cmd) => cmd.run(config),
            Self::Python(cmd) => cmd.run(config),
        }
    }
}

impl Sync {
    pub fn run(self, config: Config) -> Result<()> {
        self.command.run(config)
    }
}
