use std::path::{Path, PathBuf};

use eyre::Result;

use crate::dirs;
use crate::file::{create_dir_all, display_path};
use crate::hash::hash_to_str;

pub(crate) type OnLockedFn = Box<dyn Fn(&Path)>;

pub(crate) struct LockFile {
    path: PathBuf,
    on_locked: Option<OnLockedFn>,
}

impl LockFile {
    pub(crate) fn new(path: &Path) -> Self {
        let path = dirs::CACHE.join("lockfiles").join(hash_to_str(&path));
        Self::at(&path)
    }

    /// Locks this exact path instead of hashing it into the cache directory.
    /// Use for shared state whose users may have different cache directories.
    /// The file must remain in place while any process could hold its lock.
    pub(crate) fn at(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            on_locked: None,
        }
    }

    pub(crate) fn with_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(&Path) + 'static,
    {
        self.on_locked = Some(Box::new(cb));
        self
    }

    pub(crate) fn lock(self) -> Result<fslock::LockFile> {
        self.lock_with_notice(&|| {})
    }

    /// Like [`Self::lock`], but also runs a borrowed `on_wait` when the lock is
    /// contended. For callers whose progress reporter cannot move into the
    /// `'static` callback but should still say why the install is paused.
    pub(crate) fn lock_with_notice(self, on_wait: &dyn Fn()) -> Result<fslock::LockFile> {
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        let mut lock = fslock::LockFile::open(&self.path)?;
        if !lock.try_lock()? {
            if let Some(f) = self.on_locked {
                f(&self.path)
            }
            on_wait();
            lock.lock()?;
        }
        Ok(lock)
    }

    pub(crate) fn try_lock(self) -> Result<Option<fslock::LockFile>> {
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        let mut lock = fslock::LockFile::open(&self.path)?;
        if lock.try_lock()? {
            Ok(Some(lock))
        } else {
            Ok(None)
        }
    }
}

pub(crate) fn get(path: &Path, force: bool) -> eyre::Result<Option<fslock::LockFile>> {
    let lock = if force {
        None
    } else {
        let lock = LockFile::new(path)
            .with_callback(|l| {
                debug!("waiting for lock on {}", display_path(l));
            })
            .lock()?;
        Some(lock)
    };
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_coordinates_with_direct_file_lock() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state/watch.lock");
        let held = LockFile::at(&path).try_lock().unwrap().unwrap();
        let mut direct = fslock::LockFile::open(&path).unwrap();
        assert!(!direct.try_lock().unwrap());
        assert!(LockFile::at(&path).try_lock().unwrap().is_none());

        // Unlocking keeps the file in place so existing descriptors and
        // future callers continue to coordinate on the same file.
        drop(held);
        assert!(path.exists());
        assert!(direct.try_lock().unwrap());
        assert!(LockFile::at(&path).try_lock().unwrap().is_none());
        drop(direct);
        assert!(LockFile::at(&path).try_lock().unwrap().is_some());
    }
}
