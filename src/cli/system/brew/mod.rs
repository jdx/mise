use clap::Subcommand;
use eyre::Result;

mod tap;
mod untap;

/// Manage Homebrew taps used by system packages
///
/// These commands shell out to Homebrew and do not modify `mise.toml`. Use
/// `[system.brew.taps]` when you want tap sources shared in config.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SystemBrew {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Tap(tap::SystemBrewTap),
    Untap(untap::SystemBrewUntap),
}

impl SystemBrew {
    pub async fn run(self) -> Result<()> {
        crate::config::Settings::get().ensure_experimental("mise system")?;
        match self.command {
            Commands::Tap(cmd) => cmd.run().await,
            Commands::Untap(cmd) => cmd.run().await,
        }
    }
}
