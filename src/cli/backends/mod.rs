use clap::Subcommand;
use eyre::Result;

mod ls;

#[derive(Debug, clap::Args)]
#[clap(about = "Manage backends", visible_alias = "b", aliases = ["backend", "backend-list"])]
pub struct Backends {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Ls(ls::BackendsLs),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Ls(cmd) => cmd.run(),
        }
    }
}

impl Backends {
    pub fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::BackendsLs {}));

        cmd.run()
    }
}
