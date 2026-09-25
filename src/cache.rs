use std::path::PathBuf;

use eyre::Result;
use path_absolutize::Absolutize;

use crate::build_time::built_info;
use crate::config::{Settings, SettingsExt};
use crate::dirs;
use crate::toolset::env_cache::CachedEnv;

pub(crate) use mise_util::cache::*;

/// Register this build's identity as the base of every cache key. Runs in
/// `main` and in the test harness before anything opens a cache.
pub(crate) fn register_base_cache_keys() {
    set_base_cache_keys(
        [
            built_info::FEATURES_STR,
            built_info::PKG_VERSION,
            built_info::PROFILE,
            built_info::TARGET,
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect(),
    );
}

/// Returns every cache root maintained by whole-cache clear and prune operations.
pub(crate) fn cache_dirs() -> Result<Vec<PathBuf>> {
    cache_dirs_with_task_cache(crate::task::task_cache::task_cache_dir())
}

/// Adds an external task cache to the global cache roots without double-scanning
/// task caches already stored beneath `MISE_CACHE_DIR`.
fn cache_dirs_with_task_cache(task_cache_dir: PathBuf) -> Result<Vec<PathBuf>> {
    let cache_root = dirs::CACHE.absolutize()?.to_path_buf();
    let task_cache_dir = task_cache_dir.absolutize()?.to_path_buf();
    let mut cache_dirs = vec![cache_root.clone()];
    if !task_cache_dir.starts_with(cache_root) {
        cache_dirs.push(task_cache_dir);
    }
    Ok(cache_dirs)
}

/// Opportunistically removes stale files from each active cache root.
///
/// Each external root keeps its own marker so one project's task cache cannot
/// suppress automatic pruning for another project that shares `MISE_CACHE_DIR`.
pub(crate) fn auto_prune() -> Result<()> {
    if !rand::random::<u8>().is_multiple_of(100) {
        return Ok(()); // only prune 1% of the time
    }
    let settings = Settings::get();
    let age = match settings.cache_prune_age_duration() {
        Some(age) => age,
        None => {
            return Ok(());
        }
    };
    let cache_dirs = cache_dirs()?;
    let opts = PruneOptions {
        dry_run: false,
        verbose: false,
        age,
    };
    let mut prune_env_cache = false;
    for (index, cache_dir) in cache_dirs.into_iter().enumerate() {
        if prepare_auto_prune_root(&cache_dir, age)? {
            debug!(
                "pruning old cache files, this behavior can be modified with the MISE_CACHE_PRUNE_AGE setting"
            );
            prune(&cache_dir, &opts)?;
            if index == 0 {
                prune_env_cache = true;
            }
        }
    }
    // Also prune env cache using env_cache_ttl
    let env_cache_dir = CachedEnv::cache_dir();
    if prune_env_cache && env_cache_dir.exists() {
        let env_opts = PruneOptions {
            dry_run: false,
            verbose: false,
            age: settings.env_cache_ttl(),
        };
        prune(&env_cache_dir, &env_opts)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn test_cache() {
        let _config = Config::get().await.unwrap();
        let mut cache = CacheManagerBuilder::new(dirs::CACHE.join("test-cache")).build();
        cache.clear().unwrap();
        let val = cache.get_or_try_init(|| Ok(1)).unwrap();
        assert_eq!(val, &1);
        let val = cache.get_or_try_init(|| Ok(2)).unwrap();
        assert_eq!(val, &1);
    }

    #[tokio::test]
    async fn test_refresh_ignores_memory_and_file_cache() {
        let _config = Config::get().await.unwrap();
        let mut cache: CacheManager<i32> =
            CacheManagerBuilder::new(dirs::CACHE.join("test-cache-refresh")).build();
        cache.clear().unwrap();
        let val = cache
            .get_or_try_init_async(|| async { Ok(1) })
            .await
            .unwrap();
        assert_eq!(val, &1);

        let val = cache.refresh_async(|| async { Ok(2) }).await.unwrap();

        assert_eq!(val, 2);

        // After refresh, the in-memory cells must observe the fresh value too.
        let val = cache
            .get_or_try_init_async(|| async { Ok(3) })
            .await
            .unwrap();
        assert_eq!(val, &2);
        let val = cache.get_or_try_init(|| Ok(4)).unwrap();
        assert_eq!(val, &2);
    }

    #[tokio::test]
    async fn test_get_or_try_init_async_if_does_not_cache_rejected_values() {
        let _config = Config::get().await.unwrap();
        let mut cache: CacheManager<i32> =
            CacheManagerBuilder::new(dirs::CACHE.join("test-cache-if")).build();
        cache.clear().unwrap();

        let val = cache
            .get_or_try_init_async_if(|| async { Ok(1) }, |v| *v > 1)
            .await
            .unwrap();
        assert_eq!(val, 1);

        let val = cache
            .get_or_try_init_async_if(|| async { Ok(2) }, |v| *v > 1)
            .await
            .unwrap();
        assert_eq!(val, 2);

        let val = cache
            .get_or_try_init_async_if(|| async { Ok(3) }, |v| *v > 1)
            .await
            .unwrap();
        assert_eq!(val, 2);
    }

    #[test]
    fn cache_dirs_adds_only_external_task_cache() {
        let external = tempfile::tempdir().unwrap();
        assert_eq!(
            cache_dirs_with_task_cache(external.path().to_path_buf()).unwrap(),
            vec![dirs::CACHE.to_path_buf(), external.path().to_path_buf()]
        );

        let nested = dirs::CACHE.join("task-artifacts").join("v2");
        assert_eq!(
            cache_dirs_with_task_cache(nested).unwrap(),
            vec![dirs::CACHE.to_path_buf()]
        );

        let external = dirs::CACHE
            .parent()
            .unwrap()
            .join("external-task-cache")
            .join("v2");
        let escaping = dirs::CACHE
            .join("..")
            .join("external-task-cache")
            .join("v2");
        assert_eq!(
            cache_dirs_with_task_cache(escaping).unwrap(),
            vec![dirs::CACHE.to_path_buf(), external]
        );
    }
}
