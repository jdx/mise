use std::path::PathBuf;

use crate::config::Config;
use crate::config::config_file::config_root;
use crate::config::config_file::config_trust_root;
use crate::config::{
    ALL_CONFIG_FILES, DEFAULT_CONFIG_FILENAMES, Settings, config_file, config_files_in_dir,
    is_global_config,
};
use crate::file::{display_path, remove_file};
use crate::{config, dirs, env, file, hooks};
use clap::ValueHint;
use eyre::Result;
use itertools::Itertools;

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

    /// Show the trusted status of config files from the current directory and its parents.
    /// Does not trust or untrust any files.
    #[clap(long, verbatim_doc_comment)]
    show: bool,

    /// No longer trust this config, will prompt in the future
    #[clap(long)]
    untrust: bool,
}

impl Trust {
    pub async fn run(mut self) -> Result<()> {
        if self.show {
            return self.show();
        }
        if self.untrust {
            untrust_config_file(self.config_file())
        } else if self.ignore {
            self.ignore()
        } else if self.all {
            let mut trusted_roots = vec![];
            while let Some(p) = self.get_next_untrusted() {
                self.config_file = Some(p);
                if let Some(root) = self.trust()? {
                    trusted_roots.push(root);
                }
            }
            self.run_enter_hooks(trusted_roots).await
        } else {
            let trusted_roots = self.trust()?.into_iter().collect();
            self.run_enter_hooks(trusted_roots).await
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
}

pub(super) fn untrust_config_file(config_file: Option<PathBuf>) -> Result<()> {
    let path = match config_file {
        Some(filename) => filename,
        None => match ALL_CONFIG_FILES.first().cloned() {
            Some(path) => path,
            None => {
                warn!("No trusted config files found.");
                return Ok(());
            }
        },
    };
    let cfr = config_trust_root(&path);
    config_file::untrust(&cfr)?;
    let cfr = cfr.canonicalize()?;
    info!("untrusted {}", cfr.display());

    let trusted_via_settings = Settings::get()
        .trusted_config_paths()
        .any(|p| cfr.starts_with(p));
    if trusted_via_settings {
        warn!("{cfr:?} is trusted via settings so it will still be trusted.");
    }

    Ok(())
}

pub(super) fn resolve_config_file(config_file: Option<&PathBuf>) -> Option<PathBuf> {
    config_file.map(|config_file| {
        if config_file.is_dir() {
            config_files_in_dir(config_file)
                .last()
                .cloned()
                .unwrap_or(config_file.join(&*env::MISE_DEFAULT_CONFIG_FILENAME))
        } else {
            config_file.clone()
        }
    })
}

impl Trust {
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
        let cfr = config_trust_root(&path);
        config_file::add_ignored(cfr.clone())?;
        let cfr = cfr.canonicalize()?;
        info!("ignored {}", cfr.display());

        let trusted_via_settings = Settings::get()
            .trusted_config_paths()
            .any(|p| cfr.starts_with(p));
        if trusted_via_settings {
            warn!("{cfr:?} is trusted via settings so it will still be trusted.");
        }
        Ok(())
    }
    fn trust(&self) -> Result<Option<PathBuf>> {
        let path = match self.config_file() {
            Some(filename) => config_trust_root(&filename),
            None => match self.get_next_untrusted() {
                Some(path) => path,
                None => {
                    warn!("No untrusted config files found.");
                    return Ok(None);
                }
            },
        };
        let was_trusted = config_file::is_trusted(&path);
        let hook_root = hook_root(&path);
        config_file::trust(&path)?;
        let cfr = path.canonicalize()?;
        info!("trusted {}", cfr.display());
        let should_run_enter = !was_trusted
            && dirs::CWD
                .as_ref()
                .is_some_and(|cwd| cwd.starts_with(&hook_root));
        Ok(should_run_enter.then_some(hook_root))
    }

    fn config_file(&self) -> Option<PathBuf> {
        resolve_config_file(self.config_file.as_ref())
    }

    fn get_next(&self) -> Option<PathBuf> {
        ALL_CONFIG_FILES.first().cloned()
    }
    fn get_next_untrusted(&self) -> Option<PathBuf> {
        config::load_config_paths(&DEFAULT_CONFIG_FILENAMES, true)
            .into_iter()
            .filter(|p| !is_global_config(p))
            .map(|p| config_trust_root(&p))
            .unique()
            .find(|ctr| !config_file::is_trusted(ctr))
    }

    fn show(&self) -> Result<()> {
        let trusted = config::load_config_paths(&DEFAULT_CONFIG_FILENAMES, true)
            .into_iter()
            .filter(|p| !is_global_config(p))
            .map(|p| config_trust_root(&p))
            .unique()
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

    async fn run_enter_hooks(&self, trusted_roots: Vec<PathBuf>) -> Result<()> {
        if trusted_roots.is_empty() || self.get_next_untrusted().is_some() {
            return Ok(());
        }
        let config = if config::is_loaded() {
            Config::reset().await?
        } else {
            Config::get().await?
        };
        let ts = config.get_toolset().await?;
        let trusted_roots = trusted_roots.into_iter().unique().collect_vec();
        hooks::run_enter_hooks_for_roots(&config, &ts, &trusted_roots).await;
        Ok(())
    }
}

fn hook_root(path: &std::path::Path) -> PathBuf {
    if path.is_file() {
        config_root::config_root(path)
    } else {
        path.to_path_buf()
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
