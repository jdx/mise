use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

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
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match self {
            Self::Get(cmd) => cmd.run(config, out),
            Self::Ls(cmd) => cmd.run(config, out),
            Self::Set(cmd) => cmd.run(config, out),
            Self::Unset(cmd) => cmd.run(config, out),
        }
    }
}

impl Command for Settings {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::SettingsLs {}));

        cmd.run(config, out)
    }
}
