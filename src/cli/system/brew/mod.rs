use eyre::Result;

use crate::system::history::OperationScope;

pub(super) mod tap;
pub(super) mod untap;

/// Manage Homebrew tap URLs in `[bootstrap.brew.taps]`
///
/// mise finds a third-party tap such as `acme/tools` at
/// `https://github.com/acme/homebrew-tools` without an entry. Add one only
/// when the tap's repository lives at a different GitHub URL; only GitHub taps
/// are supported. These commands edit config; they do not run Homebrew.
///
/// Not available on Windows.
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
                OperationScope::wrap("bootstrap packages brew tap", dry_run, async { cmd.run() })
                    .await
            }
            Commands::Untap(cmd) => {
                let dry_run = cmd.dry_run;
                OperationScope::wrap("bootstrap packages brew untap", dry_run, async {
                    cmd.run()
                })
                .await
            }
        }
    }
}
