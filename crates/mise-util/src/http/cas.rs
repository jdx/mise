//! A content-addressed store of finished downloads.
//!
//! Files are kept by their BLAKE3 hash under `blobs/`. Two small indexes point
//! into them:
//!
//! - `urls/`: a request (URL and headers) to the blob it last returned and the
//!   validator (`ETag` or `Last-Modified`) the server sent with it, so a later
//!   request can ask the server whether it changed and take `304 Not Modified`
//!   as an answer.
//! - `pins/`: a checksum a tool pinned to a blob, so a download can be reused
//!   with no request at all.
//!
//! Nothing in an index is trusted on its own. A blob is hashed again before
//! it is used, against the pin when there is one, and an entry that no longer
//! matches is dropped.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::DownloadValidator;
use crate::file;

/// Blobs are evicted, least recently used first, past this total size.
const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Algorithms a pin may use. `md5` and `sha1` are not collision resistant
/// enough to name content by.
const PIN_ALGOS: &[&str] = &["sha256", "sha384", "sha512", "blake3"];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UrlEntry {
    validator: DownloadValidator,
    blake3: String,
    effective_filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PinEntry {
    blake3: String,
    effective_filename: Option<String>,
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
        Some(crate::dirs::CACHE.join("downloads-cas"))
    }
}

fn is_hex(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn blob_path(root: &Path, blake3: &str) -> PathBuf {
    root.join("blobs").join(blake3)
}

fn url_path(root: &Path, request_hash: &str) -> PathBuf {
    root.join("urls").join(format!("{request_hash}.json"))
}

/// Split a pin into its algorithm and lowercase digest, or `None` when it
/// could not safely name a file.
fn parse_pin(pin: &str) -> Option<(&str, String)> {
    let (algo, hex) = pin.split_once(':')?;
    (PIN_ALGOS.contains(&algo) && is_hex(hex)).then(|| (algo, hex.to_lowercase()))
}

fn pin_path(root: &Path, algo: &str, hex: &str) -> PathBuf {
    root.join("pins").join(algo).join(format!("{hex}.json"))
}

/// A stored download ready to be copied out.
pub(super) struct Hit {
    blob: PathBuf,
    pub(super) effective_filename: Option<String>,
    /// The index entry that led here, dropped if the blob doesn't verify.
    entry: PathBuf,
    verify: Verify,
}

enum Verify {
    /// The blob must hash to its own name.
    Blake3(String),
    /// The blob must hash to the tool's pinned checksum.
    Pin { algo: String, hex: String },
}

/// What the server was last asked about, for a conditional request.
pub(super) struct Cached {
    pub(super) validator: DownloadValidator,
    hit: Hit,
}

impl Cached {
    pub(super) fn into_hit(self) -> Hit {
        self.hit
    }

    pub(super) fn effective_filename(&self) -> Option<String> {
        self.hit.effective_filename.clone()
    }
}

pub(super) fn lookup_url(request_hash: &str) -> Option<Cached> {
    let root = root()?;
    let entry = url_path(&root, request_hash);
    let url: UrlEntry = serde_json::from_slice(&std::fs::read(&entry).ok()?).ok()?;
    if !url.validator.is_valid() || !is_hex(&url.blake3) {
        return None;
    }
    let blob = blob_path(&root, &url.blake3);
    if !blob.is_file() {
        return None;
    }
    Some(Cached {
        validator: url.validator,
        hit: Hit {
            blob,
            effective_filename: url.effective_filename,
            entry,
            verify: Verify::Blake3(url.blake3),
        },
    })
}

pub(super) fn lookup_pin(pin: &str) -> Option<Hit> {
    let root = root()?;
    let (algo, hex) = parse_pin(pin)?;
    let entry = pin_path(&root, algo, &hex);
    let pinned: PinEntry = serde_json::from_slice(&std::fs::read(&entry).ok()?).ok()?;
    if !is_hex(&pinned.blake3) {
        return None;
    }
    let blob = blob_path(&root, &pinned.blake3);
    if !blob.is_file() {
        return None;
    }
    Some(Hit {
        blob,
        effective_filename: pinned.effective_filename,
        entry,
        verify: Verify::Pin {
            algo: algo.to_string(),
            hex,
        },
    })
}

impl Hit {
    /// Copy the blob to `dest` if it still hashes to what it is expected to.
    /// Otherwise drop what pointed at it and return `false`.
    pub(super) fn restore(&self, dest: &Path) -> Result<bool> {
        let intact = match &self.verify {
            Verify::Blake3(expected) => {
                crate::hash::file_hash_blake3(&self.blob, None).is_ok_and(|hash| hash == *expected)
            }
            Verify::Pin { algo, hex } => {
                crate::hash::ensure_checksum(&self.blob, hex, None, algo).is_ok()
            }
        };
        if !intact {
            let _ = file::remove_file(&self.entry);
            if matches!(self.verify, Verify::Blake3(_)) {
                let _ = file::remove_file(&self.blob);
            }
            return Ok(false);
        }
        let parent = dest.parent().unwrap_or(Path::new("."));
        file::create_dir_all(parent)?;
        // Renamed into place so an interrupted copy never leaves a truncated
        // file at the destination. A `TempPath` holds no open handle, which
        // Windows needs to copy over it.
        let tmp = tempfile::NamedTempFile::new_in(parent)?.into_temp_path();
        file::copy(&self.blob, &tmp)?;
        tmp.persist(dest).map_err(|err| err.error)?;
        // Marks the blob as recently used for eviction.
        if let Ok(f) = std::fs::File::options().write(true).open(&self.blob) {
            let _ = f.set_modified(SystemTime::now());
        }
        Ok(true)
    }
}

/// Write `contents` to `path` so a reader never sees a partial file.
fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().expect("index paths have a parent");
    file::create_dir_all(parent)?;
    let tmp = tempfile::NamedTempFile::new_in(parent)?.into_temp_path();
    file::write(&tmp, contents)?;
    tmp.persist(path).map_err(|err| err.error)?;
    Ok(())
}

