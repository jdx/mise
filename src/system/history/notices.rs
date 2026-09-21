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
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    // one line each, so a partial write loses at most the last notice
    writeln!(file, "{}", message.replace('\n', " "))?;
    Ok(())
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

/// The kept notices, removed as they are taken.
fn take(path: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect();
    let _ = std::fs::remove_file(path);
    lines
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
    }
}
