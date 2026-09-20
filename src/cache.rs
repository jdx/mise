use std::cmp::min;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use eyre::{Result, WrapErr};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use path_absolutize::Absolutize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::LazyLock as Lazy;

use crate::build_time::built_info;
use crate::config::Settings;
use crate::file::{display_path, modified_duration};
use crate::hash::hash_to_str;
use crate::platform::Platform;
use crate::rand::random_string;
use crate::toolset::env_cache::CachedEnv;
use crate::{dirs, file};

pub(crate) use mise_cache_core::RemoteCacheMode as CacheRemoteMode;

pub(crate) fn effective_remote_cache_mode(configured: CacheRemoteMode) -> Option<CacheRemoteMode> {
    effective_remote_cache_mode_with(configured, |name| std::env::var(name).ok())
}

fn effective_remote_cache_mode_with(
    configured: CacheRemoteMode,
    get_env: impl Fn(&str) -> Option<String>,
) -> Option<CacheRemoteMode> {
    if trusted_cache_writer(&get_env) {
        return Some(configured);
    }
    match configured {
        CacheRemoteMode::ReadWrite | CacheRemoteMode::ReadOnly => Some(CacheRemoteMode::ReadOnly),
        CacheRemoteMode::WriteOnly => None,
    }
}

fn trusted_cache_writer(get_env: &impl Fn(&str) -> Option<String>) -> bool {
    if env_truthy(get_env("GITHUB_ACTIONS")) {
        return get_env("GITHUB_EVENT_NAME").as_deref() == Some("push")
            && get_env("GITHUB_REF_TYPE").as_deref() == Some("branch")
            && env_truthy(get_env("GITHUB_REF_PROTECTED"));
    }
    if env_truthy(get_env("GITLAB_CI")) {
        return get_env("CI_PIPELINE_SOURCE").as_deref() == Some("push")
            && get_env("CI_COMMIT_TAG").is_none()
            && get_env("CI_MERGE_REQUEST_IID").is_none()
            && env_truthy(get_env("CI_COMMIT_REF_PROTECTED"));
    }
    false
}

