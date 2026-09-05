use eyre::Result;

use crate::system::generations::GenerationScope;

pub(super) mod tap;
pub(super) mod untap;

/// Manage Homebrew taps used by bootstrap packages
///
/// These commands edit `[bootstrap.brew.taps]` so tapped formulae and casks
/// can be fetched directly by mise without a Homebrew installation.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct SystemBrew {
    #[usage(subcommand)]
    command: Commands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Tap(tap::SystemBrewTap),
    Untap(untap::SystemBrewUntap),
}

impl SystemBrew {
    pub(crate) async fn run(self) -> Result<()> {
        match self.command {
            Commands::Tap(cmd) => {
                let dry_run = cmd.dry_run;
                GenerationScope::wrap("bootstrap packages brew tap", "packages", dry_run, async {
                    cmd.run()
                })
                .await
            }
            Commands::Untap(cmd) => {
                let dry_run = cmd.dry_run;
                GenerationScope::wrap(
                    "bootstrap packages brew untap",
                    "packages",
                    dry_run,
                    async { cmd.run() },
                )
                .await
            }
        }
    }
}
