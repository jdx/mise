use std::path::PathBuf;

use clap::ValueHint;
use eyre::Result;

use crate::config::{config_file, DEFAULT_CONFIG_FILENAMES, SETTINGS};
use crate::file::{display_path, remove_file};
use crate::{config, dirs, file};

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
    config_file: Option<PathBuf>,

    /// Trust all config files in the current directory and its parents
    #[clap(long, short, verbatim_doc_comment, conflicts_with_all = &["ignore", "untrust"])]
    all: bool,

    /// Do not trust this config and ignore it in the future
    #[clap(long, conflicts_with = "untrust")]
    ignore: bool,

    /// No longer trust this config, will prompt in the future
    #[clap(long)]
    untrust: bool,

    /// Show the trusted status of config files from the current directory and its parents.
    /// Does not trust or untrust any files.
    #[clap(long, verbatim_doc_comment)]
    show: bool,
}

impl Trust {
    pub fn run(mut self) -> Result<()> {
        if self.show {
            return self.show();
        }
        if self.untrust {
            self.untrust()
        } else if self.ignore {
            self.ignore()
        } else if self.all {
            while let Some(p) = self.get_next_untrusted() {
                self.config_file = Some(p);
                self.trust()?;
            }
            Ok(())
        } else {
            self.trust()
        }
    }
    pub fn clean() -> Result<()> {
        if dirs::TRUSTED_CONFIGS.is_dir() {
            for path in file::ls(&dirs::TRUSTED_CONFIGS)? {
                if !path.exists() {
                    remove_file(&path)?;
                }
            }
        }
        if dirs::IGNORED_CONFIGS.is_dir() {
            for path in file::ls(&dirs::IGNORED_CONFIGS)? {
                if !path.exists() {
                    remove_file(&path)?;
                }
            }
        }
        Ok(())
    }
    fn untrust(&self) -> Result<()> {
        let path = match self.config_file() {
            Some(filename) => filename,
            None => match self.get_next() {
                Some(path) => path,
                None => {
                    warn!("No trusted config files found.");
                    return Ok(());
                }
            },
        };
        config_file::untrust(&path)?;
        let path = path.canonicalize()?;
        info!("untrusted {}", path.display());

        let trusted_via_settings = SETTINGS.trusted_config_paths().any(|p| path.starts_with(p));
        if trusted_via_settings {
            warn!("{path:?} is trusted via settings so it will still be trusted.");
        }

        Ok(())
    }
    fn ignore(&self) -> Result<()> {
        let path = match self.config_file() {
            Some(filename) => filename,
            None => match self.get_next() {
                Some(path) => path,
                None => {
                    warn!("No trusted config files found.");
                    return Ok(());
                }
            },
        };
        config_file::add_ignored(path.clone())?;
        let path = path.canonicalize()?;
        info!("ignored {}", path.display());

        let trusted_via_settings = SETTINGS.trusted_config_paths().any(|p| path.starts_with(p));
        if trusted_via_settings {
            warn!("{path:?} is trusted via settings so it will still be trusted.");
        }
        Ok(())
    }
    fn trust(&self) -> Result<()> {
        let path = match self.config_file() {
            Some(filename) => filename,
            None => match self.get_next_untrusted() {
                Some(path) => path,
                None => {
                    warn!("No untrusted config files found.");
                    return Ok(());
                }
            },
        };
        config_file::trust(&path)?;
        let path = path.canonicalize()?;
        info!("trusted {}", path.display());
        Ok(())
    }

    fn config_file(&self) -> Option<PathBuf> {
        self.config_file.as_ref().map(|config_file| {
            if config_file.is_dir() {
                for filename in DEFAULT_CONFIG_FILENAMES.iter().rev() {
                    let path = config_file.join(filename);
                    if path.exists() {
                        return path;
                    }
                }
                config_file.join("mise.toml")
            } else {
                config_file.clone()
            }
        })
    }

    fn get_next(&self) -> Option<PathBuf> {
        config::load_config_paths(&DEFAULT_CONFIG_FILENAMES, false)
            .first()
            .cloned()
    }
    fn get_next_untrusted(&self) -> Option<PathBuf> {
        config::load_config_paths(&DEFAULT_CONFIG_FILENAMES, true)
            .into_iter()
            .find(|p| !config_file::is_trusted(p))
    }

    fn show(&self) -> Result<()> {
        let trusted = config::load_config_paths(&DEFAULT_CONFIG_FILENAMES, true)
            .into_iter()
            .map(|p| (display_path(&p), config_file::is_trusted(&p)))
            .rev()
            .collect::<Vec<_>>();
        if trusted.is_empty() {
            info!("No trusted config files found.");
        }
        for (dp, trusted) in trusted {
            if trusted {
                miseprintln!("{dp}: trusted");
            } else {
                miseprintln!("{dp}: untrusted");
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # trusts ~/some_dir/mise.toml
    $ <bold>mise trust ~/some_dir/mise.toml</bold>

    # trusts mise.toml in the current or parent directory
    $ <bold>mise trust</bold>
"#
);
