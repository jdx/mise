//! Removal of daemon state left behind by deleted project roots.
//!
//! Every project root (including each linked git worktree) owns one directory
//! under `$MISE_STATE_DIR/daemons/`. Removing the root leaves that directory,
//! its registered pitchfork configuration, and any database data behind.
//! Selection here is pure filesystem inspection so it can be tested without
//! pitchfork; the pitchfork calls live in [`remove`].

use super::runtime::{Runtime, State};
use crate::file::display_path;
use eyre::Result;
use std::path::{Path, PathBuf};

/// One `<state-dir>/state.json` and the directory that contains it.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub dir: PathBuf,
    pub state: State,
}

impl Entry {
    pub(crate) fn config_file(&self) -> PathBuf {
        self.dir.join("pitchfork.toml")
    }

    /// Whether the project this state belongs to no longer exists on disk.
    ///
    /// Only a filesystem `NotFound` qualifies. `Path::is_dir` would answer
    /// false for every metadata error, so an unplugged volume, an NFS share
    /// that is down, or a directory mise cannot stat would read as a deleted
    /// project and cost the user a database. Anything that is not a definite
    /// absence keeps its state, as does a root that still exists but declares
    /// no daemons any more: `prepare()` already unregisters its configuration,
    /// and its data may still be wanted.
    pub(crate) fn orphaned(&self) -> bool {
        if self.state.root.as_os_str().is_empty() {
            return false;
        }
        match std::fs::symlink_metadata(&self.state.root) {
            Ok(_) => false,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                warn!(
                    "keeping {}: cannot read {}: {err}",
                    display_path(&self.dir),
                    display_path(&self.state.root)
                );
                false
            }
        }
    }
}

/// What [`remove`] did with one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The state directory and its data are gone.
    Removed,
    /// Left in place. A warning says why, and a later run can retry.
    Kept,
}

/// The directory holding every project's daemon state.
pub(crate) fn base_dir() -> PathBuf {
    crate::dirs::STATE.join("daemons")
}

/// Every readable `state.json` directly below `base`, in path order.
///
/// Directories without a state file (or with an unparsable one) are skipped:
/// they were never fully prepared, and guessing at them could delete data mise
/// does not understand.
pub(crate) fn scan(base: &Path) -> Result<Vec<Entry>> {
    let Ok(read_dir) = std::fs::read_dir(base) else {
        return Ok(vec![]);
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        let dir = entry?.path();
        let path = dir.join("state.json");
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match serde_json::from_slice::<State>(&bytes) {
            Ok(state) => entries.push(Entry { dir, state }),
            Err(err) => debug!("ignoring {}: {err}", display_path(&path)),
        }
    }
    entries.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(entries)
}

/// Entries whose project root has been deleted.
pub(crate) fn orphans(base: &Path) -> Result<Vec<Entry>> {
    Ok(scan(base)?.into_iter().filter(Entry::orphaned).collect())
}

/// Total size in bytes of everything below `path`; `0` when it does not exist.
/// Symlinks are not followed, so a linked data directory counts once.
pub(crate) fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

pub(crate) fn human_size(bytes: u64) -> String {
    bytesize::ByteSize::b(bytes).display().iec().to_string()
}

/// One line per orphan for the confirmation prompt and dry-run output.
pub(crate) fn describe(entries: &[(Entry, u64)]) -> Vec<String> {
    entries
        .iter()
        .map(|(entry, size)| {
            format!(
                "{} ({}) from deleted {}",
                display_path(&entry.dir),
                human_size(*size),
                display_path(&entry.state.root)
            )
        })
        .collect()
}

