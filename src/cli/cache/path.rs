use eyre::Result;

use crate::env;

/// Show the cache directory path
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, visible_alias = "dir")]
pub(super) struct CachePath {}

impl CachePath {
    pub(super) fn run(self) -> Result<()> {
        miseprintln!("{}", env::MISE_CACHE_DIR.display());
        Ok(())
    }
}
