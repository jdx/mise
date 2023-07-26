use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::file::remove_all;
use crate::output::Output;
use crate::{dirs, env};

/// Removes rtx CLI and all related data
///
/// Skips config directory by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Implode {
    /// Also remove config directory
    #[clap(long, verbatim_doc_comment)]
    config: bool,

    /// List directories that would be removed without actually removing them [default: true]
    #[clap(long, verbatim_doc_comment, default_value_t = true, hide = true)]
     dry_run: bool,

    /// This will remove your rtx [default: false]
    #[clap(long, verbatim_doc_comment, default_value_t = false)]
    no_dry_run: bool,
}

impl Command for Implode {
    fn run(mut self, _config: Config, out: &mut Output) -> Result<()> {
        if self.no_dry_run {
            self.dry_run = false;
        }

        if self.dry_run && !self.no_dry_run {
            rtxprintln!(out, "Running in dry-run mode. If you know what you're doing, run with --no-dry-run");
        }

        let mut files = vec![&*dirs::ROOT, &*dirs::CACHE, &*env::RTX_EXE];
        if self.config {
            files.push(&*dirs::CONFIG);
        }
        for f in files.into_iter().filter(|d| d.exists()) {
            if f.is_dir() {
                rtxprintln!(out, "rm -rf {}", f.display());
                if self.no_dry_run {
                    remove_all(f)?;
                }
            } else {
                rtxprintln!(out, "rm -f {}", f.display());
                if self.no_dry_run {
                    std::fs::remove_file(f)?;
                }
            }
        }

        Ok(())
    }
}


#[cfg(test)]
#[cfg(test)]
mod tests {
    use crate::assert_cli;
    use crate::dirs;

    #[test]
    fn test_implode() {
        let stdout = assert_cli!("implode", "--config", "--dry-run");
        assert!(stdout.contains(format!("rm -rf {}", dirs::ROOT.display()).as_str()));
        assert!(stdout.contains(format!("rm -rf {}", dirs::CACHE.display()).as_str()));
        assert!(stdout.contains(format!("rm -rf {}", dirs::CONFIG.display()).as_str()));
    }
}
