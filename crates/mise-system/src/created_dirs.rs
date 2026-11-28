//! The directories mise created beneath the target of an entry that walks a source directory
//! (`symlink-each`, or `copy` of a directory).
//!
//! Such a target is shared with files mise does not manage, so removing what the entry deployed
//! must not take a directory the user made, even an empty one. Entries that deploy a single file
//! record the directories they create in their own target record; a walking entry's are kept here,
//! in one record keyed by directory, because entries and groups can share them.
//!
//! A target is *tracked* once an apply has recorded what it creates. Removals under a tracked
//! target take only the directories recorded here. Under a target that is not tracked, deployed by
//! a mise from before this record existed and not applied since, any directory the removal empties
//! still goes, as it did then.

use std::collections::BTreeSet;
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
    /// directories mise created at or beneath a tracked target
    dirs: BTreeSet<PathBuf>,
}

fn record_path() -> PathBuf {
    dirs::STATE.join("dotfiles").join("created_dirs.toml")
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
            .retain(|dir| dir.is_dir() && !file::is_symlink_or_junction(dir));
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
        !self.targets.contains(&lexical_normalize(target))
            || self.dirs.contains(&lexical_normalize(dir))
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
            record.dirs.insert(dir.to_path_buf());
        }
    }
    record.targets.insert(target);
    record.save();
}

/// Record the directories of `created` at or beneath `target` that an apply made.
pub(crate) fn record(target: &Path, created: &[PathBuf]) {
    let target = lexical_normalize(target);
    let mut record = CreatedDirs::load();
    let before = record.dirs.len();
    record.dirs.extend(
        created
            .iter()
            .map(|dir| lexical_normalize(dir))
            .filter(|dir| dir.starts_with(&target)),
    );
    if record.dirs.len() != before || !record.targets.contains(&target) {
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
        let mut record = CreatedDirs::default();
        record.targets.insert(PathBuf::from("/h/t"));
        record.dirs.insert(PathBuf::from("/h/t/made"));
        assert!(record.may_remove(Path::new("/h/t"), Path::new("/h/t/made")));
        assert!(record.may_remove(Path::new("/h/t/"), Path::new("/h/t/./made")));
        assert!(!record.may_remove(Path::new("/h/t"), Path::new("/h/t/app")));
    }
}
