//! Cache the result of packaging a local tool, keyed by the actual layer inputs.
//! Reading and hashing files is intentional: reinstalling or editing a tool at
//! the same version must not reuse stale bytes. Hits skip tar construction and
//! gzip compression. Each image still receives its own complete layer blob.

use serde::{Deserialize, Serialize};

use super::*;

#[derive(Serialize, Deserialize)]
struct CachedLayer {
    digest: String,
    diff_id: String,
    size: u64,
}

pub(crate) fn build_cached_tool_layer(
    src_dir: &Path,
    target_prefix: &str,
    owner: LayerOwner,
    relocation: &ToolRelocation,
    cache_dir: &Path,
) -> Result<(LayerBlob, bool)> {
    if !src_dir.is_dir() {
        eyre::bail!("not a directory: {}", src_dir.display());
    }
    let entries = collect_sorted_entries(src_dir, false, owner, Some(relocation))?;
    let key = fingerprint(&entries, target_prefix, owner, relocation)?;
    let record_path = cache_dir.join(format!("{key}.json"));
    let _lock = match crate::lock_file::LockFile::at(&cache_dir.join(format!("{key}.lock"))).lock()
    {
        Ok(lock) => lock,
        Err(err) => {
            debug!("could not lock OCI tool layer cache: {err:#}");
            return Ok((
                build_layer_from_entries(&entries, target_prefix, owner, Some(relocation))?,
                false,
            ));
        }
    };
    match read_cached_layer(&record_path, cache_dir) {
        Ok(Some(blob)) => return Ok((blob, true)),
        Ok(None) => {}
        Err(err) => debug!("ignoring invalid OCI tool layer cache: {err:#}"),
    }

    let blob = build_layer_from_entries(&entries, target_prefix, owner, Some(relocation))?;
    // Do not publish under the old key if the installation changed while we
    // were building. The next invocation will fingerprint its new contents.
    let after = collect_sorted_entries(src_dir, false, owner, Some(relocation))?;
    if fingerprint(&after, target_prefix, owner, relocation)? == key
        && let Err(err) = write_cached_layer(&record_path, cache_dir, &blob)
    {
        debug!("could not cache OCI tool layer: {err:#}");
    }
    Ok((blob, false))
}

fn fingerprint(
    entries: &[Entry],
    prefix: &str,
    owner: LayerOwner,
    relocation: &ToolRelocation,
) -> Result<String> {
    let mut hash = Sha256::new();
    // Bump the format when layer construction changes without a mise release.
    field(&mut hash, b"mise-oci-tool-layer-v1");
    field(&mut hash, env!("CARGO_PKG_VERSION").as_bytes());
    field(&mut hash, std::env::consts::OS.as_bytes());
    field(&mut hash, std::env::consts::ARCH.as_bytes());
    field(&mut hash, prefix.as_bytes());
    field(&mut hash, &owner.uid.to_le_bytes());
    field(&mut hash, &owner.gid.to_le_bytes());
    for entry in entries {
        field(&mut hash, entry.rel.as_os_str().as_encoded_bytes());
        field(&mut hash, &entry.mode.to_le_bytes());
        match &entry.kind {
            EntryKind::Dir => field(&mut hash, b"directory"),
            EntryKind::Symlink(target) => {
                field(&mut hash, b"symlink");
                field(&mut hash, target.as_os_str().as_encoded_bytes());
            }
            EntryKind::File => {
                field(&mut hash, b"file");
                // Hash precisely the bytes the tar writer will use, including
                // rewritten shebangs and Python virtual environment metadata.
                let mut reader: Box<dyn Read> =
                    match relocated_file_contents(entry, Some(relocation))? {
                        Some(contents) => contents.reader,
                        None => Box::new(std::fs::File::open(&entry.abs)?),
                    };
                let mut contents_hash = Sha256::new();
                let mut buffer = [0; 64 * 1024];
                loop {
                    let n = reader.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    contents_hash.update(&buffer[..n]);
                }
                field(&mut hash, &contents_hash.finalize());
            }
        }
    }
    Ok(hex_encode(&hash.finalize()))
}

fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn read_cached_layer(record_path: &Path, cache_dir: &Path) -> Result<Option<LayerBlob>> {
    let record = match std::fs::read(record_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let record: CachedLayer = serde_json::from_slice(&record)?;
    crate::oci::layout::validate_sha256_digest(&record.digest)?;
    crate::oci::layout::validate_sha256_digest(&record.diff_id)?;
    let bytes = std::fs::read(cache_dir.join(record.digest.trim_start_matches("sha256:")))?;
    eyre::ensure!(
        bytes.len() as u64 == record.size,
        "cached OCI layer size mismatch"
    );
    let digest = format!("sha256:{}", hex_encode(&Sha256::digest(&bytes)));
    eyre::ensure!(digest == record.digest, "cached OCI layer digest mismatch");
    Ok(Some(LayerBlob {
        digest,
        diff_id: record.diff_id,
        size: record.size,
        bytes,
    }))
}

fn write_cached_layer(record_path: &Path, cache_dir: &Path, blob: &LayerBlob) -> Result<()> {
    crate::file::create_dir_all(cache_dir)?;
    let blob_path = cache_dir.join(blob.digest.trim_start_matches("sha256:"));
    // Atomic blob publication followed by atomic metadata publication lets
    // concurrent builds share this cache without sharing an image index.
    crate::file::write_atomic(blob_path, &blob.bytes)?;
    crate::file::write_atomic(
        record_path,
        serde_json::to_vec(&CachedLayer {
            digest: blob.digest.clone(),
            diff_id: blob.diff_id.clone(),
            size: blob.size,
        })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_changes_invalidate_cache_even_with_preserved_size_and_mtime() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("tool");
        let cache = td.path().join("cache");
        std::fs::create_dir(&src).unwrap();
        let path = src.join("file");
        std::fs::write(&path, "first").unwrap();
        let owner = LayerOwner::default();
        let relocation = ToolRelocation::default();
        let build =
            || build_cached_tool_layer(&src, "mise/tool", owner, &relocation, &cache).unwrap();
        let (first, hit) = build();
        assert!(!hit);
        let (second, hit) = build();
        assert!(hit);
        assert_eq!(first.bytes, second.bytes);
        let mtime =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&path).unwrap());
        std::fs::write(&path, "other").unwrap();
        filetime::set_file_mtime(&path, mtime).unwrap();
        let (changed, hit) = build();
        assert!(!hit);
        assert_ne!(first.digest, changed.digest);
        assert!(build().1);
        std::fs::remove_file(&path).unwrap();
        assert!(!build().1);
    }

    #[cfg(unix)]
    #[test]
    fn ownership_prefix_and_relocated_contents_have_separate_entries() {
        use std::os::unix::fs::PermissionsExt;
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("tool");
        let cache = td.path().join("cache");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(
            src.join("launcher"),
            format!("#!{}/bin/python\n", src.display()),
        )
        .unwrap();
        std::fs::set_permissions(src.join("launcher"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let original = ToolRelocation::new(vec![(src.clone(), "/mise/python".into())]);
        let changed = ToolRelocation::new(vec![(src.clone(), "/other/python".into())]);
        let mut digests = std::collections::HashSet::new();
        for (prefix, owner, relocation) in [
            ("mise/tool", LayerOwner::default(), &original),
            ("other/tool", LayerOwner::default(), &original),
            ("mise/tool", LayerOwner::new(1000, 1000), &original),
            ("mise/tool", LayerOwner::default(), &changed),
        ] {
            let (blob, hit) =
                build_cached_tool_layer(&src, prefix, owner, relocation, &cache).unwrap();
            assert!(!hit);
            assert!(digests.insert(blob.digest));
            assert!(
                build_cached_tool_layer(&src, prefix, owner, relocation, &cache)
                    .unwrap()
                    .1
            );
        }
    }

    #[test]
    fn corrupt_or_missing_cached_blobs_are_rebuilt() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("tool");
        let cache = td.path().join("cache");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file"), "content").unwrap();
        let relocation = ToolRelocation::default();
        let build = || {
            build_cached_tool_layer(
                &src,
                "mise/tool",
                LayerOwner::default(),
                &relocation,
                &cache,
            )
            .unwrap()
        };
        let (first, _) = build();
        let blob_path = cache.join(first.digest.trim_start_matches("sha256:"));
        std::fs::write(&blob_path, vec![0; first.size as usize]).unwrap();
        let (rebuilt, hit) = build();
        assert!(!hit);
        assert_eq!(rebuilt.bytes, first.bytes);
        assert!(build().1);
        std::fs::remove_file(blob_path).unwrap();
        assert!(!build().1);
    }

    #[test]
    fn concurrent_builds_package_the_same_inputs_once() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("tool");
        let cache = td.path().join("cache");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file"), "content").unwrap();
        let barrier = std::sync::Barrier::new(4);
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        build_cached_tool_layer(
                            &src,
                            "mise/tool",
                            LayerOwner::default(),
                            &ToolRelocation::default(),
                            &cache,
                        )
                        .unwrap()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|(_, hit)| !hit).count(), 1);
        assert!(
            results
                .iter()
                .all(|(blob, _)| blob.bytes == results[0].0.bytes)
        );
    }

    #[cfg(unix)]
    #[test]
    fn executable_modes_and_symlink_targets_invalidate_cache() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("tool");
        let cache = td.path().join("cache");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("file"), "contents").unwrap();
        std::fs::set_permissions(src.join("file"), std::fs::Permissions::from_mode(0o644)).unwrap();
        symlink("file", src.join("link")).unwrap();
        let relocation = ToolRelocation::default();
        let build = || {
            build_cached_tool_layer(
                &src,
                "mise/tool",
                LayerOwner::default(),
                &relocation,
                &cache,
            )
            .unwrap()
        };
        let (original, _) = build();
        assert!(build().1);
        std::fs::set_permissions(src.join("file"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let (executable, hit) = build();
        assert!(!hit);
        assert_ne!(original.digest, executable.digest);
        std::fs::remove_file(src.join("link")).unwrap();
        symlink("elsewhere", src.join("link")).unwrap();
        let (retargeted, hit) = build();
        assert!(!hit);
        assert_ne!(executable.digest, retargeted.digest);
    }
}
