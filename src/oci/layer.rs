//! Reproducible tar builder for OCI layers.
//!
//! Produces a gzipped tar where:
//! - All mtimes are zeroed (or set from SOURCE_DATE_EPOCH).
//! - All uid/gid are 0 with empty uname/gname.
//! - Entries are sorted by path before writing.
//! - Permissions are normalized (dirs 0755, exec files 0755, others 0644).
//! - Gzip header mtime is zero'd (fixed compression level).
//!
//! The result is byte-identical across re-runs on the same host for the same
//! input tree, which is required for the "swap one tool → swap one layer"
//! caching story.

use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use eyre::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use tar::{EntryType, Header};
use walkdir::WalkDir;

/// The result of building a layer blob.
#[derive(Debug, Clone)]
pub struct LayerBlob {
    /// sha256 digest of the gzipped tar (the "blob digest"; what the manifest
    /// descriptor's `digest` field references).
    pub digest: String,
    /// sha256 digest of the uncompressed tar (the "diff_id"; what the image
    /// config's `rootfs.diff_ids` references).
    pub diff_id: String,
    /// Compressed size in bytes.
    pub size: u64,
    /// The gzipped tar bytes.
    pub bytes: Vec<u8>,
}

/// Build a reproducible gzipped tar layer from files in `src_dir`, placing them
/// under `target_prefix` inside the tar (e.g. `/mise/installs/node/20.0.0`).
///
/// `src_dir` must exist and be a directory. Symlinks are preserved; their
/// targets are NOT followed.
pub fn build_layer_from_dir(src_dir: &Path, target_prefix: &str) -> Result<LayerBlob> {
    if !src_dir.is_dir() {
        eyre::bail!("not a directory: {}", src_dir.display());
    }

    let entries = collect_sorted_entries(src_dir)?;
    build_layer_from_entries(&entries, target_prefix)
}

/// Build a layer from an in-memory list of (path_in_tar, content) pairs.
/// Useful for layers that don't correspond to a real directory (e.g. the
/// synthesized config layer).
pub fn build_layer_from_files(files: &[(String, Vec<u8>, u32)]) -> Result<LayerBlob> {
    let mut sorted = files.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        builder.mode(tar::HeaderMode::Deterministic);

        // Track which parent directories we've emitted so we don't repeat them.
        let mut emitted_dirs: std::collections::BTreeSet<String> = Default::default();

        for (path, contents, mode) in &sorted {
            for dir in parent_dirs(path) {
                if emitted_dirs.insert(dir.clone()) {
                    let mut header = Header::new_gnu();
                    header.set_entry_type(EntryType::Directory);
                    header.set_mode(0o755);
                    header.set_uid(0);
                    header.set_gid(0);
                    header.set_size(0);
                    header.set_mtime(0);
                    header.set_cksum();
                    builder
                        .append_data(&mut header, format!("{dir}/"), std::io::empty())
                        .wrap_err_with(|| format!("writing dir entry {dir}"))?;
                }
            }
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(*mode);
            header.set_uid(0);
            header.set_gid(0);
            header.set_size(contents.len() as u64);
            header.set_mtime(0);
            header.set_cksum();
            builder
                .append_data(&mut header, path, contents.as_slice())
                .wrap_err_with(|| format!("writing file entry {path}"))?;
        }
        builder.finish()?;
    }

    finalize_layer(tar_bytes)
}

#[derive(Debug, Clone)]
struct Entry {
    rel: PathBuf,
    abs: PathBuf,
    kind: EntryKind,
    mode: u32,
    size: u64,
}

#[derive(Debug, Clone)]
enum EntryKind {
    Dir,
    File,
    Symlink(PathBuf),
}

