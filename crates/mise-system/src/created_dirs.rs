//! The directories mise created beneath the target of an entry that walks a source directory
//! (`symlink-each`, or `copy` of a directory).
//!
//! Such a target is shared with files mise does not manage, so removing what the entry deployed
//! must not take a directory the user made, even an empty one. Entries that deploy a single file
//! record the directories they create in their own target record; a walking entry's are kept here,
//! in one record keyed by directory, because entries and groups can share them.
//!
//! A target is *tracked* once an apply has recorded what it creates. Removals under a tracked
//! target take only the directories recorded here, and only while each is still the directory mise
//! created: one the user deleted and made again at the same path is theirs. Under a target that is
//! not tracked, deployed by a mise from before this record existed and not applied since, any
//! directory the removal empties still goes, as it did then. A record that exists but cannot be
//! read is left as it is, and while it is there removals take no directory under any target and
//! applies write under no walking target.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::WrapErr;
use serde::{Deserialize, Serialize};

use mise_util::dirs;
use mise_util::file;
use mise_util::path::PathExt;

use crate::files::lexical_normalize;

const VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct CreatedDirs {
    version: u32,
    /// targets whose created directories are all recorded in `dirs`
    targets: BTreeSet<PathBuf>,
    /// directories mise created at or beneath a tracked target, each with its [`identity`]
    dirs: BTreeMap<PathBuf, String>,
    /// the record is there but could not be read, so which directories are mise's is unknown
    #[serde(skip)]
    unreadable: bool,
}

/// Whether `path` is known to be gone, rather than there or unreadable for now.
fn gone(path: &Path) -> bool {
    matches!(std::fs::symlink_metadata(path), Err(err) if err.kind() == std::io::ErrorKind::NotFound)
}

fn record_path() -> PathBuf {
    dirs::STATE.join("dotfiles").join("created_dirs.toml")
}

/// What tells the directory at `dir` from another made later at the same path: its device, inode
/// and birth time. `None` when `dir` is not a directory, or when the platform cannot tell its birth
/// time: an inode alone can be reused by a directory the user makes later, so such a directory is
/// never recorded, and mise leaves it behind rather than risk removing the user's.
fn identity(dir: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(dir).ok()?;
    if !meta.is_dir() || file::is_symlink_or_junction(dir) {
        return None;
    }
    let born = meta
        .created()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!("{}:{}:{born}", meta.dev(), meta.ino()))
    }
    #[cfg(not(unix))]
    Some(born.to_string())
}

impl CreatedDirs {
    /// The record, or an empty one when there is none. One that cannot be read, or that another
    /// version of mise wrote, is marked unreadable: it lets no removal take a directory, and it is
    /// not written over.
    pub(crate) fn load() -> Self {
        let path = record_path();
        // only a record that is not there is none; one that cannot be looked at, or a dangling
        // link, is unreadable
        if gone(&path) {
            return Self::default();
        }
        match file::read_to_string(&path)
            .and_then(|contents| toml::from_str::<Self>(&contents).map_err(Into::into))
        {
            Ok(record) if record.version == VERSION => record,
            Ok(_) => {
                warn!(
                    "files: ignoring dotfiles state {} of another version",
                    path.display_user()
                );
                Self::unreadable()
            }
            Err(err) => {
                warn!(
                    "files: failed to read dotfiles state {}: {err}",
                    path.display_user()
                );
                Self::unreadable()
            }
        }
    }

    /// An error when the record cannot be read, so nothing can be added to it.
    pub(crate) fn ensure_readable(&self) -> eyre::Result<()> {
        if self.unreadable {
            eyre::bail!(
                "files: {} cannot be read; fix or remove it",
                record_path().display_user()
            );
        }
        Ok(())
    }

    fn unreadable() -> Self {
        Self {
            unreadable: true,
            ..Self::default()
        }
    }

