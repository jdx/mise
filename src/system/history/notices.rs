//! Things a background save had to say, kept until a person is there to
//! hear them.
//!
//! **A warning the watcher writes to its own log is a warning the user
//! never sees.** Some of what a capture notices happens once and cannot
//! be noticed again — narrowing an `include` list drops paths from every
//! checkpoint after the one that applies it, and the checkpoint that
//! applies it is the last one whose parent still holds them. When the
//! watcher gets there first, the one opportunity to say so is spent on a
//! log file. So it is written down instead, and the next `mise dot`
//! command says it before doing what it was asked.

use eyre::Result;
use std::path::PathBuf;

fn path() -> PathBuf {
    super::store::store_dir_in(&super::store::state_dir()).join("notices")
}

/// Keeps `message` for the next command a person runs.
pub(crate) fn record(message: &str) -> Result<()> {
    record_in(&path(), message)
}

fn record_in(path: &std::path::Path, message: &str) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lock = guard(path)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    // one line each, so a partial write loses at most the last notice
    writeln!(file, "{}", message.replace('\n', " "))?;
    Ok(())
}

/// Serializes writing and taking.
///
/// **Nothing may be lost between the read and the removal.** The writer
/// is the watcher — exactly the thing this exists to catch up with — so
/// a notice appended in that window would be deleted without ever being
/// said, which is the failure this module was built to prevent. Renaming
/// the file first is not enough on its own: an append opened before the
/// rename still writes into the same file, now the one being read and
/// deleted.
///
/// The lock is held around two file operations and nothing else. It
/// never spans anything that runs user code, which is what makes it
/// safe to hold across processes.
fn guard(path: &std::path::Path) -> Result<fslock::LockFile> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::lock_file::LockFile::at(&path.with_extension("lock")).lock()
}

/// Says everything kept, and keeps it no longer.
///
/// Best effort by design: a notice that cannot be read is not worth
/// failing the command the user actually asked for, and one said twice
/// is better than one never said.
pub(crate) fn drain() {
    for line in take(&path()) {
        warn!("{line}");
    }
}

/// The kept notices, read and cleared as one step under [`guard`].
fn take(path: &std::path::Path) -> Vec<String> {
    let Ok(_lock) = guard(path) else {
        return vec![];
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let _ = std::fs::remove_file(path);
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is recorded waits, is said in the order it happened, and is
    /// said once.
    #[test]
    fn a_notice_is_kept_until_it_is_said_and_then_is_gone() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state/notices");
        assert!(take(&path).is_empty(), "nothing recorded, nothing to say");

        record_in(&path, "first").unwrap();
        record_in(&path, "second\nwith a newline in it").unwrap();
        assert_eq!(
            take(&path),
            vec![
                "first".to_string(),
                "second with a newline in it".to_string()
            ]
        );
        // taken once: the next command is not told again
        assert!(take(&path).is_empty());

        // and one recorded immediately after a take is not swallowed by
        // it: the take claimed a file, not the name
        record_in(&path, "third").unwrap();
        assert_eq!(take(&path), vec!["third".to_string()]);
    }

    /// The writer is the watcher, and it does not stop while someone
    /// reads. Nothing it wrote may go missing.
    #[test]
    fn nothing_recorded_during_a_drain_is_lost() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state/notices");
        const COUNT: usize = 200;

        let writer = {
            let path = path.clone();
            std::thread::spawn(move || {
                for i in 0..COUNT {
                    record_in(&path, &format!("notice {i}")).unwrap();
                }
            })
        };

        // bounded, so a lost notice fails the test instead of hanging it
        let mut said = vec![];
        let start = std::time::Instant::now();
        while said.len() < COUNT && start.elapsed() < std::time::Duration::from_secs(30) {
            said.extend(take(&path));
        }
        writer.join().unwrap();
        said.extend(take(&path));

        said.sort();
        let mut expected: Vec<String> = (0..COUNT).map(|i| format!("notice {i}")).collect();
        expected.sort();
        assert_eq!(said, expected, "a notice was lost between the two");
    }
}
