//! Reproducible tar builder for OCI layers.
//!
//! Produces a gzipped tar where:
//! - All mtimes are zeroed (or set from SOURCE_DATE_EPOCH).
//! - All uid/gid are set from a fixed owner (default 0:0) with empty uname/gname.
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
use std::str::FromStr;

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

/// Numeric owner to write into every tar header in a generated OCI layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerOwner {
    pub uid: u32,
    pub gid: u32,
}

impl LayerOwner {
    pub const fn new(uid: u32, gid: u32) -> Self {
        Self { uid, gid }
    }
}

impl Default for LayerOwner {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl FromStr for LayerOwner {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut parts = s.split(':');
        let uid = parse_owner_id(parts.next().unwrap_or_default(), "uid")?;
        let gid = match parts.next() {
            Some(gid) => parse_owner_id(gid, "gid")?,
            None => uid,
        };
        if parts.next().is_some() {
            return Err("owner must be UID or UID:GID".to_string());
        }
        Ok(Self::new(uid, gid))
    }
}

fn parse_owner_id(value: &str, name: &str) -> std::result::Result<u32, String> {
    if value.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    value
        .parse::<u32>()
        .map_err(|_| format!("{name} must be a non-negative integer <= {}", u32::MAX))
}

/// Build a reproducible gzipped tar layer from files in `src_dir`, placing them
/// under `target_prefix` inside the tar (e.g. `/mise/installs/node/20.0.0`).
///
/// `src_dir` must exist and be a directory. Symlinks are preserved; their
/// targets are NOT followed. `owner` is applied to every emitted tar entry.
pub fn build_layer_from_dir(
    src_dir: &Path,
    target_prefix: &str,
    owner: LayerOwner,
) -> Result<LayerBlob> {
    if !src_dir.is_dir() {
        eyre::bail!("not a directory: {}", src_dir.display());
    }

    let entries = collect_sorted_entries(src_dir, false, owner)?;
    build_layer_from_entries(&entries, target_prefix, owner)
}

/// Build a layer from a source directory, preserving uid/gid/mode from disk.
pub fn build_layer_from_dir_preserve_metadata(
    src_dir: &Path,
    target_prefix: &str,
) -> Result<LayerBlob> {
    if !src_dir.is_dir() {
        eyre::bail!("not a directory: {}", src_dir.display());
    }

    let entries = collect_sorted_entries(src_dir, true, LayerOwner::default())?;
    build_layer_from_entries(&entries, target_prefix, LayerOwner::default())
}

/// Build a layer from an in-memory list of (path_in_tar, content) pairs.
/// Useful for layers that don't correspond to a real directory (e.g. the
/// synthesized config layer). `owner` is applied to every emitted tar entry.
pub fn build_layer_from_files(
    files: &[(String, Vec<u8>, u32)],
    owner: LayerOwner,
) -> Result<LayerBlob> {
    build_layer_from_files_and_dirs(files, &[], owner)
}

