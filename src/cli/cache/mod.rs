use clap::Subcommand;
use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::env;
use crate::output::Output;

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
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match self {
            Self::Clear(cmd) => cmd.run(config, out),
        }
    }
}

impl Command for Cache {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match self.command {
            Some(cmd) => cmd.run(config, out),
            None => {
                // just show the cache dir
                rtxprintln!(out, "{}", env::RTX_CACHE_DIR.display());
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;
    use crate::env;

    #[test]
    fn test_cache() {
        let stdout = assert_cli!("cache");
        assert_str_eq!(stdout.trim(), env::RTX_CACHE_DIR.display().to_string());
    }
}