/// Stops the entry's daemons, unregisters its generated configuration, and
/// deletes the state directory including data.
///
/// Every step that cannot be confirmed keeps the state. A pitchfork call that
/// times out or errors may leave a daemon alive on top of the very data this
/// would delete, so the entry is left for a later run rather than deleted on an
/// unverified assumption. The same applies when pitchfork cannot be located at
/// all (`runtime` is `None`): deleting the directory would take `state.json`
/// and the generated configuration with it, which is the only record of the
/// registration a later run could act on.
pub(crate) async fn remove(entry: &Entry, runtime: Option<&Runtime>) -> Result<Outcome> {
    // The root is gone, so pitchfork runs from the state directory instead.
    let cwd = &entry.dir;
    let Some(lock) = crate::lock_file::LockFile::at(&cwd.join("project.lock")).try_lock()? else {
        warn!(
            "keeping {}: another mise process holds its daemon lock",
            display_path(cwd)
        );
        return Ok(Outcome::Kept);
    };
    // Selection and confirmation both happen before this lock is held, and a
    // project directory can come back in between (a restored worktree, a
    // re-clone). Ask again now that nothing else can prepare this state.
    if !entry.orphaned() {
        warn!(
            "keeping {}: {} exists again",
            display_path(cwd),
            display_path(&entry.state.root)
        );
        return Ok(Outcome::Kept);
    }
    if let Some(runtime) = runtime {
        match runtime.supervisor_up(cwd).await {
            Ok(true) if !entry.state.ids.is_empty() => {
                let mut args = vec!["stop".to_string()];
                args.extend(entry.state.ids.iter().cloned());
                if let Err(err) = runtime.output(cwd, &args).await {
                    warn!(
                        "keeping {}: cannot stop its daemons: {err:#}",
                        display_path(cwd)
                    );
                    return Ok(Outcome::Kept);
                }
            }
            Ok(_) => {}
            Err(err) => {
                warn!(
                    "keeping {}: cannot establish supervisor status: {err:#}",
                    display_path(cwd)
                );
                return Ok(Outcome::Kept);
            }
        }
        let config = entry.config_file();
        if config.exists()
            && let Err(err) = runtime
                .output(
                    cwd,
                    &[
                        "config".into(),
                        "remove".into(),
                        config.to_string_lossy().into_owned(),
                    ],
                )
                .await
        {
            warn!(
                "keeping {}: cannot unregister {}: {err:#}",
                display_path(cwd),
                display_path(config)
            );
            return Ok(Outcome::Kept);
        }
    } else {
        warn!(
            "keeping {}: pitchfork is required to stop its daemons and unregister {}; run `mise use pitchfork`",
            display_path(cwd),
            display_path(entry.config_file())
        );
        return Ok(Outcome::Kept);
    }
    delete_locked_state_dir(cwd, lock)?;
    Ok(Outcome::Removed)
}

/// Deletes a state directory whose `project.lock` the caller holds.
///
/// The lock file lives inside the directory being deleted, and Windows refuses
/// to remove a file another handle still holds open. Emptying the directory
/// under the lock, releasing it, and only then dropping the directory keeps the
/// lock meaningful for as long as there is state left to protect.
fn delete_locked_state_dir(dir: &Path, lock: fslock::LockFile) -> Result<()> {
    remove_contents_except_lock(dir)?;
    drop(lock);
    crate::file::remove_all(dir)
}

