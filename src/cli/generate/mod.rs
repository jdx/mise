use clap::Subcommand;

mod bootstrap;
mod config;
mod devcontainer;
mod git_pre_commit;
mod github_action;
mod task_docs;
mod task_stubs;

/// [experimental] Generate files for various tools/services
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "gen", alias = "g")]
pub struct Generate {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Bootstrap(bootstrap::Bootstrap),
    Config(config::Config),
    Devcontainer(devcontainer::Devcontainer),
    GitPreCommit(git_pre_commit::GitPreCommit),
    GithubAction(github_action::GithubAction),
    TaskDocs(task_docs::TaskDocs),
    TaskStubs(task_stubs::TaskStubs),
}

impl Commands {
    pub async fn run(self) -> eyre::Result<()> {
        match self {
            Self::Bootstrap(cmd) => cmd.run().await,
            Self::Config(cmd) => cmd.run().await,
            Self::Devcontainer(cmd) => cmd.run().await,
            Self::GitPreCommit(cmd) => cmd.run().await,
            Self::GithubAction(cmd) => cmd.run().await,
            Self::TaskDocs(cmd) => cmd.run().await,
            Self::TaskStubs(cmd) => cmd.run().await,
        }
    }
}

impl Generate {
    pub async fn run(self) -> eyre::Result<()> {
        self.command.run().await
    }
}
