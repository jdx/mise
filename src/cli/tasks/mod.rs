use clap::Subcommand;
use eyre::Result;

use crate::cli::run;

mod add;
mod deps;
mod edit;
mod info;
mod ls;

/// Manage tasks
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "t", alias = "task", verbatim_doc_comment)]
pub struct Tasks {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Task name to get info of
    task: Option<String>,

    #[clap(flatten)]
    ls: ls::TasksLs,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Add(add::TasksAdd),
    Deps(deps::TasksDeps),
    Edit(edit::TasksEdit),
    Info(info::TasksInfo),
    Ls(ls::TasksLs),
    Run(run::Run),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Add(cmd) => cmd.run(),
            Self::Deps(cmd) => cmd.run(),
            Self::Edit(cmd) => cmd.run(),
            Self::Info(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run(),
            Self::Run(cmd) => cmd.run(),
        }
    }
}

impl Tasks {
    pub fn run(self) -> Result<()> {
        let cmd = self
            .command
            .or(self.task.map(|t| {
                Commands::Info(info::TasksInfo {
                    task: t,
                    json: self.ls.json,
                })
            }))
            .unwrap_or(Commands::Ls(self.ls));

        cmd.run()
    }
}
