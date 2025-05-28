use std::cmp::min;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use eyre::Result;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::LazyLock as Lazy;

use crate::build_time::built_info;
use crate::config::Settings;
use crate::file::{display_path, modified_duration};
use crate::hash::hash_to_str;
use crate::rand::random_string;
use crate::{dirs, file};

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
        Self {
            cache_file_path: cache_file_path.as_ref().to_path_buf(),
            cache_keys: BASE_CACHE_KEYS.clone(),
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

    #[cfg(test)]
    pub fn clear(&self) -> Result<()> {
        let path = &self.cache_file_path;
        trace!("clearing cache {}", path.display());
        if path.exists() {
            file::remove_file(path)?;
        }
        Ok(())
    }

    fn is_fresh(&self) -> bool {
        if !self.cache_file_path.exists() {
            return false;
        }
        if let Some(fresh_duration) = self.freshest_duration() {
            if let Ok(metadata) = self.cache_file_path.metadata() {
                if let Ok(modified) = metadata.modified() {
                    return modified.elapsed().unwrap_or_default() < fresh_duration;
                }
            }
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

pub(crate) fn auto_prune() -> Result<()> {
    if rand::random::<u8>() % 100 != 0 {
        return Ok(()); // only prune 1% of the time
    }
    let settings = Settings::get();
    let age = match settings.cache_prune_age_duration() {
        Some(age) => age,
        None => {
            return Ok(());
        }
    };
    let auto_prune_file = dirs::CACHE.join(".auto_prune");
    if let Ok(Ok(modified)) = auto_prune_file.metadata().map(|m| m.modified()) {
        if modified.elapsed().unwrap_or_default() < age {
            return Ok(());
        }
    }
    let empty = file::ls(*dirs::CACHE).unwrap_or_default().is_empty();
    xx::file::touch_dir(&auto_prune_file)?;
    if empty {
        return Ok(());
    }
    debug!(
        "pruning old cache files, this behavior can be modified with the MISE_CACHE_PRUNE_AGE setting"
    );
    prune(
        *dirs::CACHE,
        &PruneOptions {
            dry_run: false,
            verbose: false,
            age,
        },
    )?;
    Ok(())
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

    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn test_cache() {
        let _config = Config::get().await.unwrap();
        let cache = CacheManagerBuilder::new(dirs::CACHE.join("test-cache")).build();
        cache.clear().unwrap();
        let val = cache.get_or_try_init(|| Ok(1)).unwrap();
        assert_eq!(val, &1);
        let val = cache.get_or_try_init(|| Ok(2)).unwrap();
        assert_eq!(val, &1);
    }
}
