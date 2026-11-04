use std::path::PathBuf;

use crate::Result;

use super::trust;

/// Remove explicit trust for a config
///
/// mise asks again before loading the parts of the file that can run code. With
/// no file, untrusts the nearest config in this directory or a parent. Same as
/// `mise trust --untrust`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise untrust ~/src/app/mise.toml"###,
        help = "Stop trusting a specific file"
    )
)]
pub(crate) struct Untrust {
    /// The config file to untrust
    #[usage(value_hint = ValueHint::FilePath, verbatim_doc_comment)]
    config_file: Option<PathBuf>,
}

impl Untrust {
    pub(crate) async fn run(self) -> Result<()> {
        let config_file = self.config_file()?;
        // Only a pathless untrust looks for config, and that loads the tool backends.
        let filenames = match config_file {
            Some(_) => vec![],
            None => crate::config::trust_config_filenames().await,
        };
        trust::untrust_config_file(config_file, &filenames)
    }

    fn config_file(&self) -> Result<Option<PathBuf>> {
        trust::resolve_config_file(self.config_file.as_ref())
    }
}
