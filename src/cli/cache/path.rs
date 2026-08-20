use eyre::Result;

use crate::env;

/// Show the cache directory path
#[derive(Debug, usage_rs::Args)]
#[command(verbatim_doc_comment, visible_alias = "dir")]
pub struct CachePath {}

impl CachePath {
    pub(super) fn run(self) -> Result<()> {
        miseprintln!("{}", env::MISE_CACHE_DIR.display());
        Ok(())
    }
}
