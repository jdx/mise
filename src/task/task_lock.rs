//! Locks a task holds while its commands run, so runs that need the same resource take turns.
//!
//! `lock = "name"` names a resource on this machine, shared by every task that names it;
//! `lock = true` locks the task itself across every worktree of its repository. Each lock is
//! a file under `$MISE_STATE_DIR/task-locks/` held under an OS file lock (`flock`, or
//! `LockFileEx` on Windows) for as long as the task runs, so the kernel releases it when the
//! holder dies and nothing ever goes stale. Lock files are never deleted: a waiter that had
//! opened a removed file would lock it while a newcomer locked its replacement.
//!
//! The holder writes who it is beside the lock, for the message a waiter prints. A record
//! whose lock can be taken is left over and is never shown.
//!
//! Locks are taken in sorted order, so two runs never each hold a lock the other wants. A task
//! exports what it holds in `MISE_TASK_LOCKS`, and a task that inherits a lock there, from a
//! nested `mise run` or from a `run = [{ task }]` entry, treats it as already held instead of
//! waiting on its own parent.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use crate::dirs;
use crate::file::display_path;
use crate::task::{Task, TaskLock};

/// The locks a task's commands run under, comma-separated.
pub(crate) const LOCKS_ENV: &str = "MISE_TASK_LOCKS";

/// How often a waiter says it is still waiting.
const REPORT_EVERY: Duration = Duration::from_secs(30);
const FIRST_POLL: Duration = Duration::from_millis(100);
const MAX_POLL: Duration = Duration::from_secs(1);

/// One lock a task asks for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LockKey {
    /// What `MISE_TASK_LOCKS` carries and the lock file is named after.
    id: String,
    /// What messages call it.
    display: String,
}

impl LockKey {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    fn named(name: &str) -> Self {
        Self {
            id: name.to_string(),
            display: name.to_string(),
        }
    }

