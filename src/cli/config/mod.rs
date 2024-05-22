use clap::Subcommand;
use eyre::Result;

mod generate;
mod ls;

/// [experimental] Manage config files
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "cfg")]
pub struct Config {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Do not print table header
    #[clap(long, alias = "no-headers", verbatim_doc_comment)]
    no_header: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Ls(ls::ConfigLs),
    Generate(generate::ConfigGenerate),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Ls(cmd) => cmd.run().await,
            Self::Generate(cmd) => cmd.run().await,
        }
    }
}

impl Config {
    pub async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::ConfigLs {
            no_header: self.no_header,
        }));

        cmd.run().await
    }
}
