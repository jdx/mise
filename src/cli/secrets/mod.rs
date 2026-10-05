use eyre::Result;

mod ls;

#[derive(Debug, usage_rs::Args)]
#[usage(
    name = "secrets",
    about = "[experimental] Show the secrets this project's secrets source can give tasks"
)]
pub(crate) struct Secrets {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Output in JSON format
    #[usage(long, short = 'J')]
    pub json: bool,

    /// Don't show table header
    #[usage(long)]
    pub no_header: bool,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Ls(ls::SecretsLs),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Ls(cmd) => cmd.run().await,
        }
    }
}

impl Secrets {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::SecretsLs {
            json: self.json,
            no_header: self.no_header,
        }));

        cmd.run().await
    }
}
