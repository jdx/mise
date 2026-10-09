//! Reuse of a finished download when the server says it hasn't changed.
//!
//! Each entry holds the file, the validator (`ETag` or `Last-Modified`) the
//! server sent with it, and the file's BLAKE3 hash. The next request for the
//! same URL and headers carries that validator; a `304 Not Modified` answers
//! with no body, and the stored file is used after it hashes to the recorded
//! value again.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::DownloadValidator;
use crate::file;

/// Entries are evicted, oldest use first, past this total size.
const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

const FILE: &str = "file";
const META: &str = "meta.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    validator: DownloadValidator,
    blake3: String,
    effective_filename: Option<String>,
}

pub(super) struct Cached {
    dir: PathBuf,
    meta: Meta,
}

impl Cached {
    pub(super) fn validator(&self) -> &DownloadValidator {
        &self.meta.validator
    }

    pub(super) fn effective_filename(&self) -> Option<String> {
        self.meta.effective_filename.clone()
    }

    /// Copy the stored file to `dest` if it still hashes to what was recorded.
    /// An entry that doesn't is removed.
    pub(super) fn restore(&self, dest: &Path) -> Result<bool> {
        let stored = self.dir.join(FILE);
        let intact = stored.is_file()
            && crate::hash::file_hash_blake3(&stored, None)
                .is_ok_and(|hash| hash == self.meta.blake3);
        if !intact {
            let _ = file::remove_all(&self.dir);
            return Ok(false);
        }
        let parent = dest.parent().unwrap_or(Path::new("."));
        file::create_dir_all(parent)?;
        // Renamed into place so an interrupted copy never leaves a truncated
        // file at the destination.
        let tmp = tempfile::NamedTempFile::new_in(parent)?.into_temp_path();
        file::copy(&stored, &tmp)?;
        tmp.persist(dest).map_err(|err| err.error)?;
        // Marks the entry as recently used for eviction.
        if let Ok(f) = std::fs::File::options()
            .write(true)
            .open(self.dir.join(META))
        {
            let _ = f.set_modified(SystemTime::now());
        }
        Ok(true)
    }
}

/// Tests that don't ask for a directory never touch the user's cache.
#[cfg(test)]
pub(super) static TEST_ROOT: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

fn root() -> Option<PathBuf> {
    #[cfg(test)]
    {
        TEST_ROOT.lock().unwrap().clone()
    }
    #[cfg(not(test))]
    {
        Some(crate::dirs::CACHE.join("downloads-cache"))
    }
}

pub(super) fn lookup(request_hash: &str) -> Option<Cached> {
    let dir = root()?.join(request_hash);
    let meta: Meta = serde_json::from_slice(&std::fs::read(dir.join(META)).ok()?).ok()?;
    meta.validator.is_valid().then_some(Cached { dir, meta })
}

/// Keep `path`, just downloaded with `validator`.
pub(super) fn store(
    request_hash: &str,
    path: &Path,
    validator: &DownloadValidator,
    effective_filename: Option<String>,
) -> Result<()> {
    let Some(root) = root() else {
        return Ok(());
    };
    let blake3 = crate::hash::file_hash_blake3(path, None)?;
    let dir = root.join(request_hash);
    let parent = dir.parent().expect("entry_dir joins a name");
    file::create_dir_all(parent)?;
    // Built beside the entry under a name nothing else shares, then swapped in.
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(parent)?;
    file::copy(path, staging.path().join(FILE))?;
    let meta = Meta {
        validator: validator.clone(),
        blake3,
        effective_filename,
    };
    file::write(staging.path().join(META), serde_json::to_vec(&meta)?)?;
    let _ = file::remove_all(&dir);
    let staged = staging.keep();
    if let Err(err) = std::fs::rename(&staged, &dir) {
        let _ = file::remove_all(&staged);
        // Another process stored the same download first.
        if !dir.join(META).is_file() {
            return Err(err.into());
        }
    }
    evict(parent);
    Ok(())
}

fn evict(root: &Path) {
    let Ok(read) = std::fs::read_dir(root) else {
        return;
    };
    let mut entries = Vec::new();
    let mut total = 0u64;
    for entry in read.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let size = std::fs::metadata(path.join(FILE)).map_or(0, |m| m.len());
        let used = std::fs::metadata(path.join(META))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        total += size;
        entries.push((used, size, path));
    }
    entries.sort();
    for (_, size, path) in entries {
        if total <= MAX_CACHE_BYTES {
            break;
        }
        if file::remove_all(&path).is_ok() {
            total -= size;
        }
    }
}
