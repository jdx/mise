use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;

use crate::output::Output;

use crate::{dirs, env};

/// removes rtx CLI and all generated data
///
/// skips config directory by default
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Implode {
    /// also remove config directory
    #[clap(long, verbatim_doc_comment)]
    config: bool,

    /// list directories that would be removed without actually removing them
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
            if f.is_dir() {
                rtxprintln!(out, "rm -rf {}", f.display());
                if !self.dry_run {
                    std::fs::remove_dir_all(f)?;
                }
            } else {
                rtxprintln!(out, "rm -f {}", f.display());
                if !self.dry_run {
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
    use crate::{dirs, env};

    #[test]
    fn test_implode() {
        let stdout = assert_cli!("implode", "--config", "--dry-run");
        assert!(stdout.contains(format!("rm -rf {}", dirs::ROOT.display()).as_str()));
        assert!(stdout.contains(format!("rm -rf {}", dirs::CACHE.display()).as_str()));
        assert!(stdout.contains(format!("rm -rf {}", dirs::CONFIG.display()).as_str()));
        assert!(stdout.contains(format!("rm -f {}", env::RTX_EXE.display()).as_str()));
    }
}
