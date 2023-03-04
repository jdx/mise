use std::collections::HashSet;
use std::fs;
use std::fs::{read_dir, remove_file};
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;

use crate::dirs;
use crate::file::{create_dir_all, make_symlink};
use crate::hash::hash_to_str;

#[derive(Debug, Default)]
pub struct Tracker {
    tracking_dir: PathBuf,
    config_files: HashSet<PathBuf>,
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            tracking_dir: dirs::ROOT.join("tracked_config_files"),
            ..Default::default()
        }
    }

    pub fn track(&mut self, path: &Path) -> Result<()> {
        if !self.config_files.contains(path) {
            let tracking_path = self.tracking_dir.join(hash_to_str(&path));
            if !tracking_path.exists() {
                create_dir_all(&self.tracking_dir)?;
                make_symlink(path, &tracking_path)?;
            }
            self.config_files.insert(path.to_path_buf());
        }
        Ok(())
    }

    pub fn list_all(&self) -> Result<Vec<PathBuf>> {
        self.clean()?;
        let mut output = vec![];
        for path in read_dir(&self.tracking_dir)? {
            let path = path?.path();
            let path = fs::read_link(path)?;
            if path.exists() {
                output.push(path);
            }
        }
        Ok(output)
    }

    pub fn clean(&self) -> Result<()> {
        for path in read_dir(&self.tracking_dir)? {
            let path = path?.path();
            if !path.exists() {
                remove_file(&path)?;
            }
        }
        Ok(())
    }
}
