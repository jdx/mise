use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::env;
use crate::file::remove_all;
use crate::output::Output;

/// Deletes all cache files in rtx
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "c", alias = "clean")]
pub struct CacheClear {}

impl Command for CacheClear {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let cache_dir = env::RTX_CACHE_DIR.to_path_buf();
        if cache_dir.exists() {
            debug!("clearing cache from {}", cache_dir.display());
            remove_all(cache_dir)?;
        }
        rtxstatusln!(out, "cache cleared");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::assert_cli;

    #[test]
    fn test_cache_clear() {
        assert_cli!("cache", "clear");
    }
}
