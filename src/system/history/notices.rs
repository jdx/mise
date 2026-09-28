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
use std::path::{Path, PathBuf};

/// The notices file of the store kept under `state_dir`.
///
/// **Every operation here names the store it is for**, like the rest of
/// `store`'s `*_in` functions. A [`Store`](super::checkpoint::Store) is
/// opened on a state directory, and its notices belong to that one — so
/// a test that sandboxes a store with `Store::open_in(&tempdir)` does
/// not write to the machine's real history, and a command reading its
/// own store's notices cannot be handed another's.
fn file_in(state_dir: &Path) -> PathBuf {
    super::store::store_dir_in(state_dir).join("notices")
}

/// Says `message`, unless this process already said it.
///
/// **A standing condition is said once to the person in front of it.**
/// The same line can reach a person by more than one route in a single
/// command: a notice the watcher recorded is delivered on the way in,
/// and then the command's own walk of the same tree finds the same
/// condition and says it again. Both routes are wanted — neither knows
/// about the other, and either can be the only one — so the rule lives
/// here instead of in each of them.
pub(crate) fn say(message: &str) {
    let fresh = match said().lock() {
        Ok(mut said) => said.insert(message.to_string()),
        // a poisoned lock is not a reason to swallow a warning
        Err(_) => true,
    };
    if fresh {
        warn!("{message}");
    }
}

fn said() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SAID: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SAID.get_or_init(Default::default)
}

/// Drops kept notices that say exactly what was just said out loud, in
/// the store under `state_dir`.
///
/// **A warning is kept so it is not lost, not so it is said twice.** A
/// protective snapshot writes its warnings down because the operation
/// may fail before anything else can say them; when the operation does
/// reach a save that says them, the kept copies have served their
/// purpose and go. Lines nobody said are left alone.
pub(crate) fn forget_in(state_dir: &Path, said: &[String]) -> Result<()> {
    forget_from(&file_in(state_dir), said)
}

fn forget_from(path: &Path, said: &[String]) -> Result<()> {
    if !path.exists() || said.is_empty() {
        return Ok(());
    }
    // compared in the form `record_to` stores, or a multi-line warning
    // would never match the single line it was folded into
    let said: Vec<String> = said
        .iter()
        .map(|message| message.replace('\n', " "))
        .collect();
    let _lock = guard(path)?;
    let Ok(kept) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let remaining: Vec<&str> = kept
        .lines()
        .filter(|line| !said.iter().any(|message| message == line))
        .collect();
    match remaining.is_empty() {
        true => crate::file::write_atomic(path, "")?,
        false => crate::file::write_atomic(path, format!("{}\n", remaining.join("\n")))?,
    }
    Ok(())
}

/// Keeps `message` for the next command a person runs.
pub(crate) fn record(message: &str) -> Result<()> {
    record_in(&super::store::state_dir(), message)
}

/// Keeps `message` for the next command a person runs, in the store
/// under `state_dir`.
pub(crate) fn record_in(state_dir: &Path, message: &str) -> Result<()> {
    record_to(&file_in(state_dir), message)
}

