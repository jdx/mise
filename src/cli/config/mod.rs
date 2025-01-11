use clap::Subcommand;
use eyre::Result;

pub(crate) mod generate;
mod get;
mod ls;
mod set;

/// Manage config files
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "cfg", alias = "toml")]
pub struct Config {
    #[clap(subcommand)]
    command: Option<Commands>,

    #[clap(flatten)]
    pub ls: ls::ConfigLs,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Generate(generate::ConfigGenerate),
    Get(get::ConfigGet),
    #[clap(visible_alias = "list")]
    Ls(ls::ConfigLs),
    Set(set::ConfigSet),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Generate(cmd) => cmd.run(),
            Self::Get(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run(),
            Self::Set(cmd) => cmd.run(),
        }
    }
}

impl Config {
    pub fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(self.ls));

        cmd.run()
    }
}
