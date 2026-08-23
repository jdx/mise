mod forgejo;
pub(crate) mod github;
mod gitlab;

/// Display git provider tokens mise will use
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct Token {
    #[usage(subcommand)]
    subcommand: Commands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    /// Forgejo token
    Forgejo(forgejo::Forgejo),
    /// GitHub token
    Github(github::Github),
    /// GitLab token
    Gitlab(gitlab::Gitlab),
}

impl Token {
    pub(crate) async fn run(self) -> eyre::Result<()> {
        match self.subcommand {
            Commands::Forgejo(cmd) => cmd.run(),
            Commands::Github(cmd) => cmd.run(),
            Commands::Gitlab(cmd) => cmd.run(),
        }
    }
}
