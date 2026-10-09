//! A content-addressed store of finished downloads.
//!
//! Files are kept by their BLAKE3 hash under `blobs/<hash>/<name>`, under the
//! name they were downloaded as, since installers read the format and the
//! binary's name from it. Two small indexes point
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

/// The size `download_cache_max_size` falls back to when it can't be read.
const DEFAULT_MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Blobs are evicted, least recently used first, past this total size.
/// `download_cache_max_size = "0"` is no limit.
pub(super) fn max_cache_bytes() -> u64 {
    let setting = mise_settings::Settings::get();
    match setting
        .download_cache_max_size
        .parse::<bytesize::ByteSize>()
    {
        Ok(size) if size.as_u64() == 0 => u64::MAX,
        Ok(size) => size.as_u64(),
        Err(err) => {
            debug!(
                "invalid download_cache_max_size {:?}: {err}",
                setting.download_cache_max_size
            );
            DEFAULT_MAX_CACHE_BYTES
        }
    }
}

/// How long after a blob was last used another install may still be placing it.
const IN_USE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

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

fn blob_dir(root: &Path, blake3: &str) -> PathBuf {
    root.join("blobs").join(blake3)
}

/// The file a blob directory holds, whatever name it was stored under.
fn blob_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_file())
}

/// Where an in-flight download is written: stable per request, so a partial
/// one can be resumed, and on the store's filesystem so it renames into place.
pub(super) fn staging_path(request_hash: &str, name: &str) -> Option<PathBuf> {
    Some(root()?.join("incoming").join(request_hash).join(name))
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
    let blob = blob_file(&blob_dir(&root, &url.blake3))?;
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
    let blob = blob_file(&blob_dir(&root, &pinned.blake3))?;
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
    /// The stored file, once it hashes to what it is expected to. Otherwise
    /// drop what pointed at it and return `None`.
    pub(super) fn verified(&self) -> Option<PathBuf> {
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
            // A blob that doesn't hash to its own name is damaged, whichever
            // index led here. One that does merely isn't what the pin names.
            if let Some(dir) = self.blob.parent() {
                let name = dir.file_name().map(|n| n.to_string_lossy().to_string());
                let damaged = crate::hash::file_hash_blake3(&self.blob, None)
                    .is_ok_and(|hash| Some(hash) != name);
                if damaged {
                    let _ = file::remove_all(dir);
                }
            }
            return None;
        }
        // Marks the blob as recently used for eviction.
        if let Ok(f) = std::fs::File::options().write(true).open(&self.blob) {
            let _ = f.set_modified(SystemTime::now());
        }
        Some(self.blob.clone())
    }
}

/// Put `blob` at `dest` without ever leaving a partial file there. With `link`
/// the content is shared where the filesystem allows and copied where it
/// doesn't, such as across devices; without it the content is always copied.
pub(super) fn place(blob: &Path, dest: &Path, link: bool) -> Result<()> {
    let parent = dest.parent().unwrap_or(Path::new("."));
    file::create_dir_all(parent)?;
    // A directory of our own, so the name inside it is free to link to. A
    // `TempPath` would exist already, and Windows can't copy over a file that
    // is open.
    let scratch = tempfile::Builder::new()
        .prefix(".mise-place-")
        .tempdir_in(parent)
        .map_err(|err| placing(err, dest))?;
    let staged = scratch.path().join("file");
    if link {
        file::hard_link_or_copy(blob, &staged)?;
    } else {
        file::copy(blob, &staged)?;
    }
    // `persist` replaces an existing file on Windows too, where a plain rename
    // does not.
    tempfile::TempPath::try_from_path(&staged)
        .map_err(|err| placing(err, dest))?
        .persist(dest)
        .map_err(|err| placing(err.error, dest))?;
    Ok(())
}

/// Name the step and the path, with the Windows hint for an over-long path:
/// `tempfile` doesn't get the extended-length handling `std::fs` does, so this
/// is the call that fails first as a directory approaches `MAX_PATH`.
fn placing(err: std::io::Error, dest: &Path) -> eyre::Report {
    let msg = file::with_io_hint(
        format!(
            "failed to move the downloaded file into place: {}",
            file::display_path(dest)
        ),
        dest,
        &err,
    );
    eyre::Report::new(err).wrap_err(msg)
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

/// What kept a download: how to find it again.
pub(super) struct Keep<'a> {
    pub(super) request_hash: &'a str,
    /// The checksum the tool pinned.
    pub(super) pin: Option<&'a str>,
    pub(super) validator: Option<&'a DownloadValidator>,
    pub(super) effective_filename: Option<String>,
}

