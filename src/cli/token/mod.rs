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
    /// Show the Forgejo token mise will use
    Forgejo(forgejo::Forgejo),
    /// Show the GitHub token mise will use
    Github(github::Github),
    /// Show the GitLab token mise will use
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