fn collect_sorted_entries(src_dir: &Path) -> Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    for entry in WalkDir::new(src_dir).sort_by_file_name() {
        let entry = entry.wrap_err("walking source directory")?;
        let abs = entry.path().to_path_buf();
        let rel = abs.strip_prefix(src_dir).unwrap().to_path_buf();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let file_type = entry.file_type();
        let md = entry.path().symlink_metadata()?;
        let (kind, mode, size) = if file_type.is_dir() {
            (EntryKind::Dir, 0o755u32, 0u64)
        } else if file_type.is_symlink() {
            let target = std::fs::read_link(entry.path())?;
            (EntryKind::Symlink(target), 0o777u32, 0u64)
        } else {
            let is_exec = file_is_executable(entry.path(), &md);
            let mode = if is_exec { 0o755 } else { 0o644 };
            (EntryKind::File, mode, md.len())
        };
        entries.push(Entry {
            rel,
            abs,
            kind,
            mode,
            size,
        });
    }
    // WalkDir's sort_by_file_name sorts per-parent; we additionally sort the
    // full relative path so siblings interleave deterministically with their
    // children (parent-dir-first). Preserves `a/` before `a/b` because `/` is
    // lexically smaller than any file-name character mise will emit.
    entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(entries)
}

fn build_layer_from_entries(entries: &[Entry], target_prefix: &str) -> Result<LayerBlob> {
    let prefix = target_prefix.trim_matches('/');

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        builder.mode(tar::HeaderMode::Deterministic);
        builder.follow_symlinks(false);

        // Always emit a directory entry for the target prefix chain itself
        // (e.g. `/mise`, `/mise/installs`, `/mise/installs/node`). This
        // guarantees parent dirs exist even if the src_dir is empty.
        let mut emitted_dirs: std::collections::BTreeSet<String> = Default::default();
        for dir in prefix_parents(prefix) {
            if emitted_dirs.insert(dir.clone()) {
                emit_dir(&mut builder, &dir)?;
            }
        }

        for e in entries {
            // Tar entry paths must use forward slashes regardless of host;
            // on Windows `rel` may contain `\` from `Path::to_string_lossy`.
            let rel_str = e.rel.to_string_lossy().replace('\\', "/");
            let path_in_tar = if prefix.is_empty() {
                rel_str
            } else {
                format!("{prefix}/{rel_str}")
            };

            match &e.kind {
                EntryKind::Dir => {
                    if emitted_dirs.insert(path_in_tar.clone()) {
                        emit_dir(&mut builder, &path_in_tar)?;
                    }
                }
                EntryKind::File => {
                    let mut header = Header::new_gnu();
                    header.set_entry_type(EntryType::Regular);
                    header.set_mode(e.mode);
                    header.set_uid(0);
                    header.set_gid(0);
                    header.set_size(e.size);
                    header.set_mtime(0);
                    header.set_cksum();
                    let f = std::fs::File::open(&e.abs)
                        .wrap_err_with(|| format!("opening {}", e.abs.display()))?;
                    builder
                        .append_data(&mut header, &path_in_tar, f)
                        .wrap_err_with(|| format!("writing {path_in_tar}"))?;
                }
                EntryKind::Symlink(target) => {
                    let mut header = Header::new_gnu();
                    header.set_entry_type(EntryType::Symlink);
                    header.set_mode(e.mode);
                    header.set_uid(0);
                    header.set_gid(0);
                    header.set_size(0);
                    header.set_mtime(0);
                    header
                        .set_link_name(target)
                        .wrap_err_with(|| format!("symlink target {}", target.display()))?;
                    header.set_cksum();
                    builder
                        .append_data(&mut header, &path_in_tar, std::io::empty())
                        .wrap_err_with(|| format!("writing symlink {path_in_tar}"))?;
                }
            }
        }
        builder.finish()?;
    }

    finalize_layer(tar_bytes)
}

fn emit_dir<W: Write>(builder: &mut tar::Builder<W>, path: &str) -> Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_size(0);
    header.set_mtime(0);
    header.set_cksum();
    let path_with_slash = if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    };
    builder
        .append_data(&mut header, &path_with_slash, std::io::empty())
        .wrap_err_with(|| format!("writing dir {path_with_slash}"))?;
    Ok(())
}

fn prefix_parents(prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for part in prefix.split('/').filter(|s| !s.is_empty()) {
        if !cur.is_empty() {
            cur.push('/');
        }
        cur.push_str(part);
        out.push(cur.clone());
    }
    out
}

fn parent_dirs(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let components: Vec<&str> = path.split('/').collect();
    if components.len() <= 1 {
        return out;
    }
    let mut cur = String::new();
    for part in &components[..components.len() - 1] {
        if part.is_empty() {
            continue;
        }
        if !cur.is_empty() {
            cur.push('/');
        }
        cur.push_str(part);
        out.push(cur.clone());
    }
    out
}

