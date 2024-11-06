use crate::dirs::CACHE;
use crate::file::{display_path, remove_all};
use eyre::Result;
use filetime::set_file_times;
use walkdir::WalkDir;

/// Deletes all cache files in mise
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "c", alias = "clean")]
pub struct CacheClear {
    /// Plugin(s) to clear cache for
    /// e.g.: node, python
    plugin: Option<Vec<String>>,

    /// Mark all cache files as old
    #[clap(long, hide = true)]
    outdate: bool,
}

impl CacheClear {
    pub fn run(self) -> Result<()> {
        let cache_dirs = match &self.plugin {
            Some(plugins) => plugins.iter().map(|p| CACHE.join(p)).collect(),
            None => vec![CACHE.to_path_buf()],
        };
        if self.outdate {
            for p in cache_dirs {
                if p.exists() {
                    debug!("outdating cache from {}", display_path(&p));
                    let files = WalkDir::new(&p)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().is_file() || e.file_type().is_dir());
                    for e in files {
                        set_file_times(
                            e.path(),
                            filetime::FileTime::zero(),
                            filetime::FileTime::zero(),
                        )?;
                    }
                }
            }
        } else {
            for p in cache_dirs {
                if p.exists() {
                    debug!("clearing cache from {}", display_path(&p));
                    remove_all(p)?;
                }
            }
            match &self.plugin {
                Some(plugins) => info!("cache cleared for {}", plugins.join(", ")),
                None => info!("cache cleared"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_cache_clear() {
        reset();
        assert_cli_snapshot!("cache", "clear", @r###"
        mise cache cleared
        "###);
    }

    #[test]
    fn test_cache_clear_plugin() {
        reset();
        assert_cli_snapshot!("cache", "clear", "tiny", @r###"
        mise cache cleared for tiny
        "###);
    }
}
