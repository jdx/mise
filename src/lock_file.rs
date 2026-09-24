use std::path::{Path, PathBuf};

use eyre::Result;

use crate::dirs;
use crate::file::{create_dir_all, display_path};
use crate::hash::hash_to_str;

pub(crate) type OnLockedFn = Box<dyn Fn(&Path)>;

pub(crate) struct LockFile {
    path: PathBuf,
    on_locked: Option<OnLockedFn>,
    record_pid: bool,
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
            record_pid: false,
        }
    }

    pub(crate) fn with_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(&Path) + 'static,
    {
        self.on_locked = Some(Box::new(cb));
        self
    }

    /// Writes the holder's PID into the lock file while it is held, so a
    /// waiter can say which process it is waiting on. Only for lock files
    /// whose contents nothing else reads; unlocking truncates the file.
    pub(crate) fn with_pid(mut self) -> Self {
        self.record_pid = true;
        self
    }

    pub(crate) fn lock(self) -> Result<fslock::LockFile> {
        self.lock_with_notice(&|_| {})
    }

    /// Like [`Self::lock`], but also runs a borrowed `on_wait` when the lock is
    /// contended. For callers whose progress reporter cannot move into the
    /// `'static` callback but should still say why the install is paused.
    /// `on_wait` receives the holder's PID when the lock was taken
    /// [`Self::with_pid`] and the holder is still recorded.
    pub(crate) fn lock_with_notice(
        self,
        on_wait: &dyn Fn(Option<u32>),
    ) -> Result<fslock::LockFile> {
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        let mut lock = fslock::LockFile::open(&self.path)?;
        if !self.try_acquire(&mut lock)? {
            if let Some(f) = &self.on_locked {
                f(&self.path)
            }
            on_wait(self.holder_pid());
            if self.record_pid {
                lock.lock_with_pid()?;
            } else {
                lock.lock()?;
            }
        }
        Ok(lock)
    }

    pub(crate) fn try_lock(self) -> Result<Option<fslock::LockFile>> {
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        let mut lock = fslock::LockFile::open(&self.path)?;
        if self.try_acquire(&mut lock)? {
            Ok(Some(lock))
        } else {
            Ok(None)
        }
    }

    fn try_acquire(&self, lock: &mut fslock::LockFile) -> Result<bool> {
        Ok(if self.record_pid {
            lock.try_lock_with_pid()?
        } else {
            lock.try_lock()?
        })
    }

    /// The PID the current holder recorded, if any. Best effort: the holder
    /// may release (and truncate) the file between our failed attempt and this
    /// read, and on Windows the locked file cannot be read at all.
    fn holder_pid(&self) -> Option<u32> {
        if !self.record_pid {
            return None;
        }
        let contents = std::fs::read_to_string(&self.path).ok()?;
        contents.lines().next()?.trim().parse().ok()
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

    #[cfg(unix)]
    #[test]
    fn waiter_learns_the_recorded_holder_pid() {
        use std::sync::mpsc;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("install.lock");
        let held = LockFile::at(&path).with_pid().try_lock().unwrap().unwrap();
        let (tx, rx) = mpsc::channel();
        let waiter_path = path.clone();
        let waiter = std::thread::spawn(move || {
            let lock = LockFile::at(&waiter_path)
                .with_pid()
                .lock_with_notice(&|pid| tx.send(pid).unwrap())
                .unwrap();
            drop(lock);
        });
        assert_eq!(rx.recv().unwrap(), Some(std::process::id()));
        drop(held);
        waiter.join().unwrap();
        // Unlocking erases the PID so a later waiter never names a stale holder.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }
}