fn env_truthy(value: Option<String>) -> bool {
    value.is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

#[derive(Debug)]
pub(crate) struct CacheManagerBuilder {
    cache_file_path: PathBuf,
    cache_keys: Vec<String>,
    fresh_duration: Option<Duration>,
    fresh_files: Vec<PathBuf>,
}

pub(crate) static BASE_CACHE_KEYS: Lazy<Vec<String>> = Lazy::new(|| {
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
    pub(crate) fn new(cache_file_path: impl AsRef<Path>) -> Self {
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

    pub(crate) fn with_fresh_duration(mut self, duration: Option<Duration>) -> Self {
        self.fresh_duration = duration;
        self
    }

    pub(crate) fn with_fresh_file(mut self, path: PathBuf) -> Self {
        self.fresh_files.push(path);
        self
    }

    pub(crate) fn with_cache_key(mut self, key: String) -> Self {
        self.cache_keys.push(key);
        self
    }

    fn cache_key(&self) -> String {
        hash_to_str(&self.cache_keys).chars().take(5).collect()
    }

    pub(crate) fn build<T>(self) -> CacheManager<T>
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
pub(crate) struct CacheManager<T>
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
    pub(crate) fn get_or_try_init<F>(&self, fetch: F) -> Result<&T>
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

    pub(crate) async fn get_or_try_init_async<F, Fut>(&self, fetch: F) -> Result<&T>
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
    pub(crate) async fn get_or_try_init_async_if<F, Fut, P>(
        &self,
        fetch: F,
        should_cache: P,
    ) -> Result<T>
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
    pub(crate) async fn refresh_async<F, Fut>(&mut self, fetch: F) -> Result<T>
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
    pub(crate) fn get_cached(&self) -> Result<T>
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

    pub(crate) fn write(&self, val: &T) -> Result<()> {
        trace!("writing {}", display_path(&self.cache_file_path));
        if let Some(parent) = self.cache_file_path.parent() {
            file::create_dir_all(parent)?;
        }
        let partial_path = self
            .cache_file_path
            .with_extension(format!("part-{}", random_string(8)));
        let mut zlib = ZlibEncoder::new(File::create(&partial_path)?, Compression::fast());
        zlib.write_all(&rmp_serde::to_vec_named(&val)?[..])?;
        // Finish compression and close the file before publishing it to readers.
        drop(zlib.finish()?);
        file::rename(&partial_path, &self.cache_file_path)?;

        Ok(())
    }

    pub(crate) fn clear(&mut self) -> Result<()> {
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

/// A cache entry as it sits on disk, described by `symlink_metadata`.
///
/// Pruning is the one cache walk that deletes, so every decision it makes has to
/// come from the entry itself rather than from whatever the entry points at. A
/// cache root legitimately holds symlinks — the npm backend snapshots a built
/// package tree verbatim, links and their original absolute targets included —
/// and following one leads straight out of the cache and into a live install.
struct CacheEntry {
    path: PathBuf,
    metadata: std::fs::Metadata,
}

impl CacheEntry {
    /// A directory prune may descend into: a real one, never a link to one.
    fn is_dir(&self) -> bool {
        self.metadata.is_dir()
    }

    /// Whether the entry itself has gone untouched for `age`.
    ///
    /// The times come from the entry, so a symlink ages by its own record and
    /// not by the file it names.
    fn is_stale(&self, age: Duration) -> Result<bool> {
        Ok(self.metadata.accessed()?.elapsed().unwrap_or_default() > age)
    }
}

/// Lists `dir` without resolving any of its entries.
///
/// Entries that disappear mid-walk are dropped: another mise process pruning the
/// same root is not a reason to abandon this pass.
fn cache_entries(dir: &Path) -> Result<Vec<CacheEntry>> {
    let read_dir = match dir.read_dir() {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(err) => return Err(err).wrap_err_with(|| format!("failed to read {}", dir.display())),
    };
    let mut entries = vec![];
    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        // `DirEntry::metadata` does not follow links, unlike `Path::metadata`.
        match entry.metadata() {
            Ok(metadata) => entries.push(CacheEntry { path, metadata }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).wrap_err_with(|| format!("failed to stat {}", path.display()));
            }
        }
    }
    Ok(entries)
}

/// Deletes one classified entry, without resolving it.
///
/// A symlink goes through the repository's link remover, which unlinks a unix
/// symlink and deletes a Windows symlink or junction by handle after checking
/// its reparse tag — `remove_file` refuses the latter outright. Whatever the
/// link names is left alone either way.
fn remove_entry(entry: &CacheEntry) -> Result<()> {
    let result = if entry.is_dir() {
        file::remove_dir(&entry.path)
    } else if entry.metadata.is_symlink() {
        file::remove_symlink_or_junction(&entry.path)
    } else {
        file::remove_file(&entry.path)
    };
    match result {
        Ok(()) => Ok(()),
        // Another mise process pruning the same root may have taken the entry
        // between the listing and this call. Only an entry that is genuinely
        // gone is excused: a dangling link still stats here, and a stat that
        // fails for any other reason — an unreadable parent, say — leaves the
        // removal failure to be reported.
        Err(err) => match entry.path.symlink_metadata() {
            Err(stat) if stat.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(err),
        },
    }
}

pub(crate) fn prune(dir: &Path, opts: &PruneOptions) -> Result<PruneResults> {
    Ok(prune_dir(dir, false, opts)?.removed)
}

/// What one directory's pass did, and what it holds.
struct DirPrune {
    /// What this pass actually deleted below the directory.
    removed: PruneResults,
    /// Everything under the directory, deleted or not. A caller that evicts the
    /// whole directory counts this instead.
    held: PruneResults,
    /// Whether every entry under the directory is old enough to prune, which
    /// makes the directory evictable as a unit.
    all_stale: bool,
}

/// `descended` marks a directory prune classified itself, as opposed to a root
/// it was handed. A root is taken as given — `MISE_CACHE_DIR` may legitimately
/// be a symlink to the real cache — while a descent is confirmed below.
fn prune_dir(dir: &Path, descended: bool, opts: &PruneOptions) -> Result<DirPrune> {
    let mut removed = PruneResults { size: 0, count: 0 };
    let mut held = PruneResults { size: 0, count: 0 };
    let mut all_stale = true;
    let announce = |path: &Path| {
        if opts.dry_run || opts.verbose {
            info!("pruning {}", display_path(path));
        } else {
            debug!("pruning {}", display_path(path));
        }
    };
    let remove = |entry: &CacheEntry| {
        announce(&entry.path);
        if !opts.dry_run {
            remove_entry(entry)?;
        }
        Ok::<(), color_eyre::Report>(())
    };
    let entries = cache_entries(dir)?;
    // `read_dir` resolves the path it is handed, and the classification that led
    // here was made an instant earlier. If this path is no longer a directory in
    // its own right, that listing may describe somewhere else entirely, so
    // nothing under it is removed.
    if descended && !dir.symlink_metadata().is_ok_and(|m| m.file_type().is_dir()) {
        return Ok(DirPrune {
            removed,
            held,
            all_stale: false,
        });
    }
    // Each evictable entry carries what it holds, so a directory taken whole is
    // counted from the pass that classified it rather than walked twice.
    let mut evictable: Vec<(CacheEntry, PruneResults)> = vec![];
    for entry in entries {
        if !entry.is_dir() {
            held.size += entry.metadata.len();
            held.count += 1;
            if !entry.is_stale(opts.age)? {
                all_stale = false;
                continue;
            }
            // Held back: if everything here turns out to be stale, the caller
            // takes the directory whole rather than hollowing it out.
            let size = entry.metadata.len();
            evictable.push((entry, PruneResults { size, count: 1 }));
            continue;
        }
        let below = prune_dir(&entry.path, true, opts)?;
        removed.size += below.removed.size;
        removed.count += below.removed.count;
        held.size += below.held.size;
        held.count += below.held.count;
        if below.all_stale {
            evictable.push((entry, below.held));
        } else {
            all_stale = false;
        }
    }
    // An empty directory holds nothing to age, so it goes by its own timestamps.
    if evictable.is_empty()
        && all_stale
        && let Ok(metadata) = dir.symlink_metadata()
    {
        all_stale = metadata.modified()?.elapsed().unwrap_or_default() > opts.age
            || metadata.accessed()?.elapsed().unwrap_or_default() > opts.age;
    }
    if all_stale && descended {
        // Everything under this directory is prunable, so the caller evicts it
        // in one piece. Removing the files one by one is what leaves a cache
        // entry standing but hollow — a directory tree with nothing in it —
        // which whatever wrote that entry may then read back as intact.
        return Ok(DirPrune {
            removed,
            held,
            all_stale: true,
        });
    }
    for (entry, holds) in evictable {
        if entry.is_dir() {
            // Classified once already, and classified again here: a cache
            // writer may have published into the subtree since, and what is
            // about to be deleted is the whole of it rather than the entries
            // that were read. A subtree that now holds something fresh prunes
            // by file on this pass and is evicted by a later one.
            let recheck = prune_dir(&entry.path, true, opts)?;
            if !recheck.all_stale {
                removed.size += recheck.removed.size;
                removed.count += recheck.removed.count;
                all_stale = false;
                continue;
            }
            announce(&entry.path);
            if !opts.dry_run {
                // `remove_dir_all` unlinks a symlink rather than following it,
                // and on unix walks by descriptor, so the tree it deletes is
                // the tree it opened.
                match std::fs::remove_dir_all(&entry.path) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(err).wrap_err_with(|| {
                            format!("failed rm -rf: {}", display_path(&entry.path))
                        });
                    }
                }
            }
            removed.size += recheck.held.size;
            removed.count += recheck.held.count + 1;
        } else {
            remove(&entry)?;
            removed.size += holds.size;
            removed.count += holds.count;
        }
    }
    Ok(DirPrune {
        removed,
        held,
        all_stale,
    })
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use std::collections::BTreeMap;
    use std::fs;

    use super::*;
    use pretty_assertions::assert_eq;

    fn environment(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let values = values
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>();
        move |key| values.get(key).cloned()
    }

    #[test]
    fn remote_writes_require_a_protected_branch_ci_context() {
        assert_eq!(
            effective_remote_cache_mode_with(CacheRemoteMode::ReadWrite, environment(&[])),
            Some(CacheRemoteMode::ReadOnly)
        );
        assert_eq!(
            effective_remote_cache_mode_with(CacheRemoteMode::WriteOnly, environment(&[])),
            None
        );
        assert_eq!(
            effective_remote_cache_mode_with(
                CacheRemoteMode::ReadWrite,
                environment(&[
                    ("GITHUB_ACTIONS", "true"),
                    ("GITHUB_EVENT_NAME", "pull_request"),
                    ("GITHUB_REF_PROTECTED", "true"),
                    ("GITHUB_REF_TYPE", "branch"),
                ]),
            ),
            Some(CacheRemoteMode::ReadOnly)
        );
        assert_eq!(
            effective_remote_cache_mode_with(
                CacheRemoteMode::ReadWrite,
                environment(&[
                    ("GITHUB_ACTIONS", "true"),
                    ("GITHUB_EVENT_NAME", "push"),
                    ("GITHUB_REF_PROTECTED", "true"),
                    ("GITHUB_REF_TYPE", "branch"),
                ]),
            ),
            Some(CacheRemoteMode::ReadWrite)
        );
    }

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

    #[cfg(unix)]
    fn stale_prune_options() -> PruneOptions {
        PruneOptions {
            dry_run: false,
            verbose: false,
            age: Duration::from_secs(1),
        }
    }

    /// Backdates a path's own timestamps, leaving anything it links to alone.
    #[cfg(unix)]
    fn backdate(path: &Path) {
        let stale = filetime::FileTime::from_unix_time(0, 0);
        filetime::set_symlink_file_times(path, stale, stale).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn prune_does_not_reach_through_a_symlink_out_of_the_cache() {
        let cache = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let package = install.path().join("packages").join("inner");
        fs::create_dir_all(&package).unwrap();
        let file = package.join("index.js");
        fs::write(&file, "module.exports = {}").unwrap();
        backdate(&file);
        backdate(&package);

        let entry = cache.path().join("side-effects").join("node_modules");
        fs::create_dir_all(&entry).unwrap();
        let link = entry.join("inner");
        std::os::unix::fs::symlink(&package, &link).unwrap();

        prune(cache.path(), &stale_prune_options()).unwrap();

        assert!(file.exists(), "prune deleted a file outside the cache");
        assert!(package.exists());
    }

    /// Stands in for a directory swapped for a symlink after prune classified it:
    /// the descent finds a link where it recorded a directory.
    #[cfg(unix)]
    #[test]
    fn a_descent_that_lands_on_a_link_removes_nothing() {
        let cache = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let file = install.path().join("index.js");
        fs::write(&file, "module.exports = {}").unwrap();
        backdate(&file);

        let swapped = cache.path().join("node_modules");
        std::os::unix::fs::symlink(install.path(), &swapped).unwrap();

        prune_dir(&swapped, true, &stale_prune_options()).unwrap();

        assert!(file.exists(), "a replaced directory was walked anyway");
    }

    /// A structured cache entry — a package manager's build output, say — is
    /// only usable whole. Taking its files one by one leaves the directory tree
    /// standing and empty, which whatever wrote it may read back as intact.
    #[test]
    fn a_wholly_stale_directory_goes_as_one() {
        let cache = tempfile::tempdir().unwrap();
        let entry = cache.path().join("side-effects/pkg@1.0.0/linux/abc");
        fs::create_dir_all(entry.join("build")).unwrap();
        for path in ["build/built.node", "package.json"] {
            let path = entry.join(path);
            fs::write(&path, "x").unwrap();
            backdate(&path);
        }

        prune(cache.path(), &stale_prune_options()).unwrap();

        assert!(
            !cache.path().join("side-effects").exists(),
            "a hollow directory tree was left behind"
        );
    }

    /// The mechanism behind it: a wholly stale directory hands itself to its
    /// caller untouched instead of deleting its own contents. A tree emptied
    /// file by file is readable as an intact entry in the meantime, by another
    /// process or by whatever runs after a prune that stops halfway.
    #[test]
    fn a_wholly_stale_directory_defers_to_its_caller() {
        let cache = tempfile::tempdir().unwrap();
        let entry = cache.path().join("pkg@1.0.0");
        fs::create_dir_all(entry.join("build")).unwrap();
        let built = entry.join("build/built.node");
        fs::write(&built, "x").unwrap();
        backdate(&built);

        let below = prune_dir(&entry, true, &stale_prune_options()).unwrap();

        assert!(below.all_stale, "the entry was not seen as evictable");
        assert_eq!(below.removed.count, 0, "the entry hollowed itself out");
        assert_eq!(below.held.count, 1);
        assert!(built.exists());
    }

    /// Where ages are mixed, the stale files still go individually: a content
    /// addressed store keeps hot and cold entries side by side in one
    /// directory, and it has to stay reclaimable.
    #[test]
    fn a_directory_in_use_keeps_its_shape_and_loses_its_stale_files() {
        let cache = tempfile::tempdir().unwrap();
        let shard = cache.path().join("store/files/ab");
        fs::create_dir_all(&shard).unwrap();
        let cold = shard.join("cold-blob");
        fs::write(&cold, "cold").unwrap();
        backdate(&cold);
        let hot = shard.join("hot-blob");
        fs::write(&hot, "hot").unwrap();

        let results = prune(cache.path(), &stale_prune_options()).unwrap();

        assert!(!cold.exists(), "a stale blob survived beside a fresh one");
        assert!(hot.exists());
        assert!(shard.exists(), "a directory still in use was removed");
        assert_eq!(results.count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn prune_removes_a_stale_link_without_touching_its_target() {
        let cache = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let target = install.path().join("packages");
        fs::create_dir_all(&target).unwrap();

        let entry = cache.path().join("side-effects");
        fs::create_dir_all(&entry).unwrap();
        let link = entry.join("inner");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        backdate(&link);

        prune(cache.path(), &stale_prune_options()).unwrap();

        assert!(link.symlink_metadata().is_err(), "stale link was kept");
        assert!(target.exists(), "prune removed the link's target");
    }

    #[cfg(unix)]
    #[test]
    fn prune_finishes_when_a_link_dangles() {
        let cache = tempfile::tempdir().unwrap();
        let entry = cache.path().join("side-effects");
        fs::create_dir_all(&entry).unwrap();
        std::os::unix::fs::symlink(cache.path().join("gone"), entry.join("inner")).unwrap();
        let stale = entry.join("meta.json");
        fs::write(&stale, "{}").unwrap();
        backdate(&stale);

        prune(cache.path(), &stale_prune_options()).unwrap();

        assert!(!stale.exists(), "a dangling link stopped the prune pass");
    }
}