    /// Write the record, dropping directories that are gone or no longer the ones recorded. One that
    /// cannot be read now may still be there, so it is kept. Targets are kept even when they are gone:
    /// one is tracked before the apply that creates it, and no removal can tell that apart from a
    /// target it took. A tracked target that is gone costs only its line here.
    fn save(mut self) -> eyre::Result<()> {
        self.version = VERSION;
        self.dirs
            .retain(|dir, id| match std::fs::symlink_metadata(dir) {
                Ok(_) => identity(dir).as_deref() == Some(id.as_str()),
                Err(_) => !gone(dir),
            });
        let path = record_path();
        if self.targets.is_empty() && self.dirs.is_empty() {
            if path.exists() {
                file::remove_file(&path)?;
            }
            return Ok(());
        }
        file::create_dir_all(path.parent().expect("dotfiles state parent"))?;
        file::write_atomic(&path, toml::to_string_pretty(&self)?)
    }

    /// Whether removing files deployed under `target` may remove the emptied directory `dir`.
    pub(crate) fn may_remove(&self, target: &Path, dir: &Path) -> bool {
        if self.unreadable {
            return false;
        }
        if !self.tracks(target) {
            return true;
        }
        self.recorded(dir)
            .is_some_and(|id| identity(dir).as_deref() == Some(id))
    }

    /// Whether `target` is tracked.
    pub(crate) fn tracks(&self, target: &Path) -> bool {
        if self.unreadable {
            return true;
        }
        let target = lexical_normalize(target);
        self.targets.contains(&target)
            || self
                .targets
                .iter()
                .any(|tracked| file::same_location_ignoring_case(tracked, &target))
    }

    /// The identity recorded for `dir`, under whatever letter case the record spells it in where
    /// the directory ignores case.
    fn recorded(&self, dir: &Path) -> Option<&str> {
        let dir = lexical_normalize(dir);
        self.dirs
            .get(&dir)
            .or_else(|| {
                self.dirs
                    .iter()
                    .find(|(recorded, _)| file::same_location_ignoring_case(recorded, &dir))
                    .map(|(_, id)| id)
            })
            .map(String::as_str)
    }

    /// Record `dir` as mise's, as the directory that is there now. Whether that changed the record.
    fn insert(&mut self, dir: PathBuf) -> bool {
        // the record is TOML, which holds only UTF-8 paths; mise leaves such a directory behind
        if dir.to_str().is_none() {
            return false;
        }
        let Some(id) = identity(&dir) else {
            return false;
        };
        self.dirs.insert(dir, id.clone()) != Some(id)
    }
}

/// Load the record, let `change` modify it, and write it back when `change` says it changed it,
/// holding a lock so that two mise processes do not each overwrite what the other recorded. A
/// record that cannot be read is left as it is, and the update fails: what it would add is lost.
fn update(change: impl FnOnce(&mut CreatedDirs) -> bool) -> eyre::Result<()> {
    let path = record_path();
    let _lock = mise_util::lock_file::LockFile::new(&path).lock()?;
    let mut record = CreatedDirs::load();
    record.ensure_readable()?;
    if change(&mut record) {
        record.save()?;
    }
    Ok(())
}

/// [`update`] for a change whose loss only leaves directories behind later, so it warns.
fn update_or_warn(change: impl FnOnce(&mut CreatedDirs) -> bool) {
    if let Err(err) = update(change) {
        warn!(
            "files: failed to update dotfiles state {}: {err}",
            record_path().display_user()
        );
    }
}