/// Build a layer from in-memory file and directory entries.
pub fn build_layer_from_files_and_dirs(
    files: &[(String, Vec<u8>, u32)],
    dirs: &[String],
    owner: LayerOwner,
) -> Result<LayerBlob> {
    let mut sorted: Vec<&(String, Vec<u8>, u32)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut sorted_dirs: Vec<&String> = dirs.iter().collect();
    sorted_dirs.sort();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        builder.mode(tar::HeaderMode::Deterministic);

        // Track which parent directories we've emitted so we don't repeat them.
        let mut emitted_dirs: std::collections::BTreeSet<String> = Default::default();

        for dir in sorted_dirs {
            for parent in parent_dirs(dir) {
                if emitted_dirs.insert(parent.clone()) {
                    emit_dir(&mut builder, &parent, owner)?;
                }
            }
            if emitted_dirs.insert((*dir).clone()) {
                emit_dir(&mut builder, dir, owner)?;
            }
        }

        for (path, contents, mode) in sorted {
            for dir in parent_dirs(path) {
                if emitted_dirs.insert(dir.clone()) {
                    emit_dir(&mut builder, &dir, owner)?;
                }
            }
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(*mode);
            apply_owner(&mut header, owner);
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
    owner: LayerOwner,
    size: u64,
}

#[derive(Debug, Clone)]
enum EntryKind {
    Dir,
    File,
    Symlink(PathBuf),
}

fn collect_sorted_entries(
    src_dir: &Path,
    preserve_metadata: bool,
    owner: LayerOwner,
) -> Result<Vec<Entry>> {
    // Canonicalize once so we can match symlink targets that traverse via
    // different path spellings (e.g. with `..` components or through
    // intermediate symlinks).
    let canonical_src = std::fs::canonicalize(src_dir).unwrap_or_else(|_| src_dir.to_path_buf());

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
            let mode = if preserve_metadata {
                mode_from_metadata(&md)
            } else {
                0o755u32
            };
            (EntryKind::Dir, mode, 0u64)
        } else if file_type.is_symlink() {
            let raw_target = std::fs::read_link(entry.path())?;
            let target = rebase_symlink_target(&raw_target, &abs, &canonical_src, src_dir);
            let mode = if preserve_metadata {
                mode_from_metadata(&md)
            } else {
                0o777u32
            };
            (EntryKind::Symlink(target), mode, 0u64)
        } else {
            let mode = if preserve_metadata {
                mode_from_metadata(&md)
            } else {
                let is_exec = file_is_executable(entry.path(), &md);
                if is_exec { 0o755 } else { 0o644 }
            };
            (EntryKind::File, mode, md.len())
        };
        let entry_owner = if preserve_metadata {
            owner_from_metadata(&md)
        } else {
            owner
        };
        entries.push(Entry {
            rel,
            abs,
            kind,
            mode,
            owner: entry_owner,
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

fn build_layer_from_entries(
    entries: &[Entry],
    target_prefix: &str,
    owner: LayerOwner,
) -> Result<LayerBlob> {
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
                emit_dir(&mut builder, &dir, owner)?;
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
                        emit_dir_with_mode(&mut builder, &path_in_tar, e.owner, e.mode)?;
                    }
                }
                EntryKind::File => {
                    let mut header = Header::new_gnu();
                    header.set_entry_type(EntryType::Regular);
                    header.set_mode(e.mode);
                    apply_owner(&mut header, e.owner);
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
                    apply_owner(&mut header, e.owner);
                    header.set_size(0);
                    header.set_mtime(0);
                    // append_link emits a GNU @LongLink extension when the target
                    // exceeds the ustar 100-byte linkname limit (aube/npm deep store
                    // paths) and sets the checksum; set_link_name() alone errors on
                    // long targets (#10416).
                    builder
                        .append_link(&mut header, &path_in_tar, target)
                        .wrap_err_with(|| {
                            format!("writing symlink {path_in_tar} -> {}", target.display())
                        })?;
                }
            }
        }
        builder.finish()?;
    }

    finalize_layer(tar_bytes)
}

fn apply_owner(header: &mut Header, owner: LayerOwner) {
    header.set_uid(owner.uid as u64);
    header.set_gid(owner.gid as u64);
}

#[cfg(unix)]
fn mode_from_metadata(md: &std::fs::Metadata) -> u32 {
    md.mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_from_metadata(md: &std::fs::Metadata) -> u32 {
    if md.is_dir() { 0o755 } else { 0o644 }
}

#[cfg(unix)]
fn owner_from_metadata(md: &std::fs::Metadata) -> LayerOwner {
    LayerOwner::new(md.uid(), md.gid())
}

#[cfg(not(unix))]
fn owner_from_metadata(_md: &std::fs::Metadata) -> LayerOwner {
    LayerOwner::default()
}

fn emit_dir<W: Write>(builder: &mut tar::Builder<W>, path: &str, owner: LayerOwner) -> Result<()> {
    emit_dir_with_mode(builder, path, owner, 0o755)
}

fn emit_dir_with_mode<W: Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    owner: LayerOwner,
    mode: u32,
) -> Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_mode(mode);
    apply_owner(&mut header, owner);
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

