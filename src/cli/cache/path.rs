use eyre::Result;

use crate::env;

/// Show the cache directory path
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "dir")]
pub struct CachePath {}

impl CachePath {
    pub fn run(self) -> Result<()> {
        miseprintln!("{}", env::MISE_CACHE_DIR.display());
        Ok(())
    }
}
