use clap::Subcommand;
use eyre::Result;

use crate::cli::run;

mod deps;
mod edit;
mod ls;

/// [experimental] Manage tasks
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "t", alias = "task", verbatim_doc_comment)]
pub struct Tasks {
    #[clap(subcommand)]
    command: Option<Commands>,

    #[clap(flatten)]
    ls: ls::TasksLs,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Deps(deps::TasksDeps),
    Edit(edit::TasksEdit),
    Ls(ls::TasksLs),
    Run(run::Run),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Deps(cmd) => cmd.run().await,
            Self::Edit(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run().await,
            Self::Run(cmd) => cmd.run().await,
        }
    }
}

impl Tasks {
    pub async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(self.ls));

        cmd.run().await
    }
}
