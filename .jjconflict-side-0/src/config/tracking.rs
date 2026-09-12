use std::fs;
use std::fs::{read_dir, remove_file};
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::dirs::{TRACKED_CONFIGS, TRACKED_STUBS};
use crate::file::{create_dir_all, make_symlink_or_file};
use crate::hash::hash_to_str;

pub(crate) struct Tracker {}

impl Tracker {
    pub(crate) fn track(path: &Path) -> Result<()> {
        Self::track_in(&TRACKED_CONFIGS, path)
    }

    pub(crate) fn track_stub(path: &Path) -> Result<()> {
        Self::track_in(&TRACKED_STUBS, path)
    }

    fn track_in(dir: &Path, path: &Path) -> Result<()> {
        let tracking_path = dir.join(hash_to_str(&path));
        if !tracking_path.exists() {
            create_dir_all(dir)?;
            make_symlink_or_file(path, &tracking_path)?;
        }
        Ok(())
    }

    pub(crate) fn list_all() -> Result<Vec<PathBuf>> {
        Self::list_all_in(&TRACKED_CONFIGS)
    }

    pub(crate) fn list_all_stubs() -> Result<Vec<PathBuf>> {
        Self::list_all_in(&TRACKED_STUBS)
    }

    fn list_all_in(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut output = vec![];
        if !dir.exists() {
            return Ok(output);
        }
        for entry in read_dir(dir)? {
            let Some(path) = Self::tracked_path(&entry?.path())? else {
                continue;
            };
            // Only a regular file can be one of these. A tracked path that is a
            // device or a directory is left-over state that every reader would
            // fail on, so it is not handed out (#12246).
            if path.is_file() {
                output.push(path);
            }
        }
        Ok(output)
    }

    /// The path an entry records, or `None` when it is not one mise wrote.
    ///
    /// Entries are symlinks on unix and, since Windows symlinks need a
    /// privilege mise does not require, plain files holding the path there —
    /// see `file::make_symlink_or_file`. Both forms have to be resolved before
    /// anything can be decided about what they point at.
    fn tracked_path(entry: &Path) -> Result<Option<PathBuf>> {
        if entry.is_symlink() {
            Ok(Some(fs::read_link(entry)?))
        } else if cfg!(target_os = "windows") {
            Ok(Some(PathBuf::from(fs::read_to_string(entry)?.trim())))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn clean() -> Result<()> {
        Self::clean_in(&TRACKED_CONFIGS)?;
        Self::clean_in(&TRACKED_STUBS)
    }

    fn clean_in(dir: &Path) -> Result<()> {
        if dir.is_dir() {
            for entry in read_dir(dir)? {
                let entry = entry?.path();
                // Resolve first. Asking whether the *entry* exists answers the
                // wrong question: a symlink to `/dev/null` exists, so it
                // survived every prune while every reader kept failing on it,
                // and on Windows the entry is a plain file that always exists
                // no matter what it records.
                let keep = match Self::tracked_path(&entry)? {
                    Some(path) => path.is_file(),
                    None => false,
                };
                if !keep {
                    remove_file(&entry)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `track_in` writes a symlink on unix and a plain file on Windows, so going
    /// through it keeps these tests meaningful on both.
    fn track(dir: &Path, target: &Path) -> PathBuf {
        Tracker::track_in(dir, target).unwrap();
        dir.join(hash_to_str(&target))
    }

    #[test]
    fn keeps_an_entry_pointing_at_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let config = tmp.path().join("mise.toml");
        fs::write(&config, "[tools]\n").unwrap();

        let entry = track(&dir, &config);
        Tracker::clean_in(&dir).unwrap();

        assert!(entry.exists(), "a real config file must survive a clean");
        assert_eq!(Tracker::list_all_in(&dir).unwrap(), vec![config]);
    }

    #[test]
    fn removes_an_entry_pointing_at_something_that_is_not_a_file() {
        // Stands in for the `/dev/null` entries older versions left behind: the
        // target exists, so asking `entry.exists()` kept them forever while
        // every reader failed on them. A directory reproduces that portably.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let not_a_config = tmp.path().join("somewhere");
        fs::create_dir(&not_a_config).unwrap();

        let entry = track(&dir, &not_a_config);
        assert!(Tracker::list_all_in(&dir).unwrap().is_empty());

        Tracker::clean_in(&dir).unwrap();
        assert!(
            fs::symlink_metadata(&entry).is_err(),
            "the tracking entry must be removed"
        );
    }

    #[test]
    fn removes_an_entry_whose_target_is_gone() {
        // The entry is checked with `symlink_metadata` rather than `exists`,
        // which follows the link: a dangling symlink reports `false` whether or
        // not the clean removed it, so `exists` would pass on unix regardless.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let config = tmp.path().join("mise.toml");
        fs::write(&config, "[tools]\n").unwrap();

        let entry = track(&dir, &config);
        fs::remove_file(&config).unwrap();

        Tracker::clean_in(&dir).unwrap();
        assert!(
            fs::symlink_metadata(&entry).is_err(),
            "the tracking entry must be removed"
        );
    }
}
