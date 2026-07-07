use std::fs;
use std::fs::{read_dir, remove_file};
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::dirs::{TRACKED_CONFIGS, TRACKED_STUBS};
use crate::file::{create_dir_all, make_symlink_or_file};
use crate::hash::hash_to_str;

pub struct Tracker {}

impl Tracker {
    pub fn track(path: &Path) -> Result<()> {
        Self::track_in(&TRACKED_CONFIGS, path)
    }

    pub fn track_stub(path: &Path) -> Result<()> {
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

    pub fn list_all() -> Result<Vec<PathBuf>> {
        Self::list_all_in(&TRACKED_CONFIGS)
    }

    pub fn list_all_stubs() -> Result<Vec<PathBuf>> {
        Self::list_all_in(&TRACKED_STUBS)
    }

    fn list_all_in(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut output = vec![];
        if !dir.exists() {
            return Ok(output);
        }
        for path in read_dir(dir)? {
            let mut path = path?.path();
            if path.is_symlink() {
                path = fs::read_link(path)?;
            } else if cfg!(target_os = "windows") {
                path = PathBuf::from(fs::read_to_string(&path)?.trim());
            } else {
                continue;
            }
            if path.exists() {
                output.push(path);
            }
        }
        Ok(output)
    }

    pub fn clean() -> Result<()> {
        Self::clean_in(&TRACKED_CONFIGS)?;
        Self::clean_in(&TRACKED_STUBS)
    }

    fn clean_in(dir: &Path) -> Result<()> {
        if dir.is_dir() {
            for path in read_dir(dir)? {
                let path = path?.path();
                if !path.exists() {
                    remove_file(&path)?;
                }
            }
        }
        Ok(())
    }
}
