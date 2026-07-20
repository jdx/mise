use crate::dirs::CACHE;
use crate::file::{display_path, remove_all_with_retry};
use crate::toolset::env_cache::CachedEnv;
use eyre::Result;
use filetime::set_file_times;
use heck::ToKebabCase;
use walkdir::WalkDir;

/// Deletes all cache files in mise
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "c", alias = "clean")]
pub struct CacheClear {
    /// Tool(s) to clear cache for
    /// e.g.: node, python
    tool: Option<Vec<String>>,

    /// Mark all cache files as old
    #[clap(long, hide = true)]
    outdate: bool,
}

impl CacheClear {
    pub fn run(self) -> Result<()> {
        let cache_dirs = match &self.tool {
            Some(tools) => tools
                .iter()
                .filter_map(|p| {
                    let kebab = p.to_kebab_case();
                    if kebab.is_empty() {
                        warn!("invalid tool name: {p}");
                        None
                    } else {
                        Some(CACHE.join(kebab))
                    }
                })
                .collect(),
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
                    handle_remove_result(&p, remove_all_with_retry(&p))?;
                }
            }
            // Also clear env cache when clearing all caches
            if self.tool.is_none() {
                CachedEnv::clear()?;
            }
            match &self.tool {
                Some(tools) => info!("cache cleared for {}", tools.join(", ")),
                None => info!("cache cleared"),
            }
        }
        Ok(())
    }
}

fn handle_remove_result(path: &std::path::Path, result: Result<()>) -> Result<()> {
    match result {
        Err(err)
            if err
                .downcast_ref::<std::io::Error>()
                .is_some_and(|err| err.kind() == std::io::ErrorKind::DirectoryNotEmpty) =>
        {
            debug!(
                "cache was recreated while being cleared: {}",
                display_path(path)
            );
            Ok(())
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use eyre::Context;

    use super::*;

    #[test]
    fn test_handle_remove_result_tolerates_directory_not_empty() {
        let err = Err::<(), _>(std::io::Error::from(std::io::ErrorKind::DirectoryNotEmpty))
            .wrap_err("failed rm -rf")
            .unwrap_err();
        handle_remove_result(std::path::Path::new("cache"), Err(err)).unwrap();
    }

    #[test]
    fn test_handle_remove_result_propagates_other_errors() {
        let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied).into();
        let err = handle_remove_result(std::path::Path::new("cache"), Err(err)).unwrap_err();
        assert_eq!(
            err.downcast_ref::<std::io::Error>().map(|err| err.kind()),
            Some(std::io::ErrorKind::PermissionDenied)
        );
    }
}
