use clap::Subcommand;
use eyre::{Result, bail};

use crate::cli::run;

mod add;
mod deps;
mod edit;
mod graph;
mod info;
mod ls;
mod validate;

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
    Add(Box<add::TasksAdd>),
    Deps(deps::TasksDeps),
    Edit(edit::TasksEdit),
    Graph(graph::TasksGraph),
    Info(info::TasksInfo),
    Ls(ls::TasksLs),
    Run(Box<run::Run>),
    Validate(validate::TasksValidate),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Add(cmd) => (*cmd).run().await,
            Self::Deps(cmd) => cmd.run().await,
            Self::Edit(cmd) => cmd.run().await,
            Self::Graph(cmd) => cmd.run().await,
            Self::Info(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run().await,
            Self::Run(cmd) => (*cmd).run().await,
            Self::Validate(cmd) => cmd.run().await,
        }
    }
}

impl Tasks {
    pub async fn run(self) -> Result<()> {
        let Self { command, task, ls } = self;
        let cmd = match command {
            Some(Commands::Ls(cmd)) => Commands::Ls(ls.merge(cmd)),
            Some(cmd) => {
                if ls.has_options() {
                    bail!("task list options cannot be used with subcommands");
                }
                cmd
            }
            None => task
                .map(|task| {
                    Commands::Info(info::TasksInfo {
                        task,
                        json: ls.json,
                    })
                })
                .unwrap_or(Commands::Ls(ls)),
        };

        cmd.run().await
    }
}
