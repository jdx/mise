use crate::cache;
use crate::cache::{PruneOptions, PruneResults};
use crate::config::Settings;
use crate::dirs::CACHE;
use eyre::Result;
use number_prefix::NumberPrefix;
use std::time::Duration;

/// Removes stale mise cache files
///
/// By default, this command will remove files that have not been accessed in 30 days.
/// Change this with the MISE_CACHE_PRUNE_AGE environment variable.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "p")]
pub struct CachePrune {
    /// Plugin(s) to clear cache for
    /// e.g.: node, python
    plugin: Option<Vec<String>>,

    /// Just show what would be pruned
    #[clap(long)]
    dry_run: bool,

    /// Show pruned files
    #[clap(long, short, action = clap::ArgAction::Count)]
    verbose: u8,
}

impl CachePrune {
    pub fn run(self) -> Result<()> {
        let settings = Settings::get();
        let cache_dirs = vec![CACHE.to_path_buf()];
        let opts = PruneOptions {
            dry_run: self.dry_run,
            verbose: self.verbose > 0,
            age: settings
                .cache_prune_age_duration()
                .unwrap_or(Duration::from_secs(30 * 24 * 60 * 60)),
        };
        let mut results = PruneResults { size: 0, count: 0 };
        for p in &cache_dirs {
            let r = cache::prune(p, &opts)?;
            results.size += r.size;
            results.count += r.count;
        }
        let count = results.count;
        let size = bytes_str(results.size);
        info!("cache pruned {count} files, {size} bytes");
        Ok(())
    }
}

fn bytes_str(bytes: u64) -> String {
    match NumberPrefix::binary(bytes as f64) {
        NumberPrefix::Standalone(bytes) => format!("{} bytes", bytes),
        NumberPrefix::Prefixed(prefix, n) => format!("{:.1} {}B", n, prefix),
    }
}

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_cache_prune() {
        reset();
        assert_cli!("cache", "prune");
    }

    #[test]
    fn test_cache_prune_plugin() {
        reset();
        assert_cli!("cache", "prune", "tiny");
    }
}