/// If a symlink's target is an absolute path that points inside the tool's
/// install tree, rewrite it to a *relative* symlink so it stays valid after
/// the layer extracts to a different prefix inside the container.
///
/// Absolute targets that fall outside the tool tree are left alone with a
/// warning — they'd be dangling inside the container either way, and
/// rewriting them requires knowledge we don't have at layer-build time.
fn rebase_symlink_target(
    raw: &Path,
    link_abs_path: &Path,
    canonical_src: &Path,
    src_dir: &Path,
) -> PathBuf {
    if !raw.is_absolute() {
        return raw.to_path_buf();
    }

    // Canonicalize the target's parent so we can match layouts where the
    // target is expressed via a different symlink path or with `..`. Fall
    // back to the raw target if canonicalize fails (e.g. the symlink is
    // already dangling on disk — we still want to emit it verbatim).
    let target_canon = std::fs::canonicalize(raw).unwrap_or_else(|_| raw.to_path_buf());

    let rel_target: PathBuf = if let Ok(r) = target_canon.strip_prefix(canonical_src) {
        r.to_path_buf()
    } else if let Ok(r) = raw.strip_prefix(src_dir) {
        r.to_path_buf()
    } else {
        warn!(
            "oci layer: symlink {} → {} has an absolute target outside the tool's install dir; \
             it will be dangling inside the container",
            link_abs_path.display(),
            raw.display()
        );
        return raw.to_path_buf();
    };

    // Compute a relative path from the symlink's directory back up to
    // `src_dir`, then descend into the rebased target. This gives a symlink
    // that's correct in both the host layout and the in-image layout.
    let link_rel = link_abs_path
        .strip_prefix(src_dir)
        .unwrap_or(Path::new(""))
        .to_path_buf();
    let depth = link_rel.components().count().saturating_sub(1);
    let mut out = PathBuf::new();
    for _ in 0..depth {
        out.push("..");
    }
    out.push(rel_target);
    out
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

        let a = build_layer_from_dir(dir.path(), "mise/installs/test/1.0", LayerOwner::default())
            .unwrap();
        let b = build_layer_from_dir(dir.path(), "mise/installs/test/1.0", LayerOwner::default())
            .unwrap();
        assert_eq!(a.digest, b.digest, "digests should match across runs");
        assert_eq!(a.diff_id, b.diff_id, "diff_ids should match across runs");
        assert_eq!(a.bytes, b.bytes, "bytes should match across runs");
    }

    #[test]
    fn different_prefix_different_digest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("x"), b"x").unwrap();
        let a = build_layer_from_dir(dir.path(), "a", LayerOwner::default()).unwrap();
        let b = build_layer_from_dir(dir.path(), "b", LayerOwner::default()).unwrap();
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
        let a = build_layer_from_files(&files, LayerOwner::default()).unwrap();
        let b = build_layer_from_files(&files, LayerOwner::default()).unwrap();
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn default_files_layer_owner_is_root() {
        let files = vec![("etc/mise/config.toml".to_string(), b"foo\n".to_vec(), 0o644)];
        let blob = build_layer_from_files(&files, LayerOwner::default()).unwrap();

        assert_layer_owner(&blob, 0, 0);
    }

    #[test]
    fn configured_files_layer_owner_applies_to_every_entry_and_is_reproducible() {
        let files = vec![
            ("etc/mise/config.toml".to_string(), b"foo\n".to_vec(), 0o644),
            (
                "usr/local/bin/mise".to_string(),
                b"#!/bin/sh\nexec true\n".to_vec(),
                0o755,
            ),
        ];
        let owner = LayerOwner::new(1000, 1001);

        let a = build_layer_from_files(&files, owner).unwrap();
        let b = build_layer_from_files(&files, owner).unwrap();

        assert_layer_owner(&a, 1000, 1001);
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn configured_dir_layer_owner_applies_to_every_entry() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        fs::write(dir.path().join("bin/hello"), b"#!/bin/sh\necho hi\n").unwrap();

        let blob = build_layer_from_dir(
            dir.path(),
            "opt/mise/installs/test/1.0",
            LayerOwner::new(1000, 1000),
        )
        .unwrap();

        assert_layer_owner(&blob, 1000, 1000);
    }

    #[cfg(unix)]
    #[test]
    fn preserve_metadata_dir_layer_keeps_special_permission_bits() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o1777)).unwrap();
        let helper = bin.join("helper");
        fs::write(&helper, b"#!/bin/sh\necho hi\n").unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o4755)).unwrap();

        let blob = build_layer_from_dir_preserve_metadata(dir.path(), "").unwrap();

        assert_layer_mode(&blob, "bin", 0o1777);
        assert_layer_mode(&blob, "bin/helper", 0o4755);
        let md = fs::symlink_metadata(&helper).unwrap();
        assert_layer_entry_owner(&blob, "bin/helper", md.uid() as u64, md.gid() as u64);
    }

    #[test]
    fn layer_owner_parses_uid_and_optional_gid() {
        assert_eq!(
            "1000".parse::<LayerOwner>().unwrap(),
            LayerOwner::new(1000, 1000)
        );
        assert_eq!(
            "1000:1001".parse::<LayerOwner>().unwrap(),
            LayerOwner::new(1000, 1001)
        );

        for invalid in ["", ":", "1000:", ":1000", "1:2:3", "abc", "-1"] {
            assert!(
                invalid.parse::<LayerOwner>().is_err(),
                "{invalid:?} should be rejected"
            );
        }
    }

    fn assert_layer_owner(blob: &LayerBlob, uid: u64, gid: u64) {
        let decoder = flate2::read::GzDecoder::new(blob.bytes.as_slice());
        let mut archive = tar::Archive::new(decoder);
        let mut entries_seen = 0;

        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            assert_eq!(entry.header().uid().unwrap(), uid, "uid for {path}");
            assert_eq!(entry.header().gid().unwrap(), gid, "gid for {path}");
            entries_seen += 1;
        }

        assert!(entries_seen > 0, "expected at least one tar entry");
    }

    #[cfg(unix)]
    fn assert_layer_mode(blob: &LayerBlob, expected_path: &str, expected_mode: u32) {
        let decoder = flate2::read::GzDecoder::new(blob.bytes.as_slice());
        let mut archive = tar::Archive::new(decoder);

        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            if path.trim_end_matches('/') == expected_path.trim_end_matches('/') {
                assert_eq!(
                    entry.header().mode().unwrap(),
                    expected_mode,
                    "mode for {path}"
                );
                return;
            }
        }

        panic!("expected layer entry {expected_path}");
    }

    #[cfg(unix)]
    fn assert_layer_entry_owner(blob: &LayerBlob, expected_path: &str, uid: u64, gid: u64) {
        let decoder = flate2::read::GzDecoder::new(blob.bytes.as_slice());
        let mut archive = tar::Archive::new(decoder);

        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            if path == expected_path {
                assert_eq!(entry.header().uid().unwrap(), uid, "uid for {path}");
                assert_eq!(entry.header().gid().unwrap(), gid, "gid for {path}");
                return;
            }
        }

        panic!("expected layer entry {expected_path}");
    }

    #[cfg(unix)]
    #[test]
    fn absolute_intra_tree_symlinks_become_relative() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let src = dir.path();
        fs::create_dir_all(src.join("bin")).unwrap();
        fs::create_dir_all(src.join("lib/node_modules/npm/bin")).unwrap();
        fs::write(
            src.join("lib/node_modules/npm/bin/npm-cli.js"),
            b"#!/bin/sh\n",
        )
        .unwrap();

        // Absolute symlink pointing within the same install tree.
        let canonical = std::fs::canonicalize(src).unwrap();
        let target = canonical.join("lib/node_modules/npm/bin/npm-cli.js");
        symlink(&target, src.join("bin/npm")).unwrap();

        // Absolute symlink pointing OUTSIDE the install tree (should be left
        // as-is with a warning; we only assert it doesn't panic).
        symlink("/usr/bin/false", src.join("bin/external")).unwrap();

        let entries = collect_sorted_entries(src, false, LayerOwner::default()).unwrap();
        let npm = entries
            .iter()
            .find(|e| e.rel == Path::new("bin/npm"))
            .unwrap();
        match &npm.kind {
            EntryKind::Symlink(t) => {
                assert!(
                    !t.is_absolute(),
                    "intra-tree symlink should have been rewritten to relative, got {t:?}",
                );
                assert_eq!(t, &PathBuf::from("../lib/node_modules/npm/bin/npm-cli.js"),);
            }
            k => panic!("expected symlink, got {k:?}"),
        }
        let external = entries
            .iter()
            .find(|e| e.rel == Path::new("bin/external"))
            .unwrap();
        match &external.kind {
            EntryKind::Symlink(t) => assert_eq!(t, &PathBuf::from("/usr/bin/false")),
            k => panic!("expected symlink, got {k:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn long_symlink_target_written_via_gnu_longlink() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let src = dir.path();
        fs::create_dir_all(src.join("bin")).unwrap();

        // A relative symlink target well over tar's 100-byte linkname limit,
        // mirroring aube/npm deep node_modules store paths (#10416). With the old
        // manual set_link_name() this aborted the whole layer build.
        let long_target = format!("../{}cli.js", "node_modules/.aube/pkg/".repeat(8));
        assert!(
            long_target.len() > 100,
            "target must exceed the tar linkname limit"
        );
        symlink(&long_target, src.join("bin/cli")).unwrap();

        let entries = collect_sorted_entries(src, false, LayerOwner::default()).unwrap();
        let blob = build_layer_from_entries(&entries, "mise", LayerOwner::default()).unwrap();

        // The GNU @LongLink extension must round-trip back to the full target.
        let decoder = flate2::read::GzDecoder::new(blob.bytes.as_slice());
        let mut archive = tar::Archive::new(decoder);
        let mut found = false;
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            if path == "mise/bin/cli" {
                let link = entry.link_name().unwrap().unwrap();
                assert_eq!(&*link, Path::new(&long_target));
                found = true;
            }
        }
        assert!(found, "symlink entry mise/bin/cli not found in layer");
    }
}
