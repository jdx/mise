use std::cmp::min;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use eyre::{Result, bail};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use path_absolutize::Absolutize;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::sync::LazyLock as Lazy;

use crate::build_time::built_info;
use crate::config::Settings;
use crate::file::{display_path, modified_duration};
use crate::hash::hash_to_str;
use crate::platform::Platform;
use crate::rand::random_string;
use crate::toolset::env_cache::CachedEnv;
use crate::{dirs, file};

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum CacheRemoteMode {
    #[default]
    ReadWrite,
    ReadOnly,
    WriteOnly,
}

impl CacheRemoteMode {
    pub(crate) fn reads(self) -> bool {
        matches!(self, Self::ReadWrite | Self::ReadOnly)
    }

    pub(crate) fn writes(self) -> bool {
        matches!(self, Self::ReadWrite | Self::WriteOnly)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct CacheDigest {
    pub(crate) algorithm: String,
    pub(crate) hash: String,
    pub(crate) size: u64,
}

impl CacheDigest {
    pub(crate) fn blake3(bytes: &[u8]) -> Self {
        Self {
            algorithm: "blake3".into(),
            hash: blake3::hash(bytes).to_hex().to_string(),
            size: bytes.len() as u64,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.algorithm != "blake3" && self.algorithm != "sha256" {
            bail!("unsupported remote cache digest algorithm");
        }
        if self.hash.len() != 64
            || !self
                .hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("invalid remote cache digest");
        }
        Ok(())
    }

    pub(crate) fn matches_bytes(&self, bytes: &[u8]) -> Result<bool> {
        self.validate()?;
        if self.size != bytes.len() as u64 {
            return Ok(false);
        }
        let hash = match self.algorithm.as_str() {
            "blake3" => blake3::hash(bytes).to_hex().to_string(),
            "sha256" => hex::encode(sha2::Sha256::digest(bytes)),
            _ => unreachable!("digest algorithm was validated"),
        };
        Ok(self.hash == hash)
    }

    pub(crate) fn matches_file(&self, path: &Path) -> Result<bool> {
        self.validate()?;
        if self.size != path.metadata()?.len() {
            return Ok(false);
        }
        let hash = match self.algorithm.as_str() {
            "blake3" => crate::hash::file_hash_blake3(path, None)?,
            "sha256" => crate::hash::file_hash_sha256(path, None)?,
            _ => unreachable!("digest algorithm was validated"),
        };
        Ok(self.hash == hash)
    }
}

#[derive(Debug)]
pub struct CacheManagerBuilder {
    cache_file_path: PathBuf,
    cache_keys: Vec<String>,
    fresh_duration: Option<Duration>,
    fresh_files: Vec<PathBuf>,
}

pub static BASE_CACHE_KEYS: Lazy<Vec<String>> = Lazy::new(|| {
    [
        built_info::FEATURES_STR,
        built_info::PKG_VERSION,
        built_info::PROFILE,
        built_info::TARGET,
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
});

impl CacheManagerBuilder {
    pub fn new(cache_file_path: impl AsRef<Path>) -> Self {
        let settings = Settings::get();
        let mut cache_keys = BASE_CACHE_KEYS.clone();
        cache_keys.extend([
            settings.os().to_string(),
            settings.arch().to_string(),
            Platform::current().libc().unwrap_or_default().to_string(),
        ]);
        Self {
            cache_file_path: cache_file_path.as_ref().to_path_buf(),
            cache_keys,
            fresh_files: vec![],
            fresh_duration: None,
        }
    }

    pub fn with_fresh_duration(mut self, duration: Option<Duration>) -> Self {
        self.fresh_duration = duration;
        self
    }

    pub fn with_fresh_file(mut self, path: PathBuf) -> Self {
        self.fresh_files.push(path);
        self
    }

    pub fn with_cache_key(mut self, key: String) -> Self {
        self.cache_keys.push(key);
        self
    }

    fn cache_key(&self) -> String {
        hash_to_str(&self.cache_keys).chars().take(5).collect()
    }

    pub fn build<T>(self) -> CacheManager<T>
    where
        T: Serialize + DeserializeOwned,
    {
        let key = self.cache_key();
        let (base, ext) = file::split_file_name(&self.cache_file_path);
        let mut cache_file_path = self.cache_file_path;
        cache_file_path.set_file_name(format!("{base}-{key}.{ext}"));
        CacheManager {
            cache_file_path,
            cache: Box::new(OnceCell::new()),
            cache_async: Box::new(tokio::sync::OnceCell::new()),
            fresh_files: self.fresh_files,
            fresh_duration: self.fresh_duration,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheManager<T>
where
    T: Serialize + DeserializeOwned,
{
    cache_file_path: PathBuf,
    fresh_duration: Option<Duration>,
    fresh_files: Vec<PathBuf>,
    cache: Box<OnceCell<T>>,
    cache_async: Box<tokio::sync::OnceCell<T>>,
}

impl<T> CacheManager<T>
where
    T: Serialize + DeserializeOwned,
{
    pub fn get_or_try_init<F>(&self, fetch: F) -> Result<&T>
    where
        F: FnOnce() -> Result<T>,
    {
        let val = self.cache.get_or_try_init(|| {
            let path = &self.cache_file_path;
            if self.is_fresh() {
                match self.parse() {
                    Ok(val) => return Ok::<_, color_eyre::Report>(val),
                    Err(err) => {
                        warn!("failed to parse cache file: {} {:#}", path.display(), err);
                    }
                }
            }
            let val = (fetch)()?;
            if let Err(err) = self.write(&val) {
                warn!("failed to write cache file: {} {:#}", path.display(), err);
            }
            Ok(val)
        })?;
        Ok(val)
    }

    pub async fn get_or_try_init_async<F, Fut>(&self, fetch: F) -> Result<&T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let val = self
            .cache_async
            .get_or_try_init(|| async {
                let path = &self.cache_file_path;
                if self.is_fresh() {
                    match self.parse() {
                        Ok(val) => return Ok::<_, color_eyre::Report>(val),
                        Err(err) => {
                            warn!("failed to parse cache file: {} {:#}", path.display(), err);
                        }
                    }
                }
                let val = fetch().await?;
                if let Err(err) = self.write(&val) {
                    warn!("failed to write cache file: {} {:#}", path.display(), err);
                }
                Ok(val)
            })
            .await?;
        Ok(val)
    }

    /// Like [`Self::get_or_try_init_async`], but values rejected by `should_cache`
    /// are returned without populating the in-memory or on-disk cache.
    pub async fn get_or_try_init_async_if<F, Fut, P>(&self, fetch: F, should_cache: P) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
        P: Fn(&T) -> bool,
        T: Clone,
    {
        if let Some(val) = self.cache_async.get().or_else(|| self.cache.get())
            && should_cache(val)
        {
            return Ok(val.clone());
        }

        let path = &self.cache_file_path;
        if self.is_fresh() {
            match self.parse() {
                Ok(val) => {
                    if should_cache(&val) {
                        let _ = self.cache.set(val.clone());
                        let _ = self.cache_async.set(val.clone());
                        return Ok(val);
                    }
                }
                Err(err) => {
                    warn!("failed to parse cache file: {} {:#}", path.display(), err);
                }
            }
        }

        let val = fetch().await?;
        if should_cache(&val) {
            if let Err(err) = self.write(&val) {
                warn!("failed to write cache file: {} {:#}", path.display(), err);
            }
            let _ = self.cache.set(val.clone());
            let _ = self.cache_async.set(val.clone());
        }
        Ok(val)
    }

    /// Fetch fresh data, write it to disk, and return it without consulting
    /// any cache. The in-memory cache cells are replaced with the fresh value
    /// so future non-refresh reads observe it instead of a stale previously-
    /// initialized one.
    pub async fn refresh_async<F, Fut>(&mut self, fetch: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
        T: Clone,
    {
        let val = fetch().await?;
        if let Err(err) = self.write(&val) {
            warn!(
                "failed to write cache file: {} {:#}",
                self.cache_file_path.display(),
                err
            );
        }
        *self.cache = OnceCell::with_value(val.clone());
        *self.cache_async = tokio::sync::OnceCell::new_with(Some(val.clone()));
        Ok(val)
    }

    /// Read the cache file without checking freshness and without fetching or writing.
    pub fn get_cached(&self) -> Result<T>
    where
        T: Clone,
    {
        if let Some(val) = self.cache_async.get() {
            return Ok(val.clone());
        }
        if let Some(val) = self.cache.get() {
            return Ok(val.clone());
        }
        self.parse()
    }

    fn parse(&self) -> Result<T> {
        let path = &self.cache_file_path;
        trace!("reading {}", display_path(path));
        let mut zlib = ZlibDecoder::new(File::open(path)?);
        let mut bytes = Vec::new();
        zlib.read_to_end(&mut bytes)?;
        Ok(rmp_serde::from_slice(&bytes)?)
    }

    pub fn write(&self, val: &T) -> Result<()> {
        trace!("writing {}", display_path(&self.cache_file_path));
        if let Some(parent) = self.cache_file_path.parent() {
            file::create_dir_all(parent)?;
        }
        let partial_path = self
            .cache_file_path
            .with_extension(format!("part-{}", random_string(8)));
        let mut zlib = ZlibEncoder::new(File::create(&partial_path)?, Compression::fast());
        zlib.write_all(&rmp_serde::to_vec_named(&val)?[..])?;
        file::rename(&partial_path, &self.cache_file_path)?;

        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        let path = &self.cache_file_path;
        trace!("clearing cache {}", path.display());
        if path.exists() {
            file::remove_file(path)?;
        }
        *self.cache = Default::default();
        *self.cache_async = Default::default();
        Ok(())
    }

    fn is_fresh(&self) -> bool {
        if !self.cache_file_path.exists() {
            return false;
        }
        if let Some(fresh_duration) = self.freshest_duration()
            && let Ok(metadata) = self.cache_file_path.metadata()
            && let Ok(modified) = metadata.modified()
        {
            return modified.elapsed().unwrap_or_default() < fresh_duration;
        }
        true
    }

    fn freshest_duration(&self) -> Option<Duration> {
        let mut freshest = self.fresh_duration;
        for path in self.fresh_files.iter().unique() {
            let duration = modified_duration(path).unwrap_or_default();
            freshest = Some(match freshest {
                None => duration,
                Some(freshest) => min(freshest, duration),
            })
        }
        freshest
    }
}

pub(crate) struct PruneResults {
    pub(crate) size: u64,
    pub(crate) count: u64,
}

pub(crate) struct PruneOptions {
    pub(crate) dry_run: bool,
    pub(crate) verbose: bool,
    pub(crate) age: Duration,
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

/// Refreshes a cache root's private auto-prune marker and reports whether the
/// root contains entries eligible for a pruning pass.
fn prepare_auto_prune_root(cache_dir: &Path, age: Duration) -> Result<bool> {
    if !cache_dir.exists() {
        return Ok(false);
    }
    let auto_prune_file = cache_dir.join(".auto_prune");
    if let Ok(Ok(modified)) = auto_prune_file.metadata().map(|m| m.modified())
        && modified.elapsed().unwrap_or_default() < age
    {
        return Ok(false);
    }
    let empty = file::ls(cache_dir)?.is_empty();
    xx::file::touch_dir(&auto_prune_file)?;
    Ok(!empty)
}

pub(crate) fn prune(dir: &Path, opts: &PruneOptions) -> Result<PruneResults> {
    let mut results = PruneResults { size: 0, count: 0 };
    let remove = |file: &Path| {
        if opts.dry_run || opts.verbose {
            info!("pruning {}", display_path(file));
        } else {
            debug!("pruning {}", display_path(file));
        }
        if !opts.dry_run {
            file::remove_file_or_dir(file)?;
        }
        Ok::<(), color_eyre::Report>(())
    };
    for subdir in file::dir_subdirs(dir)? {
        let subdir = dir.join(&subdir);
        let r = prune(&subdir, opts)?;
        results.size += r.size;
        results.count += r.count;
        let metadata = subdir.metadata()?;
        // only delete empty directories if they're old
        if file::ls(&subdir)?.is_empty()
            && metadata.modified()?.elapsed().unwrap_or_default() > opts.age
        {
            remove(&subdir)?;
            results.count += 1;
        }
    }
    for f in file::ls(dir)? {
        let path = dir.join(&f);
        let metadata = path.metadata()?;
        let elapsed = metadata.accessed()?.elapsed().unwrap_or_default();
        if elapsed > opts.age {
            remove(&path)?;
            results.size += metadata.len();
            results.count += 1;
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use std::fs;

    use super::*;
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

    #[test]
    fn auto_prune_markers_are_scoped_per_cache_root() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::write(first.path().join("artifact"), "first").unwrap();
        fs::write(second.path().join("artifact"), "second").unwrap();
        let age = Duration::from_secs(60);

        assert!(prepare_auto_prune_root(first.path(), age).unwrap());
        assert!(prepare_auto_prune_root(second.path(), age).unwrap());
        assert!(!prepare_auto_prune_root(first.path(), age).unwrap());
        assert!(first.path().join(".auto_prune").exists());
        assert!(second.path().join(".auto_prune").exists());
    }
}
