use eyre::Result;

mod ls;

/// [experimental] Inspect the project's secrets source
///
/// With no subcommand, lists secret names (same as `mise secrets ls`; the flags
/// below are passed to it). See https://mise.jdx.dev/environments/secrets/fnox.html
#[derive(Debug, usage_rs::Args)]
#[usage(name = "secrets", verbatim_doc_comment)]
pub(crate) struct Secrets {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Output in JSON format
    #[usage(long, short = 'J')]
    pub json: bool,

    /// Do not print the table header
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
