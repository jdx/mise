use clap::Subcommand;

mod git_pre_commit;
mod github_action;
mod task_docs;

/// [experimental] Generate files for various tools/services
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "g")]
pub struct Generate {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    GitPreCommit(git_pre_commit::GitPreCommit),
    GithubAction(github_action::GithubAction),
    TaskDocs(task_docs::TaskDocs),
}

impl Commands {
    pub fn run(self) -> eyre::Result<()> {
        match self {
            Self::GitPreCommit(cmd) => cmd.run(),
            Self::GithubAction(cmd) => cmd.run(),
            Self::TaskDocs(cmd) => cmd.run(),
        }
    }
}

impl Generate {
    pub fn run(self) -> eyre::Result<()> {
        self.command.run()
    }
}
