use clap::Subcommand;
use eyre::Result;

use crate::env;

mod clear;
mod prune;

/// Manage the mise cache
///
/// Run `mise cache` with no args to view the current cache directory.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Cache {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Clear(clear::CacheClear),
    Prune(prune::CachePrune),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Clear(cmd) => cmd.run(),
            Self::Prune(cmd) => cmd.run(),
        }
    }
}

impl Cache {
    pub fn run(self) -> Result<()> {
        match self.command {
            Some(cmd) => cmd.run(),
            None => {
                // just show the cache dir
                miseprintln!("{}", env::MISE_CACHE_DIR.display());
                Ok(())
            }
        }
    }
}
