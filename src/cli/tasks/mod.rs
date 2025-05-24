use clap::Subcommand;
use eyre::Result;

use crate::cli::run;

mod add;
mod deps;
mod edit;
mod info;
mod ls;

/// Manage tasks
#[derive(clap::Args)]
#[clap(visible_alias = "t", alias = "task", verbatim_doc_comment)]
pub struct Tasks {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Task name to get info of
    task: Option<String>,

    #[clap(flatten)]
    ls: ls::TasksLs,
}

#[derive(Subcommand)]
enum Commands {
    Add(add::TasksAdd),
    Deps(deps::TasksDeps),
    Edit(edit::TasksEdit),
    Info(info::TasksInfo),
    Ls(ls::TasksLs),
    Run(run::Run),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Add(cmd) => cmd.run().await,
            Self::Deps(cmd) => cmd.run().await,
            Self::Edit(cmd) => cmd.run().await,
            Self::Info(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run().await,
            Self::Run(cmd) => cmd.run().await,
        }
    }
}

impl Tasks {
    pub async fn run(self) -> Result<()> {
        let cmd = self
            .command
            .or(self.task.map(|t| {
                Commands::Info(info::TasksInfo {
                    task: t,
                    json: self.ls.json,
                })
            }))
            .unwrap_or(Commands::Ls(self.ls));

        cmd.run().await
    }
}
