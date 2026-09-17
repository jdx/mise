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
    /// Only a missing directory qualifies. A root that exists but declares no
    /// daemons any more keeps its state: `prepare()` already unregisters its
    /// configuration, and its data may still be wanted.
    pub(crate) fn orphaned(&self) -> bool {
        !self.state.root.as_os_str().is_empty() && !self.state.root.is_dir()
    }
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
/// `runtime` is `None` when pitchfork cannot be located; the directory is still
/// removed because nothing can be running for a project whose supervisor
/// binary is gone from every PATH mise knows about.
///
/// Returns false when another mise process holds the project lock, which leaves
/// the state in place for a later run.
pub(crate) async fn remove(entry: &Entry, runtime: Option<&Runtime>) -> Result<bool> {
    // The root is gone, so pitchfork runs from the state directory instead.
    let cwd = &entry.dir;
    let Some(lock) = crate::lock_file::LockFile::at(&cwd.join("project.lock")).try_lock()? else {
        warn!(
            "skipping {}: another mise process holds its daemon lock",
            display_path(cwd)
        );
        return Ok(false);
    };
    if let Some(runtime) = runtime {
        match runtime.supervisor_up(cwd).await {
            Ok(true) if !entry.state.ids.is_empty() => {
                let mut args = vec!["stop".to_string()];
                args.extend(entry.state.ids.iter().cloned());
                if let Err(err) = runtime.output(cwd, &args).await {
                    debug!("{err:#}");
                }
            }
            Ok(_) => {}
            Err(err) => debug!("{err:#}"),
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
            warn!("{err:#}");
        }
    } else {
        warn!(
            "pitchfork not found; {} may remain registered until it is removed with `pitchfork config remove`",
            display_path(entry.config_file())
        );
    }
    crate::file::remove_all(cwd)?;
    drop(lock);
    Ok(true)
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
    async fn remove_without_pitchfork_deletes_state_and_respects_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("daemons");
        let gone = tmp.path().join("gone");
        let dir = write_state(&base, "gone", &gone, &[("db/one", 1)]);
        let entry = orphans(&base).unwrap().remove(0);

        let held = crate::lock_file::LockFile::at(&dir.join("project.lock"))
            .try_lock()
            .unwrap()
            .unwrap();
        assert!(!remove(&entry, None).await.unwrap());
        assert!(dir.join("state.json").exists(), "locked state must survive");
        drop(held);

        assert!(remove(&entry, None).await.unwrap());
        assert!(!dir.exists());
    }
}