/// Keep `path`, just downloaded, by copying it into the store. Returns the
/// stored file, or `None` when there is nothing to index it by.
pub(super) fn store(path: &Path, keep: Keep<'_>) -> Result<Option<PathBuf>> {
    keep_file(path, keep, false)
}

/// Keep `path`, which sits in the store's staging area, by moving it into the
/// store. Returns the stored file.
pub(super) fn ingest(path: &Path, keep: Keep<'_>) -> Result<Option<PathBuf>> {
    keep_file(path, keep, true)
}

fn keep_file(path: &Path, keep: Keep<'_>, move_in: bool) -> Result<Option<PathBuf>> {
    let Some(root) = root() else {
        return Ok(None);
    };
    // The caller checks the pin after the download. Checking here too keeps a
    // wrong file from ever being indexed under it.
    let pin = keep.pin.and_then(parse_pin).filter(|(algo, hex)| {
        let ok = crate::hash::ensure_checksum(path, hex, None, algo).is_ok();
        if !ok {
            debug!("not indexing a download that doesn't match its pin");
        }
        ok
    });
    if keep.validator.is_none() && pin.is_none() {
        return Ok(None);
    }
    let name = path.file_name().expect("downloads have a file name");
    let blake3 = crate::hash::file_hash_blake3(path, None)?;
    let dir = blob_dir(&root, &blake3);
    // An existing blob is reused only while it still hashes to its own name;
    // otherwise it is replaced by the file just downloaded.
    let existing = blob_file(&dir).filter(|existing| {
        let intact = crate::hash::file_hash_blake3(existing, None).is_ok_and(|h| h == blake3);
        if !intact {
            let _ = file::remove_all(&dir);
        }
        intact
    });
    let blob = match existing {
        Some(blob) => {
            if move_in {
                let _ = file::remove_file(path);
            }
            blob
        }
        None => {
            let blob = dir.join(name);
            // Built beside its final place so the rename can't cross a device.
            let blobs = root.join("blobs");
            file::create_dir_all(&blobs)?;
            let building = tempfile::Builder::new()
                .prefix(".building-")
                .tempdir_in(&blobs)?;
            let staged = building.path().join(name);
            if move_in {
                file::rename(path, &staged)?;
            } else {
                file::copy(path, &staged)?;
            }
            let building = building.keep();
            if let Err(err) = std::fs::rename(&building, &dir) {
                let _ = file::remove_all(&building);
                // Another process stored the same download first.
                if blob_file(&dir).is_none() {
                    return Err(err.into());
                }
            }
            blob_file(&dir).unwrap_or(blob)
        }
    };
    if let Some(validator) = keep.validator {
        let entry = UrlEntry {
            validator: validator.clone(),
            blake3: blake3.clone(),
            effective_filename: keep.effective_filename.clone(),
        };
        write_atomic(
            &url_path(&root, keep.request_hash),
            &serde_json::to_vec(&entry)?,
        )?;
    }
    if let Some((algo, hex)) = pin {
        let entry = PinEntry {
            blake3,
            effective_filename: keep.effective_filename,
        };
        write_atomic(&pin_path(&root, algo, &hex), &serde_json::to_vec(&entry)?)?;
    }
    evict(&root, blob.parent());
    Ok(Some(blob))
}

/// Past `download_cache_max_size` the least recently used blobs go first, except
/// `keep`, which the caller is about to use, and any used within `IN_USE`: a
/// blob is verified, which refreshes its time, moments before another install
/// places it, so a recent one may be mid-use. Staged downloads that were never
/// finished go after a week.
fn evict(root: &Path, keep: Option<&Path>) {
    let week = std::time::Duration::from_secs(7 * 24 * 60 * 60);
    if let Ok(read) = std::fs::read_dir(root.join("incoming")) {
        for entry in read.flatten() {
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > week);
            if stale {
                let _ = file::remove_all(entry.path());
            }
        }
    }
    let Ok(read) = std::fs::read_dir(root.join("blobs")) else {
        return;
    };
    let mut entries = Vec::new();
    let mut total = 0u64;
    for entry in read.flatten() {
        let dir = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.')
            || !dir.is_dir()
            || keep == Some(dir.as_path())
        {
            continue;
        }
        // Names of one blob share a file where they can, so count the largest.
        let (mut size, mut used) = (0, SystemTime::UNIX_EPOCH);
        for file in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if let Ok(meta) = file.metadata() {
                size = size.max(meta.len());
                used = used.max(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH));
            }
        }
        total += size;
        if used.elapsed().is_ok_and(|age| age < IN_USE) {
            continue;
        }
        entries.push((used, size, dir));
    }
    entries.sort();
    let limit = max_cache_bytes();
    for (_, size, dir) in entries {
        if total <= limit {
            break;
        }
        if file::remove_all(&dir).is_ok() {
            total -= size;
        }
    }
}