/// Keep `path`, just downloaded. `validator` lets the server confirm it later;
/// `pin` lets a tool that pinned the same checksum skip the request.
pub(super) fn store(
    request_hash: &str,
    pin: Option<&str>,
    path: &Path,
    validator: Option<&DownloadValidator>,
    effective_filename: Option<String>,
) -> Result<()> {
    let Some(root) = root() else {
        return Ok(());
    };
    // The caller checks the pin after the download. Checking here too keeps a
    // wrong file from ever being indexed under it.
    let pin = pin.and_then(parse_pin).filter(|(algo, hex)| {
        let ok = crate::hash::ensure_checksum(path, hex, None, algo).is_ok();
        if !ok {
            debug!("not indexing a download that doesn't match its pin");
        }
        ok
    });
    if validator.is_none() && pin.is_none() {
        return Ok(());
    }
    let blake3 = crate::hash::file_hash_blake3(path, None)?;
    let blob = blob_path(&root, &blake3);
    if !blob.is_file() {
        let parent = blob.parent().expect("blob paths have a parent");
        file::create_dir_all(parent)?;
        let tmp = tempfile::NamedTempFile::new_in(parent)?.into_temp_path();
        file::copy(path, &tmp)?;
        tmp.persist(&blob).map_err(|err| err.error)?;
    }
    if let Some(validator) = validator {
        let entry = UrlEntry {
            validator: validator.clone(),
            blake3: blake3.clone(),
            effective_filename: effective_filename.clone(),
        };
        write_atomic(&url_path(&root, request_hash), &serde_json::to_vec(&entry)?)?;
    }
    if let Some((algo, hex)) = pin {
        let entry = PinEntry {
            blake3,
            effective_filename,
        };
        write_atomic(&pin_path(&root, algo, &hex), &serde_json::to_vec(&entry)?)?;
    }
    evict(&root.join("blobs"));
    Ok(())
}

fn evict(blobs: &Path) {
    let Ok(read) = std::fs::read_dir(blobs) else {
        return;
    };
    let mut entries = Vec::new();
    let mut total = 0u64;
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        total += meta.len();
        entries.push((
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
            entry.path(),
        ));
    }
    entries.sort();
    for (_, size, path) in entries {
        if total <= MAX_CACHE_BYTES {
            break;
        }
        if file::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}
