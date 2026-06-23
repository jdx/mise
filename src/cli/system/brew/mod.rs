use clap::Subcommand;
use eyre::Result;

pub(super) mod tap;
pub(super) mod untap;

/// Manage Homebrew taps used by bootstrap packages
///
/// These commands edit `[bootstrap.brew.taps]` so tapped formulae and casks
/// can be fetched directly by mise without a Homebrew installation.
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
        crate::config::Settings::get().ensure_experimental("mise bootstrap")?;
        match self.command {
            Commands::Tap(cmd) => cmd.run(),
            Commands::Untap(cmd) => cmd.run(),
        }
    }
}