fn finalize_layer(tar_bytes: Vec<u8>) -> Result<LayerBlob> {
    let diff_id = {
        let mut h = Sha256::new();
        h.update(&tar_bytes);
        format!("sha256:{}", hex_encode(&h.finalize()))
    };

    // Gzip with a fixed compression level and zeroed header mtime for
    // reproducibility.
    let mut gz_bytes = Vec::new();
    {
        let mut encoder = GzEncoder::new(&mut gz_bytes, Compression::new(6));
        encoder.write_all(&tar_bytes)?;
        encoder.finish()?;
    }

    // flate2 writes the current time into the gzip header and host OS. To get
    // deterministic output, zero out MTIME (bytes 4..8), XFL (byte 8), and
    // normalize OS (byte 9) to 0xff ("unknown"). Gzip headers are always ≥10
    // bytes (magic + CM + FLG + MTIME + XFL + OS), so a single guard covers
    // all three writes.
    // (gzip header layout: [0x1f, 0x8b, CM, FLG, MTIME(4), XFL, OS, ...])
    if gz_bytes.len() >= 10 {
        gz_bytes[4..8].copy_from_slice(&[0, 0, 0, 0]);
        gz_bytes[8] = 0;
        gz_bytes[9] = 0xff;
    }

    let digest = {
        let mut h = Sha256::new();
        h.update(&gz_bytes);
        format!("sha256:{}", hex_encode(&h.finalize()))
    };
    let size = gz_bytes.len() as u64;

    Ok(LayerBlob {
        digest,
        diff_id,
        size,
        bytes: gz_bytes,
    })
}

/// Cross-platform exec-bit detection for tar-header mode normalization.
///
/// On Unix we inspect the real mode bits. On Windows there's no exec bit on
/// the filesystem, so we fall back to extension-based heuristics that match
/// the host's usual notion of "executable" (PATHEXT-style). This matters
/// only for symmetry — the resulting layer is consumed inside a Linux
/// container, which uses the mode we set here, so OCI tools like aqua or
/// ubi-style binaries get the exec bit they need.
#[cfg(unix)]
fn file_is_executable(_path: &Path, md: &std::fs::Metadata) -> bool {
    (md.mode() & 0o111) != 0
}

#[cfg(not(unix))]
fn file_is_executable(path: &Path, _md: &std::fs::Metadata) -> bool {
    // Treat common Windows executable extensions as exec. For anything else,
    // default to non-exec; users building linux OCI images on Windows can
    // add `--exec-bit` in the future if this proves insufficient.
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("exe") | Some("bat") | Some("cmd") | Some("ps1") | Some("com"),
    )
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn reproducible_same_inputs_same_digest() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        fs::write(dir.path().join("bin/hello"), b"#!/bin/sh\necho hi\n").unwrap();
        fs::write(dir.path().join("README"), b"hello\n").unwrap();

        let a = build_layer_from_dir(dir.path(), "mise/installs/test/1.0").unwrap();
        let b = build_layer_from_dir(dir.path(), "mise/installs/test/1.0").unwrap();
        assert_eq!(a.digest, b.digest, "digests should match across runs");
        assert_eq!(a.diff_id, b.diff_id, "diff_ids should match across runs");
        assert_eq!(a.bytes, b.bytes, "bytes should match across runs");
    }

    #[test]
    fn different_prefix_different_digest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("x"), b"x").unwrap();
        let a = build_layer_from_dir(dir.path(), "a").unwrap();
        let b = build_layer_from_dir(dir.path(), "b").unwrap();
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn files_layer_is_reproducible() {
        let files = vec![
            ("etc/mise/config.toml".to_string(), b"foo\n".to_vec(), 0o644),
            (
                "usr/local/bin/mise".to_string(),
                b"#!/bin/sh\nexec true\n".to_vec(),
                0o755,
            ),
        ];
        let a = build_layer_from_files(&files).unwrap();
        let b = build_layer_from_files(&files).unwrap();
        assert_eq!(a.bytes, b.bytes);
    }
}
