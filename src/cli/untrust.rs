use std::path::PathBuf;

use crate::config::config_files_in_dir;
use crate::{Result, env};
use clap::ValueHint;

use super::trust;

/// No longer trust a config, will prompt in the future
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Untrust {
    /// The config file to untrust
    #[clap(value_hint = ValueHint::FilePath, verbatim_doc_comment)]
    config_file: Option<PathBuf>,
}

impl Untrust {
    pub fn run(self) -> Result<()> {
        trust::untrust_config_file(self.config_file())
    }

    fn config_file(&self) -> Option<PathBuf> {
        self.config_file.as_ref().map(|config_file| {
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
}
