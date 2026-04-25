use std::path::PathBuf;

use crate::Result;
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
        trust::resolve_config_file(self.config_file.as_ref())
    }
}
