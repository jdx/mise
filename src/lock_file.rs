use crate::file::create_dir_all;
use std::path::{Path, PathBuf};

pub type OnLockedFn = Box<dyn Fn(&Path)>;

pub struct LockFile {
    path: PathBuf,
    on_locked: Option<OnLockedFn>,
}

impl LockFile {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.with_extension(".lock\0"),
            on_locked: None,
        }
    }

    pub fn with_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(&Path) + 'static,
    {
        self.on_locked = Some(Box::new(cb));
        self
    }

    pub fn lock(self) -> Result<fslock::LockFile, std::io::Error> {
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        let mut lock = fslock::LockFile::open(&self.path)?;
        if !lock.try_lock()? {
            if let Some(f) = self.on_locked {
                f(&self.path)
            }
            lock.lock()?;
        }
        Ok(lock)
    }
}
