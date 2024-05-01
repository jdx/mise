use std::path::Path;

use eyre::Result;

use crate::config::Settings;
use crate::file::remove_all;
use crate::ui::prompt;
use crate::{dirs, env, file};

/// Removes mise CLI and all related data
///
/// Skips config directory by default.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Implode {
    /// Also remove config directory
    #[clap(long, verbatim_doc_comment)]
    config: bool,

    /// List directories that would be removed without actually removing them
    #[clap(long, short = 'n', verbatim_doc_comment)]
    dry_run: bool,
}

impl Implode {
    pub fn run(self) -> Result<()> {
        let mut files = vec![*dirs::STATE, *dirs::DATA, *dirs::CACHE, &*env::MISE_BIN];
        if self.config {
            files.push(&dirs::CONFIG);
        }
        for f in files.into_iter().filter(|d| d.exists()) {
            if self.dry_run {
                miseprintln!("rm -rf {}", f.display());
            }

            if self.confirm_remove(f)? {
                if f.is_dir() {
                    remove_all(f)?;
                } else {
                    file::remove_file(f)?;
                }
            }
        }

        Ok(())
    }

    fn confirm_remove(&self, f: &Path) -> Result<bool> {
        let settings = Settings::try_get()?;
        if self.dry_run {
            Ok(false)
        } else if settings.yes {
            Ok(true)
        } else {
            let r = prompt::confirm(format!("remove {} ?", f.display()))?;
            Ok(r)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::dirs;

    #[test]
    fn test_implode() {
        let stdout = assert_cli!("implode", "--config", "--dry-run");
        assert!(stdout.contains(format!("rm -rf {}", dirs::STATE.display()).as_str()));
        assert!(stdout.contains(format!("rm -rf {}", dirs::DATA.display()).as_str()));
        assert!(stdout.contains(format!("rm -rf {}", dirs::CACHE.display()).as_str()));
        assert!(stdout.contains(format!("rm -rf {}", dirs::CONFIG.display()).as_str()));
    }
}
