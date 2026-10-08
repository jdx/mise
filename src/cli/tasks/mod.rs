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
///
/// With no subcommand, lists tasks like `mise tasks ls` and accepts the same
/// flags. With a task name, shows that task like `mise tasks info`. To run a
/// task, use `mise run`.
#[derive(usage_rs::Args)]
#[usage(
    visible_alias = "t",
    alias = "task",
    example("mise tasks", help = "List tasks"),
    example("mise tasks build", help = "Show the build task"),
    example(
        "mise tasks --hidden --sort source",
        help = "Include hidden tasks, sorted by source"
    )
)]
pub(crate) struct Tasks {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Task to show, as `mise tasks info` does
    task: Option<String>,

    #[usage(flatten)]
    ls: ls::TasksLs,
}

#[derive(usage_rs::Subcommands)]
enum Commands {
    Add(Box<add::TasksAdd>),
    Deps(deps::TasksDeps),
    Edit(edit::TasksEdit),
    Graph(graph::TasksGraph),
    Info(info::TasksInfo),
    Ls(ls::TasksLs),
    /// Run tasks; an alias for `mise run`
    ///
    /// Takes the same arguments and flags as `mise run`. See `mise run --help`
    /// or https://mise.jdx.dev/cli/run.html for the full reference.
    Run(Box<run::Run>),
    Validate(validate::TasksValidate),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
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
    pub(crate) async fn run(self) -> Result<()> {
        let Self { command, task, ls } = self;
        let cmd = match command {
            Some(Commands::Ls(cmd)) => Commands::Ls(ls.merge(cmd)?),
            Some(cmd) => {
                if ls.has_options() {
                    bail!("task list options cannot be used with subcommands");
                }
                cmd
            }
            None => match task {
                Some(task) => {
                    if ls.has_non_json_options() {
                        bail!("task list options cannot be used with task info");
                    }
                    Commands::Info(info::TasksInfo {
                        task,
                        json: ls.json,
                    })
                }
                None => Commands::Ls(ls),
            },
        };

        cmd.run().await
    }
}
