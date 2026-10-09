//! A content-addressed cache of verified download artifacts.
//!
//! Entries are keyed by the checksum a tool pinned (in its options or in
//! `mise.lock`), never by URL, and every entry is hashed again before it is
//! used. A cache that was restored, shared or edited can therefore cost a
//! download but never changes what is installed.

use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::file;
use crate::hash;
use crate::result::Result;
use crate::ui::progress_report::SingleReport;

/// Algorithms an entry may be keyed by. `md5` and `sha1` are not collision
/// resistant enough to address content by.
const ALGOS: &[&str] = &["sha256", "sha384", "sha512", "blake3"];

/// A cached artifact and the file name it was downloaded as.
pub struct Restored {
    pub effective_filename: Option<String>,
}

fn root() -> PathBuf {
    crate::dirs::CACHE.join("downloads-cas")
}

/// The entry for a pinned `algo:hex` checksum, or `None` when the checksum
/// cannot safely name a file.
fn entry(checksum: &str) -> Option<PathBuf> {
    let (algo, hex) = checksum.split_once(':')?;
    let hex = hex.to_lowercase();
    if !ALGOS.contains(&algo) || hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(root().join(algo).join(hex))
}

fn name_path(entry: &Path) -> PathBuf {
    entry.with_extension("name")
}

/// Copy the artifact pinned by `checksum` to `dest` if the cache holds it and
/// it still hashes to `checksum`. An entry that does not is removed.
pub fn restore(
    checksum: &str,
    dest: &Path,
    pr: Option<&dyn SingleReport>,
) -> Result<Option<Restored>> {
    if !Settings::get().download_cache {
        return Ok(None);
    }
    let Some(entry) = entry(checksum) else {
        return Ok(None);
    };
    if !entry.is_file() {
        return Ok(None);
    }
    let (algo, hex) = checksum.split_once(':').expect("entry() checked the form");
    if let Err(err) = hash::ensure_checksum(&entry, hex, pr, algo) {
        debug!("dropping download cache entry {}: {err}", entry.display());
        let _ = file::remove_file(&entry);
        let _ = file::remove_file(name_path(&entry));
        return Ok(None);
    }
    if let Some(parent) = dest.parent() {
        file::create_dir_all(parent)?;
    }
    file::copy(&entry, dest)?;
    let effective_filename = std::fs::read_to_string(name_path(&entry))
        .ok()
        .filter(|name| !name.is_empty());
    Ok(Some(Restored { effective_filename }))
}

/// Keep `src`, which the caller has verified against `checksum`.
pub fn store(checksum: &str, src: &Path, effective_filename: Option<&str>) -> Result<()> {
    if !Settings::get().download_cache {
        return Ok(());
    }
    let Some(entry) = entry(checksum) else {
        return Ok(());
    };
    if entry.is_file() {
        return Ok(());
    }
    let dir = entry.parent().expect("entry() joins a file name");
    file::create_dir_all(dir)?;
    // Written beside the entry and renamed into place, so a reader never
    // sees a partial file.
    let tmp = dir.join(format!(
        "{}.tmp-{}",
        entry.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    file::copy(src, &tmp)?;
    if let Some(name) = effective_filename {
        file::write(name_path(&entry), name)?;
    }
    if let Err(err) = file::rename(&tmp, &entry) {
        let _ = file::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_accepts_pinned_hex_digests_only() {
        let sha = format!("sha256:{}", "ab".repeat(32));
        assert!(
            entry(&sha)
                .unwrap()
                .ends_with(format!("sha256/{}", "ab".repeat(32)))
        );
        assert!(entry(&sha.to_uppercase().replace("SHA256", "sha256")).is_some());
        assert!(entry("md5:d41d8cd98f00b204e9800998ecf8427e").is_none());
        assert!(entry("sha1:da39a3ee5e6b4b0d3255bfef95601890afd80709").is_none());
        assert!(entry("sha256:../../etc/passwd").is_none());
        assert!(entry("sha256:").is_none());
        assert!(entry("deadbeef").is_none());
    }
}
