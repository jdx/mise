use clap::Subcommand;

mod git_pre_commit;
mod github_action;

/// [experimental] Generate files for various tools/services
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "gen")]
pub struct Generate {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    GitPreCommit(git_pre_commit::GitPreCommit),
    GithubAction(github_action::GithubAction),
}

impl Commands {
    pub async fn run(self) -> eyre::Result<()> {
        match self {
            Self::GitPreCommit(cmd) => cmd.run().await,
            Self::GithubAction(cmd) => cmd.run().await,
        }
    }
}

impl Generate {
    pub async fn run(self) -> eyre::Result<()> {
        self.command.run().await
    }
}
