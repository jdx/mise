//! Private write-ahead recovery. These bytes never enter Git or durable history.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use eyre::{Result, bail};
use sha2::Digest;

use super::journal::{Blob, Capture, JournalEntry, PathSnapshot, PathState};

/// Recover only writes whose current state is still the recorded result.
/// Missing completion records and concurrent edits require human intervention.
pub(crate) fn recover(state_dir: &Path, journal: &[JournalEntry]) -> Result<()> {
    recover_entries(state_dir, journal, false)
}

/// A normally returned failure leaves earlier completed phases in place.
/// Only writes lacking a completion record still need verification.
pub(crate) fn recover_unfinished(state_dir: &Path, journal: &[JournalEntry]) -> Result<()> {
    recover_entries(state_dir, journal, true)
}

fn recover_entries(
    state_dir: &Path,
    journal: &[JournalEntry],
    unfinished_only: bool,
) -> Result<()> {
    let mut failures = vec![];
    let mut blocked: Vec<&Path> = vec![];
    for (seq, entry) in journal.iter().enumerate().rev() {
        let JournalEntry::PathChanged { path, prior, .. } = entry else {
            continue;
        };
        if blocked
            .iter()
            .any(|blocked| path.starts_with(blocked) || blocked.starts_with(path))
        {
            continue;
        }
        let after = journal.iter().rev().find_map(|entry| match entry {
            JournalEntry::Committed {
                seq: recorded,
                after,
            } if *recorded as usize == seq => Some(after),
            _ => None,
        });
        if unfinished_only && after.is_some() {
            continue;
        }
        if let Err(error) = recover_path(state_dir, path, prior, after) {
            blocked.push(path);
            failures.push(format!("{}: {error:#}", crate::file::display_path(path)));
        }
    }
    if !failures.is_empty() {
        bail!(
            "interrupted file recovery needs attention; temporary recovery data was retained. {}. Run `mise bootstrap dotfiles recover` to retry, or inspect the files and use `recover <operation> --keep-current` to explicitly accept their current contents",
            failures.join("; ")
        );
    }
    Ok(())
}

fn recover_path(
    state_dir: &Path,
    path: &Path,
    prior: &PathSnapshot,
    after: Option<&PathState>,
) -> Result<()> {
    validate_destination(path)?;
    let comparison = tempfile::tempdir()?;
    let depth = if matches!(prior, PathSnapshot::Directory { .. }) {
        Capture::Shallow
    } else {
        Capture::Full
    };
    // Comparing must not leave copies of concurrent user edits in the store.
    let current = PathSnapshot::capture_with(comparison.path(), path, depth);
    if !matches!(prior, PathSnapshot::Unrecorded { .. }) && &current == prior {
        return Ok(());
    }
    let Some(after) = after else {
        bail!("write completion was not recorded; inspect the live file before retrying recovery");
    };
    if PathState::observe(path) != *after {
        bail!("changed after the operation; left untouched");
    }
    // Entry count alone cannot establish a directory's identity. Never
    // replace a populated directory on that evidence.
    if matches!(after, PathState::Dir { entries, .. } if *entries != 0)
        && !(matches!(prior, PathSnapshot::Directory { .. })
            && matches!(
                after,
                PathState::Dir {
                    identity: Some(_),
                    ..
                }
            ))
    {
        bail!("directory contents cannot be verified safely; left untouched");
    }
    validate_snapshot(state_dir, prior)?;
    if PathState::observe(path) != *after {
        bail!("changed while preparing recovery; left untouched");
    }
    restore(state_dir, path, prior)
}

fn validate_destination(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("invalid recovery destination");
    }
    for parent in path.ancestors().skip(1) {
        if std::fs::symlink_metadata(parent).is_ok_and(|meta| meta.is_symlink()) {
            bail!("a parent directory is now a symlink; left untouched");
        }
    }
    Ok(())
}

