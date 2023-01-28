use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

mod activate;
mod envrc;

/// Output direnv function to use rtx inside direnv
///
/// See https://github.com/jdxcode/rtx#direnv for more information
///
/// Because this generates the legacy files based on currently installed plugins,
/// you should run this command after installing new plugins. Otherwise
/// direnv may not know to update environment variables when legacy file versions change.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Direnv {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Envrc(envrc::Envrc),
    Activate(activate::DirenvActivate),
}

impl Commands {
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run(config, out),
            Self::Envrc(cmd) => cmd.run(config, out),
        }
    }
}

impl Command for Direnv {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let cmd = self
            .command
            .unwrap_or(Commands::Activate(activate::DirenvActivate {}));
        cmd.run(config, out)
    }
}