/// Deletes everything in `dir` except the `project.lock` the caller holds.
fn remove_contents_except_lock(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.file_name().is_some_and(|name| name == "project.lock") {
            continue;
        }
        crate::file::remove_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_state(base: &Path, name: &str, root: &Path, data: &[(&str, usize)]) -> PathBuf {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let state = State {
            root: root.to_path_buf(),
            namespace: format!("{name}-ns"),
            ids: vec![format!("{name}-ns/db")],
            ..State::default()
        };
        std::fs::write(
            dir.join("state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("pitchfork.toml"), "[daemons]\n").unwrap();
        for (file, size) in data {
            let path = dir.join("data").join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![b'x'; *size]).unwrap();
        }
        dir
    }

    #[test]
    fn selects_only_states_whose_root_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let live_root = tmp.path().join("live");
        std::fs::create_dir(&live_root).unwrap();
        let deleted_root = tmp.path().join("deleted");
        std::fs::create_dir(&deleted_root).unwrap();
        write_state(&base, "aaa-live", &live_root, &[("db/one", 10)]);
        let orphan = write_state(
            &base,
            "bbb-gone",
            &deleted_root,
            &[("db/one", 100), ("db/sub/two", 24)],
        );
        std::fs::remove_dir_all(&deleted_root).unwrap();
        // A directory that was never prepared has no state.json and is ignored.
        std::fs::create_dir_all(base.join("ccc-partial").join("data")).unwrap();
        std::fs::create_dir_all(base.join("ddd-corrupt")).unwrap();
        std::fs::write(base.join("ddd-corrupt/state.json"), "{").unwrap();

        let all = scan(&base).unwrap();
        assert_eq!(all.len(), 2);
        let orphans = orphans(&base).unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].dir, orphan);
        assert_eq!(orphans[0].state.root, deleted_root);
        assert_eq!(dir_size(&orphan.join("data")), 124);

        let lines = describe(&[(orphans[0].clone(), dir_size(&orphan))]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with(&display_path(&orphan)), "{}", lines[0]);
        assert!(lines[0].contains("from deleted"), "{}", lines[0]);
        assert!(
            lines[0].ends_with(&display_path(&deleted_root)),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn missing_base_and_live_roots_yield_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(orphans(&tmp.path().join("none")).unwrap().is_empty());
        let base = tmp.path().join("daemons");
        write_state(&base, "live", tmp.path(), &[]);
        assert!(orphans(&base).unwrap().is_empty());
        assert_eq!(dir_size(&tmp.path().join("nothing")), 0);
    }

    #[tokio::test]
    async fn state_survives_a_held_lock_and_a_missing_pitchfork() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let gone = tmp.path().join("gone");
        let dir = write_state(&base, "gone", &gone, &[("db/one", 1)]);
        let entry = orphans(&base).unwrap().remove(0);

        let held = crate::lock_file::LockFile::at(&dir.join("project.lock"))
            .try_lock()
            .unwrap()
            .unwrap();
        assert_eq!(remove(&entry, None).await.unwrap(), Outcome::Kept);
        assert!(dir.join("state.json").exists(), "locked state must survive");
        drop(held);

        // Without pitchfork nothing can be stopped or unregistered, and
        // deleting state.json would destroy the record a later run needs.
        assert_eq!(remove(&entry, None).await.unwrap(), Outcome::Kept);
        assert!(dir.join("state.json").exists());
    }

    #[test]
    fn deleting_state_releases_its_lock_before_removing_the_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let dir = write_state(&base, "gone", &tmp.path().join("gone"), &[("db/one", 1)]);
        let lock = crate::lock_file::LockFile::at(&dir.join("project.lock"))
            .try_lock()
            .unwrap()
            .unwrap();
        delete_locked_state_dir(&dir, lock).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn a_root_that_cannot_be_confirmed_missing_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        // A path that exists but is not a directory: `is_dir()` answers false
        // for it exactly as it does for an unreadable one, and neither is the
        // definite absence that makes state safe to delete.
        let file_root = tmp.path().join("root-is-a-file");
        std::fs::write(&file_root, "").unwrap();
        write_state(&base, "file-root", &file_root, &[]);
        assert!(orphans(&base).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_root_that_reappears_before_the_lock_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let root = tmp.path().join("restored");
        let dir = write_state(&base, "restored", &root, &[]);
        let entry = orphans(&base).unwrap().remove(0);

        // Selection and the confirmation prompt both ran while the project was
        // gone; a restored worktree between then and now must not be deleted.
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(remove(&entry, None).await.unwrap(), Outcome::Kept);
        assert!(dir.join("state.json").exists());
    }
}
