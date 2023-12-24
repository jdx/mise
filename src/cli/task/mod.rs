use crate::cli::run;
use clap::Subcommand;
use eyre::Result;

mod edit;
mod ls;

/// [experimental] Manage tasks
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "t", alias = "tasks", verbatim_doc_comment)]
pub struct Task {
    #[clap(subcommand)]
    command: Option<Commands>,

    #[clap(flatten)]
    ls: ls::TaskLs,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Edit(edit::TaskEdit),
    Ls(ls::TaskLs),
    Run(run::Run),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Edit(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run(),
            Self::Run(cmd) => cmd.run(),
        }
    }
}

impl Task {
    pub fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(self.ls));

        cmd.run()
    }
}