    /// `lock = true`: the task's name in its repository. Every worktree of a repository shares
    /// one git common dir, so they share this key.
    fn this_task(task: &Task) -> Self {
        let dir = task
            .config_root
            .clone()
            .or_else(|| task.config_source.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let repo = repository_of(&dir);
        let id = format!(
            "task:{}",
            &mise_util::hash::hash_sha256_to_str(&format!(
                "{}\0{}",
                repo.to_string_lossy(),
                task.name
            ))[..16]
        );
        Self {
            id,
            display: format!("{} (this repository)", task.name),
        }
    }

    fn path(&self, ext: &str) -> PathBuf {
        let file = &mise_util::hash::hash_sha256_to_str(&self.id)[..16];
        locks_dir().join(format!("{file}.{ext}"))
    }
}

fn locks_dir() -> PathBuf {
    dirs::STATE.join("task-locks")
}

/// The git directory every worktree of the repository holding `dir` shares, or `dir` itself
/// outside git.
fn repository_of(dir: &Path) -> PathBuf {
    let dir = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    for root in dir.ancestors() {
        let dot_git = root.join(".git");
        if dot_git.is_dir() {
            return dunce::canonicalize(&dot_git).unwrap_or(dot_git);
        }
        // A linked worktree or a submodule has a file naming its private git dir; a linked
        // worktree's private dir names the shared one in `commondir`.
        if let Ok(text) = std::fs::read_to_string(&dot_git)
            && let Some(target) = text.trim().strip_prefix("gitdir:")
        {
            let git_dir = root.join(target.trim());
            let shared = match std::fs::read_to_string(git_dir.join("commondir")) {
                Ok(common) => git_dir.join(common.trim()),
                Err(_) => git_dir,
            };
            return dunce::canonicalize(&shared).unwrap_or(shared);
        }
    }
    dir
}

/// The locks `task` asks for, sorted and without repeats.
pub(crate) fn requested(task: &Task) -> Vec<LockKey> {
    let mut keys = match &task.lock {
        None | Some(TaskLock::Off) => vec![],
        Some(TaskLock::Task) => vec![LockKey::this_task(task)],
        Some(TaskLock::Names(names)) => names.iter().map(|n| LockKey::named(n)).collect(),
    };
    keys.sort();
    keys.dedup();
    keys
}

/// How `mise tasks info` and `--dry-run` describe a task's locks.
pub fn describe(task: &Task) -> Option<String> {
    let keys = requested(task);
    (!keys.is_empty()).then(|| {
        keys.iter()
            .map(|k| k.display.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// The locks a parent task already holds for this one.
pub(crate) fn inherited(env: &BTreeMap<String, String>) -> BTreeSet<String> {
    env.get(LOCKS_ENV)
        .map(|value| {
            value
                .split(',')
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// What `MISE_TASK_LOCKS` says once this task holds `keys` on top of what it inherited.
pub(crate) fn exported(inherited: &BTreeSet<String>, keys: &[LockKey]) -> String {
    let mut all = inherited.clone();
    all.extend(keys.iter().map(|k| k.id.clone()));
    all.into_iter().collect::<Vec<_>>().join(",")
}

/// A wait or a holder's age: tenths of a second under a minute, whole units above.
fn human(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        0..60 => format!("{:.1}s", d.as_secs_f64()),
        60..3600 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, secs % 3600 / 60),
    }
}

/// Who holds a lock, as the holder wrote it.
#[derive(Debug, Serialize, Deserialize)]
struct HolderRecord {
    pid: u32,
    task: String,
    root: Option<PathBuf>,
    started_at: u64,
}

impl HolderRecord {
    fn describe(&self) -> String {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .saturating_sub(Duration::from_secs(self.started_at));
        let since = human(since);
        match &self.root {
            Some(root) => format!(
                "held by {} in {} (pid {}, for {since})",
                self.task,
                display_path(root),
                self.pid
            ),
            None => format!("held by {} (pid {}, for {since})", self.task, self.pid),
        }
    }
}

/// Locks this task holds. Dropping it removes the holder records, then releases the locks.
pub(crate) struct HeldLocks {
    locks: Vec<(LockKey, fslock::LockFile)>,
}

impl Drop for HeldLocks {
    fn drop(&mut self) {
        // The record goes first, while the lock still keeps the next holder from writing its own.
        for (key, _) in &self.locks {
            let _ = std::fs::remove_file(key.path("json"));
        }
    }
}

/// Taking a task's locks, one at a time in sorted order.
pub(crate) struct LockAcquisition<'a> {
    task: &'a Task,
    pending: std::vec::IntoIter<LockKey>,
    /// The lock this acquisition is waiting for, already opened.
    current: Option<(LockKey, fslock::LockFile)>,
    held: HeldLocks,
}

impl<'a> LockAcquisition<'a> {
    pub(crate) fn new(task: &'a Task, keys: Vec<LockKey>) -> Result<Self> {
        let dir = locks_dir();
        std::fs::create_dir_all(&dir)
            .wrap_err_with(|| format!("task {}: cannot create {}", task.name, dir.display()))?;
        Ok(Self {
            task,
            pending: keys.into_iter(),
            current: None,
            held: HeldLocks { locks: vec![] },
        })
    }

    /// Takes every lock that is free, in order, and stops at the first one that is not.
    /// `true` once every lock is held.
    pub(crate) fn try_take(&mut self) -> Result<bool> {
        loop {
            let (key, mut file) = match self.current.take() {
                Some(current) => current,
                None => match self.pending.next() {
                    Some(key) => {
                        let path = key.path("lock");
                        let file = fslock::LockFile::open(&path).wrap_err_with(|| {
                            format!(
                                "task {}: cannot open lock file {}",
                                self.task.name,
                                path.display()
                            )
                        })?;
                        (key, file)
                    }
                    None => return Ok(true),
                },
            };
            if !file.try_lock().wrap_err_with(|| {
                format!("task {}: cannot take lock {}", self.task.name, key.display)
            })? {
                self.current = Some((key, file));
                return Ok(false);
            }
            self.write_record(&key);
            self.held.locks.push((key, file));
        }
    }

    /// The lock this acquisition waits for and who holds it, if the holder said.
    pub(crate) fn waiting_for(&self) -> String {
        let Some((key, _)) = &self.current else {
            return String::new();
        };
        let holder = std::fs::read_to_string(key.path("json"))
            .ok()
            .and_then(|text| serde_json::from_str::<HolderRecord>(&text).ok())
            .map(|record| format!(", {}", record.describe()))
            .unwrap_or_default();
        format!("lock {}{holder}", key.display)
    }

    /// Waits until every lock is held. `report` prints a line now and every 30 seconds;
    /// `check_interruption` ends the wait as soon as the run is stopping.
    pub(crate) async fn wait(
        mut self,
        timeout: Option<Duration>,
        report: impl Fn(&str),
        check_interruption: impl Fn() -> Result<()>,
    ) -> Result<HeldLocks> {
        let start = Instant::now();
        let mut reported_at: Option<Instant> = None;
        let mut poll = FIRST_POLL;
        while !self.try_take()? {
            check_interruption()?;
            if let Some(timeout) = timeout
                && start.elapsed() >= timeout
            {
                bail!(
                    "task {}: timed out after {} waiting for {}",
                    self.task.name,
                    human(timeout),
                    self.waiting_for()
                );
            }
            if reported_at.is_none_or(|at| at.elapsed() >= REPORT_EVERY) {
                report(&format!("waiting for {}", self.waiting_for()));
                reported_at = Some(Instant::now());
            }
            tokio::time::sleep(poll).await;
            poll = (poll * 2).min(MAX_POLL);
        }
        if reported_at.is_some() {
            report(&format!(
                "got {} after {}",
                self.held_names(),
                human(start.elapsed())
            ));
        }
        Ok(self.held)
    }

    /// Every lock, once `try_take` returned `true`.
    pub(crate) fn into_held(self) -> HeldLocks {
        debug_assert!(self.current.is_none());
        self.held
    }

    fn held_names(&self) -> String {
        let names = self
            .held
            .locks
            .iter()
            .map(|(k, _)| k.display.as_str())
            .collect::<Vec<_>>();
        format!(
            "{} {}",
            if names.len() == 1 { "lock" } else { "locks" },
            names.join(", ")
        )
    }

    fn write_record(&self, key: &LockKey) {
        let record = HolderRecord {
            pid: std::process::id(),
            task: self.task.name.clone(),
            root: self.task.config_root.clone(),
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default(),
        };
        // Only for the message other runs print, so a failure here changes nothing else.
        if let Ok(json) = serde_json::to_string(&record)
            && let Err(err) = crate::file::write(key.path("json"), json)
        {
            debug!("task {}: cannot record lock holder: {err}", self.task.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree(main: &Path, name: &str) -> PathBuf {
        let private = main.join(".git").join("worktrees").join(name);
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        let root = main.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .unwrap();
        root
    }

    #[test]
    fn worktrees_share_their_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        let linked = worktree(&main, "feature");
        let nested = main.join("crates").join("a");
        std::fs::create_dir_all(&nested).unwrap();

        let shared = repository_of(&main);
        assert_eq!(repository_of(&linked), shared);
        assert_eq!(repository_of(&nested), shared);

        let other = tmp.path().join("other");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        assert_ne!(repository_of(&other), shared);
    }

    #[test]
    fn durations_read_at_a_glance() {
        assert_eq!(human(Duration::from_millis(1500)), "1.5s");
        assert_eq!(human(Duration::from_secs(182)), "3m02s");
        assert_eq!(human(Duration::from_secs(7260)), "2h01m");
    }

    #[test]
    fn inherited_locks_round_trip_through_the_env() {
        let keys = vec![LockKey::named("b"), LockKey::named("a")];
        let parent: BTreeSet<String> = ["c".to_string()].into();
        let value = exported(&parent, &keys);
        assert_eq!(value, "a,b,c");
        let env = BTreeMap::from([(LOCKS_ENV.to_string(), value)]);
        assert_eq!(
            inherited(&env),
            ["a", "b", "c"].map(String::from).into_iter().collect()
        );
        assert!(inherited(&BTreeMap::new()).is_empty());
    }
}
