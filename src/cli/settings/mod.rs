use clap::Subcommand;
use eyre::Result;

mod get;
mod ls;
mod set;
mod unset;

#[derive(Debug, clap::Args)]
#[clap(about = "Manage settings")]
pub struct Settings {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Only display key names for each setting
    #[clap(long, verbatim_doc_comment)]
    keys: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Get(get::SettingsGet),
    Ls(ls::SettingsLs),
    Set(set::SettingsSet),
    Unset(unset::SettingsUnset),
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

impl Settings {
    pub fn run(self) -> Result<()> {
        let cmd = self
            .command
            .unwrap_or(Commands::Ls(ls::SettingsLs { keys: self.keys }));

        cmd.run()
    }
}
