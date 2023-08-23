use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::file::remove_all;
use crate::output::Output;
use crate::ui::prompt;
use crate::{dirs, env, file};

/// Removes rtx CLI and all related data
///
/// Skips config directory by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Implode {
    /// Also remove config directory
    #[clap(long, verbatim_doc_comment)]
    config: bool,

    /// List directories that would be removed without actually removing them
    #[clap(long, verbatim_doc_comment)]
    dry_run: bool,
}

impl Command for Implode {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let mut files = vec![&*dirs::ROOT, &*dirs::CACHE, &*env::RTX_EXE];
        if self.config {
            files.push(&*dirs::CONFIG);
        }
        for f in files.into_iter().filter(|d| d.exists()) {
            if self.dry_run {
                rtxprintln!(out, "rm -rf {}", f.display());
            }

            if f.is_dir() {
                if !self.dry_run && prompt::confirm(&format!("remove {} ?", f.display()))? {
                    remove_all(f)?;
                    return Ok(());
                }
            } else if !self.dry_run && prompt::confirm(&format!("remove {} ?", f.display()))? {
                file::remove_file(f)?;
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
