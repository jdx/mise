use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::config::Config;

mod get;
mod ls;
mod set;
mod unset;

#[derive(Debug, clap::Args)]
#[clap(about = "Manage settings")]
pub struct Settings {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Get(get::SettingsGet),
    Ls(ls::SettingsLs),
    Set(set::SettingsSet),
    Unset(unset::SettingsUnset),
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

impl Settings {
    pub fn run(self, config: Config) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::SettingsLs {}));

        cmd.run(config)
    }
}
