mod token;

/// GitLab related commands
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Gitlab {
    #[clap(subcommand)]
    subcommand: Commands,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    Token(token::Token),
}

impl Gitlab {
    pub async fn run(self) -> eyre::Result<()> {
        match self.subcommand {
            Commands::Token(cmd) => cmd.run(),
        }
    }
}
