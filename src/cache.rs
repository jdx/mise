use std::cmp::min;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::file::modified_duration;
use color_eyre::eyre::Result;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use once_cell::sync::OnceCell;
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct CacheManager<T>
where
    T: Clone + Serialize + DeserializeOwned,
{
    cache_file_path: PathBuf,
    fresh_duration: Option<Duration>,
    fresh_files: Vec<PathBuf>,
    cache: Box<OnceCell<T>>,
}

impl<T> CacheManager<T>
where
    T: Clone + Serialize + DeserializeOwned,
{
    pub fn new(cache_file_path: PathBuf) -> Self {
        Self {
            cache_file_path,
            cache: Box::new(OnceCell::new()),
            fresh_files: Vec::new(),
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
                        warn!("failed to parse cache file: {} {}", path.display(), err);
                    }
                }
            }
            let val = (fetch)()?;
            if let Err(err) = self.write(val.clone()) {
                warn!("failed to write cache file: {} {}", path.display(), err);
            }
            Ok(val)
        })?;
        Ok(val)
    }

    fn parse(&self) -> Result<T> {
        let path = &self.cache_file_path;
        trace!("reading cache {}", path.display());
        let mut zlib = ZlibDecoder::new(File::open(path)?);
        let mut bytes = Vec::new();
        zlib.read_to_end(&mut bytes)?;
        Ok(rmp_serde::from_slice(&bytes)?)
    }

    pub fn write(&self, val: T) -> Result<()> {
        let path = &self.cache_file_path;
        trace!("writing cache {}", path.display());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut zlib = ZlibEncoder::new(File::create(path)?, Compression::fast());
        zlib.write_all(&rmp_serde::to_vec_named(&val)?[..])?;

        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let path = &self.cache_file_path;
        trace!("clearing cache {}", path.display());
        if path.exists() {
            fs::remove_file(path)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache() {
        // does not fail with invalid path
        let cache = CacheManager::new("/invalid:path/to/cache".into());
        cache.clear().unwrap();
        let val = cache.get_or_try_init(|| Ok(1)).unwrap();
        assert_eq!(val, &1);
        let val = cache.get_or_try_init(|| Ok(2)).unwrap();
        assert_eq!(val, &1);
    }
}
