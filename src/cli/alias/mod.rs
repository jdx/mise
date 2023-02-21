use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
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
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match self {
            Self::Get(cmd) => cmd.run(config, out),
            Self::Ls(cmd) => cmd.run(config, out),
            Self::Set(cmd) => cmd.run(config, out),
            Self::Unset(cmd) => cmd.run(config, out),
        }
    }
}

impl Command for Alias {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::AliasLs {
            plugin: self.plugin,
        }));

        cmd.run(config, out)
    }
}
