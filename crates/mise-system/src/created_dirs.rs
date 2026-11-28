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
//! created: one the user deleted and made again at the same path is theirs. Under a target that is not tracked, deployed by
//! a mise from before this record existed and not applied since, any directory the removal empties
//! still goes, as it did then.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
}

fn record_path() -> PathBuf {
    dirs::STATE.join("dotfiles").join("created_dirs.toml")
}

/// What tells the directory at `dir` from another made later at the same path: its device, inode
/// and birth time where the platform has them. `None` when `dir` is not a directory.
fn identity(dir: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(dir).ok()?;
    if !meta.is_dir() || file::is_symlink_or_junction(dir) {
        return None;
    }
    let born = meta
        .created()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(|| "-".to_string(), |since| since.as_nanos().to_string());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!("{}:{}:{born}", meta.dev(), meta.ino()))
    }
    #[cfg(not(unix))]
    Some(born)
}

impl CreatedDirs {
    /// The record, or an empty one when there is none. One that cannot be read tracks nothing,
    /// which leaves removals as they were before the record existed.
    pub(crate) fn load() -> Self {
        let path = record_path();
        if !path.exists() {
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
                Self::default()
            }
            Err(err) => {
                warn!(
                    "files: failed to read dotfiles state {}: {err}",
                    path.display_user()
                );
                Self::default()
            }
        }
    }

    /// Write the record, dropping directories and targets that no longer exist. A record that
    /// cannot be written only leaves directories behind later, so it warns.
    fn save(mut self) {
        self.version = VERSION;
        self.dirs
            .retain(|dir, id| identity(dir).as_deref() == Some(id.as_str()));
        self.targets.retain(|target| target.is_dir());
        let path = record_path();
        let result = if self.targets.is_empty() && self.dirs.is_empty() {
            if path.exists() {
                file::remove_file(&path)
            } else {
                Ok(())
            }
        } else {
            (|| -> eyre::Result<()> {
                file::create_dir_all(path.parent().expect("dotfiles state parent"))?;
                file::write_atomic(&path, toml::to_string_pretty(&self)?)
            })()
        };
        if let Err(err) = result {
            warn!(
                "files: failed to write dotfiles state {}: {err}",
                path.display_user()
            );
        }
    }

    /// Whether removing files deployed under `target` may remove the emptied directory `dir`.
    pub(crate) fn may_remove(&self, target: &Path, dir: &Path) -> bool {
        if !self.targets.contains(&lexical_normalize(target)) {
            return true;
        }
        self.dirs
            .get(&lexical_normalize(dir))
            .is_some_and(|id| identity(dir).as_deref() == Some(id.as_str()))
    }

    /// Whether `target` is tracked.
    pub(crate) fn tracks(&self, target: &Path) -> bool {
        self.targets.contains(&lexical_normalize(target))
    }

    /// Record `dir` as mise's, as the directory that is there now. Whether that changed the record.
    fn insert(&mut self, dir: PathBuf) -> bool {
        let Some(id) = identity(&dir) else {
            return false;
        };
        self.dirs.insert(dir, id.clone()) != Some(id)
    }
}

/// Start tracking `target` before an apply writes under it, if it is not tracked yet. What is
/// already there predates the record, so it is taken as mise's only where it holds something mise
/// deployed: `target` and the directories down to each path of `deployed` that exists, as a
/// removal took them before the record existed. A directory of the user's that holds nothing
/// mise deployed stays the user's.
pub(crate) fn track(target: &Path, deployed: impl IntoIterator<Item = PathBuf>) {
    let target = lexical_normalize(target);
    let mut record = CreatedDirs::load();
    if record.targets.contains(&target) {
        return;
    }
    for path in deployed {
        let path = lexical_normalize(&path);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        for dir in path.ancestors().skip(1) {
            if !dir.starts_with(&target) {
                break;
            }
            record.insert(dir.to_path_buf());
        }
    }
    record.targets.insert(target);
    record.save();
}

/// Record the directories of `created` at or beneath `target` that an apply made.
pub(crate) fn record(target: &Path, created: &[PathBuf]) {
    let target = lexical_normalize(target);
    let mut record = CreatedDirs::load();
    let mut changed = false;
    for dir in created.iter().map(|dir| lexical_normalize(dir)) {
        if dir.starts_with(&target) {
            changed |= record.insert(dir);
        }
    }
    if changed || !record.targets.contains(&target) {
        record.targets.insert(target);
        record.save();
    }
}

/// Drop `removed` from the record.
pub(crate) fn forget(removed: &[PathBuf]) {
    if removed.is_empty() {
        return;
    }
    let mut record = CreatedDirs::load();
    let before = record.dirs.len();
    for dir in removed {
        record.dirs.remove(&lexical_normalize(dir));
    }
    if record.dirs.len() != before {
        record.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        record.insert(made.clone());
        // the user deletes it and makes it again, which may reuse its inode
        std::fs::remove_dir(&made).unwrap();
        std::fs::create_dir(&made).unwrap();
        assert!(!record.may_remove(&target, &made));
    }
}