fn read_blob(state_dir: &Path, blob: &Blob) -> Result<Vec<u8>> {
    use base64::Engine;
    if blob.sha256.len() != 64 || !blob.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid recovery content identifier");
    }
    let bytes = match &blob.inline {
        Some(inline) => base64::engine::general_purpose::STANDARD.decode(inline)?,
        None => {
            let path = super::journal::blobs_dir_in(state_dir).join(&blob.sha256);
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.len() != blob.size {
                bail!("invalid recovery content file");
            }
            std::fs::read(path)?
        }
    };
    if bytes.len() as u64 != blob.size || hex::encode(sha2::Sha256::digest(&bytes)) != blob.sha256 {
        bail!("recovery content failed verification");
    }
    Ok(bytes)
}

fn validate_snapshot(state_dir: &Path, snapshot: &PathSnapshot) -> Result<()> {
    match snapshot {
        PathSnapshot::File { content, .. } => {
            read_blob(state_dir, content)?;
        }
        PathSnapshot::Dir {
            files, links, dirs, ..
        } => {
            let mut seen = BTreeSet::new();
            for relative in files
                .iter()
                .map(|f| &f.rel)
                .chain(links.iter().map(|f| &f.rel))
                .chain(dirs.iter().map(|f| &f.rel))
            {
                if relative.as_os_str().is_empty()
                    || !relative
                        .components()
                        .all(|c| matches!(c, Component::Normal(_)))
                    || !seen.insert(relative)
                {
                    bail!("invalid path inside recovery directory");
                }
            }
            for leaf in files
                .iter()
                .map(|f| &f.rel)
                .chain(links.iter().map(|f| &f.rel))
            {
                if seen
                    .iter()
                    .any(|other| *other != leaf && other.starts_with(leaf))
                {
                    bail!("recovery directory descends through a file or symlink");
                }
            }
            for file in files {
                read_blob(state_dir, &file.content)?;
            }
        }
        PathSnapshot::Unrecorded { reason, .. } => bail!("no usable preimage: {reason}"),
        _ => {}
    }
    Ok(())
}

fn remove_leaf(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir(path)?,
        Ok(_) => std::fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o7777))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

fn restore(state_dir: &Path, path: &Path, snapshot: &PathSnapshot) -> Result<()> {
    match snapshot {
        PathSnapshot::Missing => remove_leaf(path)?,
        PathSnapshot::File { content, mode } => {
            use std::io::Write;
            let bytes = read_blob(state_dir, content)?;
            let parent = path.parent().ok_or_else(|| eyre::eyre!("missing parent"))?;
            crate::file::create_dir_all(parent)?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            temporary.write_all(&bytes)?;
            set_mode(temporary.path(), *mode)?;
            temporary.as_file().sync_all()?;
            if path.is_dir() && !path.is_symlink() {
                std::fs::remove_dir(path)?;
            }
            temporary.persist(path)?;
        }
        PathSnapshot::Symlink { dest } => {
            remove_leaf(path)?;
            crate::file::make_symlink(dest, path)?;
        }
        PathSnapshot::Directory { mode } => {
            if path.is_symlink() || path.is_file() {
                remove_leaf(path)?;
            }
            crate::file::create_dir_all(path)?;
            set_mode(path, *mode)?;
        }
        PathSnapshot::Dir {
            files,
            links,
            dirs,
            mode,
        } => {
            remove_leaf(path)?;
            super::store::create_private_dir(path)?;
            for dir in dirs {
                crate::file::create_dir_all(path.join(&dir.rel))?;
            }
            for file in files {
                restore(
                    state_dir,
                    &path.join(&file.rel),
                    &PathSnapshot::File {
                        content: file.content.clone(),
                        mode: file.mode,
                    },
                )?;
            }
            for link in links {
                let target = path.join(&link.rel);
                if let Some(parent) = target.parent() {
                    crate::file::create_dir_all(parent)?;
                }
                crate::file::make_symlink(&link.dest, &target)?;
            }
            for dir in dirs.iter().rev() {
                set_mode(&path.join(&dir.rel), dir.mode)?;
            }
            set_mode(path, *mode)?;
        }
        PathSnapshot::Unrecorded { reason, .. } => bail!("no usable preimage: {reason}"),
    }
    Ok(())
}

