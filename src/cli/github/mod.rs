mod login;
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
    Login(login::Login),
    Token(token::Token),
}

impl Github {
    pub async fn run(self) -> eyre::Result<()> {
        match self.subcommand {
            Commands::Login(cmd) => cmd.run(),
            Commands::Token(cmd) => cmd.run(),
        }
    }
}