fn record_to(path: &Path, message: &str) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lock = guard(path)?;
    let line = message.replace('\n', " ");
    // **A standing condition is one notice, not one per walk.** The
    // watcher walks on its own schedule and would otherwise write the
    // same line every time — a file growing without bound, and a burst
    // of identical warnings when someone finally reads it. Saying it
    // once is saying it; a condition that goes away and comes back is
    // recorded again, because the file was emptied when it was said.
    if let Ok(kept) = std::fs::read_to_string(path)
        && kept.lines().any(|existing| existing == line)
    {
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    // one line each, so a partial write loses at most the last notice
    writeln!(file, "{line}")?;
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
fn guard(path: &Path) -> Result<fslock::LockFile> {
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
pub fn drain() {
    drain_in(&super::store::state_dir());
}

/// Says everything the store under `state_dir` kept, and keeps it no
/// longer.
pub(crate) fn drain_in(state_dir: &Path) {
    for line in take(&file_in(state_dir)) {
        say(&line);
    }
}

/// The kept notices, read and cleared as one step under [`guard`].
///
/// Nothing is created by asking: with no notices file there is nothing
/// to take, and a machine that keeps no history must not grow a history
/// directory because a command looked.
fn take(path: &Path) -> Vec<String> {
    if !path.exists() {
        return vec![];
    }
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

        record_to(&path, "first").unwrap();
        record_to(&path, "second\nwith a newline in it").unwrap();
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
        record_to(&path, "third").unwrap();
        assert_eq!(take(&path), vec!["third".to_string()]);
    }

    /// **Taking back what was said out loud leaves the rest.** A
    /// protective snapshot records its warnings because the operation
    /// may fail before anything says them; the save that does say them
    /// takes those copies back, and must not take anything else with
    /// them.
    #[test]
    fn a_notice_already_said_is_taken_back_and_its_neighbours_are_not() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state/notices");

        // nothing recorded, and an empty list, are both no-ops rather
        // than errors: the save path calls this on every walk
        forget_from(&path, &["anything".to_string()]).unwrap();
        record_to(&path, "plaintext warning").unwrap();
        record_to(&path, "an unrelated notice").unwrap();
        forget_from(&path, &[]).unwrap();
        assert_eq!(
            take(&path),
            vec![
                "plaintext warning".to_string(),
                "an unrelated notice".to_string()
            ]
        );

        record_to(&path, "plaintext warning").unwrap();
        record_to(&path, "an unrelated notice").unwrap();
        forget_from(&path, &["plaintext warning".to_string()]).unwrap();
        assert_eq!(
            take(&path),
            vec!["an unrelated notice".to_string()],
            "forgetting one said notice took its neighbour with it"
        );

        // a line nobody said is left alone
        record_to(&path, "still waiting").unwrap();
        forget_from(&path, &["never said".to_string()]).unwrap();
        assert_eq!(take(&path), vec!["still waiting".to_string()]);

        // and a multi-line warning is matched in the folded form it is
        // stored as, not the form it was written in
        record_to(&path, "two\nlines").unwrap();
        forget_from(&path, &["two\nlines".to_string()]).unwrap();
        assert!(
            take(&path).is_empty(),
            "a folded notice could not be taken back"
        );
    }

    /// A notice belongs to the store it was recorded in.
    ///
    /// The path was resolved from the machine's real state directory
    /// however the store was opened, so a store opened elsewhere — a
    /// test sandbox, another machine's directory — wrote its notices
    /// into the real history and read back ones that were never its own.
    #[test]
    fn a_notice_belongs_to_the_store_it_was_recorded_in() {
        let temp = tempfile::tempdir().unwrap();
        let one = temp.path().join("one");
        let two = temp.path().join("two");
        record_in(&one, "from one").unwrap();
        // under that store's own history directory, nowhere else
        assert!(file_in(&one).starts_with(&one));
        assert!(
            take(&file_in(&two)).is_empty(),
            "another store was told a notice that was not its own"
        );
        assert_eq!(take(&file_in(&one)), vec!["from one".to_string()]);
    }

    /// Nothing is created by asking: a machine that keeps no history has
    /// no history directory, and looking for notices must not make one.
    #[test]
    fn asking_for_notices_creates_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let path = state.join("notices");
        assert!(take(&path).is_empty());
        assert!(!state.exists(), "the state directory was created by a read");
    }

    /// A condition that is still true on the next walk is not news
    /// twice, and the file it would otherwise grow is on disk.
    #[test]
    fn a_standing_condition_is_recorded_once() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state/notices");
        for _ in 0..5 {
            record_to(&path, "a credential is saved in plaintext").unwrap();
        }
        record_to(&path, "something else").unwrap();
        assert_eq!(
            take(&path),
            vec![
                "a credential is saved in plaintext".to_string(),
                "something else".to_string()
            ]
        );
        // said, and gone — so a condition that returns is news again
        record_to(&path, "a credential is saved in plaintext").unwrap();
        assert_eq!(
            take(&path),
            vec!["a credential is saved in plaintext".to_string()]
        );
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
                    record_to(&path, &format!("notice {i}")).unwrap();
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
