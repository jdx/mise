use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

mod python;

#[derive(Debug, clap::Args)]
#[clap(about = "Add tool versions from external tools to rtx")]
pub struct Sync {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Python(python::SyncPython),
}

impl Commands {
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match self {
            Self::Python(cmd) => cmd.run(config, out),
        }
    }
}

impl Command for Sync {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        self.command.run(config, out)
    }
}
