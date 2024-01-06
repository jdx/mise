use clap::Subcommand;
use miette::Result;

use crate::plugins::PluginName;

mod get;
mod ls;
mod set;
mod unset;

#[derive(Debug, clap::Args)]
#[clap(about = "Manage aliases", visible_alias = "a", alias = "aliases")]
pub struct Alias {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// filter aliases by plugin
    #[clap(short, long)]
    pub plugin: Option<PluginName>,

    /// Don't show table header
    #[clap(long)]
    pub no_header: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Get(get::AliasGet),
    Ls(ls::AliasLs),
    Set(set::AliasSet),
    Unset(unset::AliasUnset),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Get(cmd) => cmd.run(),
            Self::Ls(cmd) => cmd.run(),
            Self::Set(cmd) => cmd.run(),
            Self::Unset(cmd) => cmd.run(),
        }
    }
}

impl Alias {
    pub fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::AliasLs {
            plugin: self.plugin,
            no_header: self.no_header,
        }));

        cmd.run()
    }
}