/// Start tracking each target of `targets` before an apply writes under it, if it is not tracked
/// yet. What is already there predates the record, so it is taken as mise's only where it holds
/// something mise deployed: the target and the directories down to each of its deployed paths
/// that exists, as a removal took them before the record existed. A directory of the user's that
/// holds nothing mise deployed stays the user's.
///
/// Targets that start being tracked together are seeded alike, whichever is nested in which; only
/// a target tracked before has its own record, which seeding leaves alone.
///
/// An error means the targets are not tracked, so a later removal would take any directory it
/// empties there, including the user's: the apply must not write under them.
pub(crate) fn track(targets: Vec<(PathBuf, Vec<PathBuf>)>) -> eyre::Result<()> {
    let names = targets
        .iter()
        .map(|(target, _)| target.display_user().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    update(|record| {
        // what was tracked before this apply: beneath such a target, what mise created is already
        // recorded, and anything else there is the user's
        let before = CreatedDirs {
            targets: record.targets.clone(),
            ..CreatedDirs::default()
        };
        let mut changed = false;
        for (target, deployed) in targets {
            let target = lexical_normalize(&target);
            if before.tracks(&target) {
                continue;
            }
            if !target.ancestors().skip(1).any(|dir| before.tracks(dir)) {
                for path in deployed {
                    let path = lexical_normalize(&path);
                    if std::fs::symlink_metadata(&path).is_err() {
                        continue;
                    }
                    let dirs = path
                        .ancestors()
                        .skip(1)
                        .take_while(|dir| dir.starts_with(&target))
                        .collect::<Vec<_>>();
                    // a path in a target tracked before, beneath this one, is that target's to
                    // account for, and one reached through a link says nothing of whose the
                    // directories it shows are
                    if dirs.iter().any(|dir| {
                        (*dir != target && before.tracks(dir)) || file::is_symlink_or_junction(dir)
                    }) {
                        continue;
                    }
                    for dir in dirs {
                        record.insert(dir.to_path_buf());
                    }
                }
            }
            record.targets.insert(target);
            changed = true;
        }
        changed
    })
    .wrap_err_with(|| {
        format!("files: cannot record what mise creates under {names}, so it does not write there")
    })
}

/// Record the directories of `created` at or beneath `target` that an apply made.
pub(crate) fn record(target: &Path, created: &[PathBuf]) {
    let target = lexical_normalize(target);
    update_or_warn(|record| {
        let mut changed = false;
        for dir in created.iter().map(|dir| lexical_normalize(dir)) {
            if dir.starts_with(&target) {
                changed |= record.insert(dir);
            }
        }
        if !record.tracks(&target) {
            record.targets.insert(target);
            changed = true;
        }
        changed
    });
}

/// Drop `removed` from the record.
pub(crate) fn forget(removed: &[PathBuf]) {
    if removed.is_empty() {
        return;
    }
    update_or_warn(|record| {
        let before = record.dirs.len();
        for dir in removed {
            let dir = lexical_normalize(dir);
            record
                .dirs
                .retain(|recorded, _| !file::same_location_ignoring_case(recorded, &dir));
        }
        record.dirs.len() != before
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_record_removes_no_dir() {
        let record = CreatedDirs::unreadable();
        assert!(record.tracks(Path::new("/h/t")));
        assert!(!record.may_remove(Path::new("/h/t"), Path::new("/h/t/app")));
    }

    #[test]
    fn untracked_target_removes_any_emptied_dir() {
        let record = CreatedDirs::default();
        assert!(record.may_remove(Path::new("/h/t"), Path::new("/h/t/app")));
    }

    #[test]
    fn tracked_target_removes_only_recorded_dirs() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("t");
        let made = target.join("made");
        std::fs::create_dir_all(&made).unwrap();
        std::fs::create_dir_all(target.join("app")).unwrap();
        let mut record = CreatedDirs::default();
        record.targets.insert(target.clone());
        if identity(&made).is_none() {
            // a filesystem with no birth time records nothing, and removes nothing
            assert!(!record.insert(made.clone()));
            assert!(!record.may_remove(&target, &made));
            return;
        }
        assert!(record.insert(made.clone()));
        assert!(record.may_remove(&target, &made));
        assert!(record.may_remove(&target.join("."), &target.join("./made")));
        assert!(!record.may_remove(&target, &target.join("app")));
    }

    #[test]
    fn recreated_dir_is_not_mise_s() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("t");
        let made = target.join("made");
        std::fs::create_dir_all(&made).unwrap();
        let mut record = CreatedDirs::default();
        record.targets.insert(target.clone());
        if !record.insert(made.clone()) {
            // a filesystem with no birth time records nothing to compare
            return;
        }
        assert!(record.may_remove(&target, &made));
        // the user deletes it and makes it again, which may reuse its inode
        std::fs::remove_dir(&made).unwrap();
        std::fs::create_dir(&made).unwrap();
        assert!(!record.may_remove(&target, &made));
    }
}