/// Delete only this completed operation's sidecars, preserving any still
/// referenced by an unresolved operation. Caller holds the operation lock.
pub(crate) fn discard(state_dir: &Path, journal: &[JournalEntry]) -> Result<()> {
    discard_except(state_dir, journal, None)
}

/// The selected pending record remains durable until its cleanup succeeds.
pub(crate) fn discard_pending(
    state_dir: &Path,
    journal: &[JournalEntry],
    uuid: &str,
) -> Result<()> {
    discard_except(state_dir, journal, Some(uuid))
}

fn discard_except(
    state_dir: &Path,
    journal: &[JournalEntry],
    completed: Option<&str>,
) -> Result<()> {
    let mut candidates = sidecars(journal);
    for (_, pending) in super::store::list_pending_in(state_dir)? {
        if completed == Some(pending.checkpoint.uuid.as_str()) {
            continue;
        }
        if let Some(operation) = pending.checkpoint.operation {
            for retained in sidecars(&operation.journal) {
                candidates.remove(&retained);
            }
        }
    }
    for hash in candidates {
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("invalid recovery content identifier");
        }
        let path = super::journal::blobs_dir_in(state_dir).join(hash);
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn sidecars(journal: &[JournalEntry]) -> BTreeSet<String> {
    let mut hashes = BTreeSet::new();
    for entry in journal {
        if let JournalEntry::PathChanged { prior, .. } = entry {
            let blobs: Vec<_> = match prior {
                PathSnapshot::File { content, .. } => vec![content],
                PathSnapshot::Dir { files, .. } => files.iter().map(|file| &file.content).collect(),
                _ => vec![],
            };
            hashes.extend(
                blobs
                    .into_iter()
                    .filter(|blob| blob.inline.is_none())
                    .map(|blob| blob.sha256.clone()),
            );
        }
    }
    hashes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &Path, prior: PathSnapshot) -> Vec<JournalEntry> {
        vec![
            JournalEntry::PathChanged {
                part: "test".into(),
                item: "write".into(),
                path: path.into(),
                prior,
            },
            JournalEntry::Committed {
                seq: 0,
                after: PathState::observe(path),
            },
        ]
    }

    #[test]
    fn restore_private_preimage_and_discard_completed_sidecar() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temp.path())?;
        let state = root.join("state");
        let path = root.join("untracked");
        let bytes = vec![42; super::super::journal::BLOB_INLINE_MAX as usize + 1];
        std::fs::write(&path, &bytes)?;
        let prior = PathSnapshot::capture_with(&state, &path, Capture::Full);
        std::fs::write(&path, b"operation result")?;
        let journal = change(&path, prior);
        let hashes = sidecars(&journal);
        assert_eq!(hashes.len(), 1);
        recover(&state, &journal)?;
        assert_eq!(std::fs::read(&path)?, bytes);
        recover(&state, &journal)?; // idempotent after safe recovery
        discard(&state, &journal)?;
        for hash in hashes {
            assert!(
                !super::super::journal::blobs_dir_in(&state)
                    .join(hash)
                    .exists()
            );
        }
        assert!(!state.join("history/repo.git").exists());
        Ok(())
    }

    #[test]
    fn concurrent_edit_and_missing_completion_are_preserved() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temp.path())?;
        let path = root.join("config");
        std::fs::write(&path, b"before")?;
        let prior = PathSnapshot::capture_with(&root, &path, Capture::Full);
        std::fs::write(&path, b"operation result")?;
        let journal = change(&path, prior);
        std::fs::write(&path, b"concurrent edit")?;
        assert!(
            recover(&root, &journal)
                .unwrap_err()
                .to_string()
                .contains("left untouched")
        );
        assert_eq!(std::fs::read(&path)?, b"concurrent edit");
        assert!(
            recover(&root, &journal[..1])
                .unwrap_err()
                .to_string()
                .contains("completion was not recorded")
        );
        assert_eq!(std::fs::read(&path)?, b"concurrent edit");
        Ok(())
    }

    #[test]
    fn replaced_directory_recovers_empty_descendants_and_contents() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temp.path())?;
        let path = root.join("config");
        std::fs::create_dir_all(path.join("empty"))?;
        std::fs::write(path.join("file"), b"before")?;
        let prior = PathSnapshot::capture_with(&root, &path, Capture::Full);
        std::fs::remove_file(path.join("file"))?;
        std::fs::remove_dir(path.join("empty"))?;
        std::fs::remove_dir(&path)?;
        std::fs::write(&path, b"replacement")?;
        let journal = change(&path, prior);
        recover(&root, &journal)?;
        assert_eq!(std::fs::read(path.join("file"))?, b"before");
        assert!(path.join("empty").is_dir());
        recover(&root, &journal)?;
        Ok(())
    }

    #[test]
    fn corrupt_preimage_does_not_replace_current_file() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temp.path())?;
        let path = root.join("config");
        std::fs::write(&path, b"current")?;
        let prior = PathSnapshot::File {
            content: Blob {
                sha256: "0".repeat(64),
                size: 1,
                inline: Some("YQ==".into()),
            },
            mode: 0o600,
        };
        assert!(recover(&root, &change(&path, prior)).is_err());
        assert_eq!(std::fs::read(&path)?, b"current");
        Ok(())
    }

    #[test]
    fn ordinary_failure_keeps_completed_writes_but_retains_incomplete_ones() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temp.path())?;
        let path = root.join("config");
        std::fs::write(&path, b"before")?;
        let prior = PathSnapshot::capture_with(&root, &path, Capture::Full);
        std::fs::write(&path, b"completed phase")?;
        let journal = change(&path, prior);
        recover_unfinished(&root, &journal)?;
        assert_eq!(std::fs::read(&path)?, b"completed phase");
        assert!(recover_unfinished(&root, &journal[..1]).is_err());
        assert_eq!(std::fs::read(&path)?, b"completed phase");
        Ok(())
    }

    #[test]
    fn unreadable_pending_records_prevent_sidecar_discard() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temp.path())?;
        let path = root.join("config");
        std::fs::write(
            &path,
            vec![42; super::super::journal::BLOB_INLINE_MAX as usize + 1],
        )?;
        let prior = PathSnapshot::capture_with(&root, &path, Capture::Full);
        let journal = change(&path, prior);
        let directory = super::super::store::pending_dir_in(&root);
        std::fs::create_dir_all(&directory)?;
        let broken = directory.join("unfinished.json");
        std::fs::write(&broken, b"{broken")?;
        assert!(discard(&root, &journal).is_err());
        assert_eq!(std::fs::read(&broken)?, b"{broken");
        assert!(!directory.join("unfinished.json.broken").exists());
        for hash in sidecars(&journal) {
            assert!(
                super::super::journal::blobs_dir_in(&root)
                    .join(hash)
                    .exists()
            );
        }
        Ok(())
    }

    #[test]
    fn a_failed_later_write_blocks_earlier_recovery_of_the_same_path() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temp.path())?;
        let path = root.join("config");
        std::fs::write(&path, b"original")?;
        let original = PathSnapshot::capture_with(&root, &path, Capture::Full);
        std::fs::write(&path, b"first result")?;
        let mut journal = change(&path, original);
        std::fs::write(&path, b"between writes")?;
        let intermediate = PathSnapshot::capture_with(&root, &path, Capture::Full);
        std::fs::write(&path, b"second result")?;
        journal.push(JournalEntry::PathChanged {
            part: "test".into(),
            item: "second".into(),
            path: path.clone(),
            prior: intermediate,
        });
        journal.push(JournalEntry::Committed {
            seq: 2,
            after: PathState::observe(&path),
        });
        // A concurrent edit returned to an earlier result. Once the later
        // step detects divergence, the earlier step must not overwrite it.
        std::fs::write(&path, b"first result")?;
        assert!(recover(&root, &journal).is_err());
        assert_eq!(std::fs::read(&path)?, b"first result");
        Ok(())
    }
}
