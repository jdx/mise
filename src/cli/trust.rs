use std::fs::read_dir;
use std::path::PathBuf;

use clap::ValueHint;
use eyre::Result;

use crate::config;
use crate::config::{config_file, DEFAULT_CONFIG_FILENAMES};
use crate::dirs::TRUSTED_CONFIGS;
use crate::file::remove_file;

/// Marks a config file as trusted
///
/// This means mise will parse the file with potentially dangerous
/// features enabled.
///
/// This includes:
/// - environment variables
/// - templates
/// - `path:` plugin versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Trust {
    /// The config file to trust
    #[clap(value_hint = ValueHint::FilePath, verbatim_doc_comment)]
    config_file: Option<String>,

    /// Trust all config files in the current directory and its parents
    #[clap(long, short, verbatim_doc_comment)]
    all: bool,

    /// No longer trust this config
    #[clap(long)]
    untrust: bool,
}

impl Trust {
    pub fn run(self) -> Result<()> {
        if self.untrust {
            self.untrust()
        } else if self.all {
            while self.get_next_untrusted().is_some() {
                self.trust()?;
            }
            Ok(())
        } else {
            self.trust()
        }
    }
    pub fn clean() -> Result<()> {
        if TRUSTED_CONFIGS.is_dir() {
            for path in read_dir(&*TRUSTED_CONFIGS)? {
                let path = path?.path();
                if !path.exists() {
                    remove_file(&path)?;
                }
            }
        }
        Ok(())
    }
    fn untrust(&self) -> Result<()> {
        let path = match &self.config_file {
            Some(filename) => PathBuf::from(filename),
            None => match self.get_next_trusted() {
                Some(path) => path,
                None => bail!("No trusted config files found."),
            },
        };
        let path = path.canonicalize()?;
        config_file::untrust(&path)?;
        info!("untrusted {}", path.display());
        Ok(())
    }
    fn trust(&self) -> Result<()> {
        let path = match &self.config_file {
            Some(filename) => PathBuf::from(filename),
            None => match self.get_next_untrusted() {
                Some(path) => path,
                None => bail!("No untrusted config files found."),
            },
        };
        let path = path.canonicalize()?;
        config_file::trust(&path)?;
        info!("trusted {}", path.display());
        Ok(())
    }

    fn get_next_trusted(&self) -> Option<PathBuf> {
        config::load_config_paths(&DEFAULT_CONFIG_FILENAMES)
            .into_iter()
            .find(|p| config_file::is_trusted(p))
    }
    fn get_next_untrusted(&self) -> Option<PathBuf> {
        config::load_config_paths(&DEFAULT_CONFIG_FILENAMES)
            .into_iter()
            .find(|p| !config_file::is_trusted(p))
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
    # trusts ~/some_dir/.mise.toml
    $ <bold>mise trust ~/some_dir/.mise.toml</bold>

    # trusts .mise.toml in the current or parent directory
    $ <bold>mise trust</bold>
"#
);

#[cfg(test)]
mod tests {
    #[test]
    fn test_trust() {
        assert_cli_snapshot!("trust");
        assert_cli_snapshot!("trust", "--untrust");
        assert_cli_snapshot!("trust", ".test-tool-versions");
        assert_cli_snapshot!("trust", "--untrust", ".test-tool-versions");
    }
}
