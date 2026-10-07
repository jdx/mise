use eyre::Result;

mod ls;

#[derive(Debug, usage_rs::Args)]
#[usage(
    name = "secrets",
    about = "[experimental] List the secret names this project's secrets source provides, without their values"
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
    pub(crate) async fn run(self, json: bool, no_header: bool) -> Result<()> {
        match self {
            Self::Ls(mut cmd) => {
                cmd.json |= json;
                cmd.no_header |= no_header;
                cmd.run().await
            }
        }
    }
}

impl Secrets {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::SecretsLs {
            json: false,
            no_header: false,
            complete: false,
        }));

        cmd.run(self.json, self.no_header).await
    }
}
