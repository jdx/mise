mod token;

/// GitHub related commands
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true)]
pub struct Github {
    #[clap(subcommand)]
    subcommand: Commands,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    Token(token::Token),
}

impl Github {
    pub async fn run(self) -> eyre::Result<()> {
        deprecated_at!(
            "2026.5.1",
            "2027.5.0",
            "cli.github",
            "`mise github ...` is deprecated. Use `mise token github` instead."
        );
        match self.subcommand {
            Commands::Token(cmd) => cmd.run(),
        }
    }
}
