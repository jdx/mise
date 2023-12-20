use eyre::Result;

use crate::dirs::CACHE;
use crate::file::{display_path, remove_all};

/// Deletes all cache files in rtx
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "c", alias = "clean")]
pub struct CacheClear {
    /// Plugin(s) to clear cache for
    /// e.g.: node, python
    plugin: Option<Vec<String>>,
}

impl CacheClear {
    pub fn run(self) -> Result<()> {
        let cache_dirs = match &self.plugin {
            Some(plugins) => plugins.iter().map(|p| CACHE.join(p)).collect(),
            None => vec![CACHE.to_path_buf()],
        };
        for p in cache_dirs {
            if p.exists() {
                debug!("clearing cache from {}", display_path(&p));
                remove_all(p)?;
            }
        }
        match &self.plugin {
            Some(plugins) => rtxstatusln!("cache cleared for {}", plugins.join(", ")),
            None => rtxstatusln!("cache cleared"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_cache_clear() {
        assert_cli_snapshot!("cache", "clear", @r###"
        rtx cache cleared
        "###);
    }

    #[test]
    fn test_cache_clear_plugin() {
        assert_cli_snapshot!("cache", "clear", "tiny", @r###"
        rtx cache cleared for tiny
        "###);
    }
}
