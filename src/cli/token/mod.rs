mod forgejo;
pub(crate) mod github;
mod gitlab;

/// Display git provider tokens mise will use
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Token {
    #[clap(subcommand)]
    subcommand: Commands,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    /// Forgejo token
    Forgejo(forgejo::Forgejo),
    /// GitHub token
    Github(github::Github),
    /// GitLab token
    Gitlab(gitlab::Gitlab),
}

impl Token {
    pub async fn run(self) -> eyre::Result<()> {
        match self.subcommand {
            Commands::Forgejo(cmd) => cmd.run(),
            Commands::Github(cmd) => cmd.run(),
            Commands::Gitlab(cmd) => cmd.run(),
        }
    }
}
