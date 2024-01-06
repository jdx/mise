use clap::Subcommand;
use miette::Result;

mod node;
mod python;

#[derive(Debug, clap::Args)]
#[clap(about = "Add tool versions from external tools to mise")]
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
    pub fn run(self) -> Result<()> {
        match self {
            Self::Node(cmd) => cmd.run(),
            Self::Python(cmd) => cmd.run(),
        }
    }
}

impl Sync {
    pub fn run(self) -> Result<()> {
        self.command.run()
    }
}
