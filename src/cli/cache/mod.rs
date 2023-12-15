use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::env;

mod clear;

/// Manage the rtx cache
///
/// Run `rtx cache` with no args to view the current cache directory.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Cache {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Clear(clear::CacheClear),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Clear(cmd) => cmd.run(),
        }
    }
}

impl Cache {
    pub fn run(self) -> Result<()> {
        match self.command {
            Some(cmd) => cmd.run(),
            None => {
                // just show the cache dir
                rtxprintln!("{}", env::RTX_CACHE_DIR.display());
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::env;

    #[test]
    fn test_cache() {
        let stdout = assert_cli!("cache");
        assert_str_eq!(stdout.trim(), env::RTX_CACHE_DIR.display().to_string());
    }
}
