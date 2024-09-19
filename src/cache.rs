use std::cmp::min;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use eyre::Result;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use once_cell::sync::{Lazy, OnceCell};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::build_time::built_info;
use crate::config::Settings;
use crate::file::{display_path, modified_duration};
use crate::hash::hash_to_str;
use crate::rand::random_string;
use crate::{dirs, file};

#[derive(Debug, Clone)]
pub struct CacheManager<T>
where
    T: Serialize + DeserializeOwned,
{
    cache_file_path: PathBuf,
    fresh_duration: Option<Duration>,
    fresh_files: Vec<PathBuf>,
    cache: Box<OnceCell<T>>,
    no_cache: bool,
}

impl<T> CacheManager<T>
where
    T: Serialize + DeserializeOwned,
{
    pub fn new(cache_file_path: impl AsRef<Path>) -> Self {
        // "replace $KEY in path with key()
        let cache_file_path = regex!(r#"\$KEY"#)
            .replace_all(cache_file_path.as_ref().to_str().unwrap(), &*KEY)
            .to_string()
            .into();
        Self {
            cache_file_path,
            cache: Box::new(OnceCell::new()),
            fresh_files: Vec::new(),
            fresh_duration: None,
            no_cache: false,
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

    pub fn get_or_try_init<F>(&self, fetch: F) -> Result<&T>
    where
        F: FnOnce() -> Result<T>,
    {
        let val = self.cache.get_or_try_init(|| {
            let path = &self.cache_file_path;
            if !self.no_cache && self.is_fresh() {
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
        for path in &self.fresh_files {
            let duration = modified_duration(path).unwrap_or_default();
            freshest = Some(match freshest {
                None => duration,
                Some(freshest) => min(freshest, duration),
            })
        }
        freshest
    }
}

static KEY: Lazy<String> = Lazy::new(|| {
    let mut parts = vec![
        built_info::FEATURES_STR,
        //built_info::PKG_VERSION, # TODO: put this in for non-debug when we autoclean cache (#2139)
        built_info::PROFILE,
        built_info::TARGET,
    ];
    if cfg!(debug_assertions) {
        parts.push(built_info::PKG_VERSION);
    }
    hash_to_str(&parts).chars().take(5).collect()
});

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
    if rand::random::<u8>() % 10 != 0 {
        return Ok(()); // only prune 10% of the time
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
    debug!("pruning old cache files, this behavior can be modified with the MISE_CACHE_PRUNE_AGE setting");
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
    use super::*;
    use crate::test::reset;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_cache() {
        reset();
        // does not fail with invalid path
        let cache = CacheManager::new("/invalid:path/to/cache");
        cache.clear().unwrap();
        let val = cache.get_or_try_init(|| Ok(1)).unwrap();
        assert_eq!(val, &1);
        let val = cache.get_or_try_init(|| Ok(2)).unwrap();
        assert_eq!(val, &1);
    }
}
