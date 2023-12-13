use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::config::Config;
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
}

#[derive(Debug, Subcommand)]
enum Commands {
    Get(get::AliasGet),
    Ls(ls::AliasLs),
    Set(set::AliasSet),
    Unset(unset::AliasUnset),
}

impl Commands {
    pub fn run(self, config: Config) -> Result<()> {
        match self {
            Self::Get(cmd) => cmd.run(config),
            Self::Ls(cmd) => cmd.run(config),
            Self::Set(cmd) => cmd.run(config),
            Self::Unset(cmd) => cmd.run(config),
        }
    }
}

impl Alias {
    pub fn run(self, config: Config) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::AliasLs {
            plugin: self.plugin,
        }));

        cmd.run(config)
    }
}
