use std::path::Path;

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
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let mut files = vec![&*dirs::ROOT, &*dirs::CACHE, &*env::RTX_EXE];
        if self.config {
            files.push(&*dirs::CONFIG);
        }
        for f in files.into_iter().filter(|d| d.exists()) {
            if self.dry_run {
                rtxprintln!(out, "rm -rf {}", f.display());
            }

            if self.confirm_remove(&config, f)? {
                if f.is_dir() {
                    remove_all(f)?;
                } else {
                    file::remove_file(f)?;
                }
            }
        }

        Ok(())
    }
}

impl Implode {
    fn confirm_remove(&self, config: &Config, f: &Path) -> Result<bool> {
        if self.dry_run {
            Ok(false)
        } else if config.settings.yes {
            Ok(true)
        } else {
            let r = prompt::confirm(&format!("remove {} ?", f.display()))?;
            Ok(r)
        }
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
