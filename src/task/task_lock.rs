//! Locks a task holds while its commands run, so runs that need the same resource take turns.
//!
//! `lock = "name"` names a resource on this machine, shared by every task that names it;
//! `lock = true` locks the task itself across every worktree of its repository. Each lock is
//! a file under `$MISE_STATE_DIR/task-locks/` held under an OS file lock (`flock`, or
//! `LockFileEx` on Windows) for as long as the task runs, so the kernel releases it when the
//! holder dies and nothing ever goes stale. Lock files are never deleted: a waiter that had
//! opened a removed file would lock it while a newcomer locked its replacement.
//!
//! A named lock is a pool of slots, one unless `task.lock_pools` gives it more, each its own
//! file. A run takes `count` of them (`lock = { name = "cpu", count = 4 }`), all or none: it
//! tries the slots under a short admission lock for its pool, and gives back any it took if
//! fewer than `count` are free, so two waiters never each hold part of what the other needs.
//! Slot 0 is the file a plain `lock = "name"` always took, and slot N is `name:N`.
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

use crate::config::Settings;
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
    /// How many of the pool's slots the task holds.
    count: u32,
    /// How many slots the pool has.
    capacity: u32,
    /// Slots a parent task already holds, which this one must not try again.
    skip: BTreeSet<u32>,
}

impl LockKey {
    /// A pool of `capacity` slots, `count` of them wanted. A count above the capacity is
    /// lowered to it.
    fn pool(name: &str, count: u32, capacity: u32) -> Self {
        let capacity = capacity.max(1);
        let count = count.clamp(1, capacity);
        let display = if capacity == 1 {
            name.to_string()
        } else {
            format!("{name} ({count} of {capacity} slots)")
        };
        Self {
            id: name.to_string(),
            display,
            count,
            capacity,
            skip: BTreeSet::new(),
        }
    }

    #[cfg(test)]
    fn named(name: &str) -> Self {
        Self::pool(name, 1, 1)
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
            count: 1,
            capacity: 1,
            skip: BTreeSet::new(),
        }
    }

    /// What `MISE_TASK_LOCKS` calls slot `n`.
    fn slot_id(&self, n: u32) -> String {
        if n == 0 {
            self.id.clone()
        } else {
            format!("{}:{n}", self.id)
        }
    }

    fn slot_path(&self, n: u32, ext: &str) -> PathBuf {
        let file = &mise_util::hash::hash_sha256_to_str(&self.slot_id(n))[..16];
        locks_dir().join(format!("{file}.{ext}"))
    }

    /// The lock that admits one run at a time to take several slots.
    fn admission_path(&self) -> PathBuf {
        self.slot_path(0, "admit")
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

/// How many slots the pool `name` has: one unless `task.lock_pools` says otherwise.
fn capacity_of(name: &str) -> Result<u32> {
    let Some(value) = Settings::get()
        .task
        .lock_pools
        .as_ref()
        .and_then(|pools| pools.get(name).cloned())
    else {
        return Ok(1);
    };
    match value.trim().parse::<u32>() {
        Ok(n) if (1..=MAX_SLOTS).contains(&n) => Ok(n),
        _ => bail!(
            "task.lock_pools.{name} must be a whole number from 1 to {MAX_SLOTS}, not {value:?}"
        ),
    }
}

/// More slots than any machine has cores, so a typo does not make a thousand lock files.
const MAX_SLOTS: u32 = 4096;

/// The locks `task` asks for, sorted and without repeats. A name listed twice takes the larger
/// count.
pub(crate) fn requested(task: &Task) -> Result<Vec<LockKey>> {
    let mut keys = match &task.lock {
        None | Some(TaskLock::Off) => vec![],
        Some(TaskLock::Task) => vec![LockKey::this_task(task)],
        Some(TaskLock::Names(locks)) => locks
            .iter()
            .map(|l| Ok(LockKey::pool(&l.name, l.count.get(), capacity_of(&l.name)?)))
            .collect::<Result<Vec<_>>>()?,
    };
    keys.sort();
    keys.dedup_by(|later, first| {
        let same = later.id == first.id;
        if same && later.count > first.count {
            *first = later.clone();
        }
        same
    });
    Ok(keys)
}

/// How `mise tasks info` and `--dry-run` describe a task's locks.
pub fn describe(task: &Task) -> Option<String> {
    let keys = requested(task).ok()?;
    (!keys.is_empty()).then(|| {
        keys.iter()
            .map(|k| k.display.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// What `keys` still need once the slots a parent task holds are counted: a key the parent
/// covers is dropped, and one it covers in part keeps the slots it holds in `skip`.
pub(crate) fn still_needed(keys: &[LockKey], inherited: &BTreeSet<String>) -> Vec<LockKey> {
    keys.iter()
        .filter_map(|key| {
            let held: BTreeSet<u32> = (0..key.capacity)
                .filter(|n| inherited.contains(&key.slot_id(*n)))
                .collect();
            if held.len() as u32 >= key.count {
                return None;
            }
            let mut key = key.clone();
            key.count -= held.len() as u32;
            key.skip = held;
            Some(key)
        })
        .collect()
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

/// Every run takes locks in one order, sorted by id, so no two runs can each hold a lock the
/// other waits for. A task started under a locked task already holds what it inherited, so it
/// may only add locks that sort after all of them: one parent holding `a` whose child wants `b`
/// and another holding `b` whose child wants `a` would otherwise wait for each other forever.
pub(crate) fn check_nested_order(
    task: &Task,
    inherited: &BTreeSet<String>,
    to_take: &[LockKey],
) -> Result<()> {
    if let Some(key) = to_take.iter().find(|key| !key.skip.is_empty()) {
        bail!(
            "task {}: cannot take more of lock {} while a parent task holds some of it: two \
             parents could each wait for the slots the other holds. Raise the `count` on the \
             parent task's `lock`",
            task.name,
            key.display
        );
    }
    // Compare lock names: an inherited id may be a slot, `name:N`, which sorts after `name`.
    let held = inherited.iter().map(|id| pool_of(id)).max();
    let (Some(held), Some(first)) = (held, to_take.first()) else {
        return Ok(());
    };
    if first.id.as_str() < held {
        bail!(
            "task {}: cannot take lock {} while a parent task holds {held}: a nested task can only \
             add locks that sort after the ones it inherits. Add {} to the parent task's `lock`",
            task.name,
            first.display,
            first.display
        );
    }
    Ok(())
}

/// The lock a `MISE_TASK_LOCKS` id belongs to: the id without a slot suffix (`name:N`).
fn pool_of(id: &str) -> &str {
    match id.rsplit_once(':') {
        Some((name, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => id,
    }
}

/// What `MISE_TASK_LOCKS` says once this task holds `held` on top of what it inherited.
pub(crate) fn exported(inherited: &BTreeSet<String>, held: &HeldLocks) -> String {
    let mut all = inherited.clone();
    all.extend(held.slot_ids());
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
    locks: Vec<HeldPool>,
}

/// The slots of one lock a task holds.
struct HeldPool {
    key: LockKey,
    slots: Vec<(u32, fslock::LockFile)>,
}

impl HeldLocks {
    /// Whether this task took no lock of its own, because it inherited every one it names.
    pub(crate) fn is_empty(&self) -> bool {
        self.locks.is_empty()
    }

    /// What `MISE_TASK_LOCKS` calls each slot held.
    fn slot_ids(&self) -> Vec<String> {
        self.locks
            .iter()
            .flat_map(|pool| pool.slots.iter().map(|(n, _)| pool.key.slot_id(*n)))
            .collect()
    }
}

impl Drop for HeldLocks {
    fn drop(&mut self) {
        // The record goes first, while the lock still keeps the next holder from writing its own.
        for pool in &self.locks {
            for (n, _) in &pool.slots {
                let _ = std::fs::remove_file(pool.key.slot_path(*n, "json"));
            }
        }
    }
}

/// Taking a task's locks, one at a time in sorted order.
pub(crate) struct LockAcquisition<'a> {
    task: &'a Task,
    pending: std::vec::IntoIter<LockKey>,
    /// The lock this acquisition is waiting for.
    current: Option<LockKey>,
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

    fn open(&self, path: &Path) -> Result<fslock::LockFile> {
        fslock::LockFile::open(path).wrap_err_with(|| {
            format!(
                "task {}: cannot open lock file {}",
                self.task.name,
                path.display()
            )
        })
    }

    /// Takes `key.count` free slots, or none. Several slots are tried under the pool's
    /// admission lock, which is held only for this attempt and never while waiting.
    fn try_take_slots(&self, key: &LockKey) -> Result<Option<Vec<(u32, fslock::LockFile)>>> {
        let fail = |err: std::io::Error| {
            eyre::Report::new(err).wrap_err(format!(
                "task {}: cannot take lock {}",
                self.task.name, key.display
            ))
        };
        let mut admission = if key.count > 1 {
            let mut file = self.open(&key.admission_path())?;
            if !file.try_lock().map_err(fail)? {
                return Ok(None);
            }
            Some(file)
        } else {
            None
        };
        let mut taken = vec![];
        for n in (0..key.capacity).filter(|n| !key.skip.contains(n)) {
            let mut file = self.open(&key.slot_path(n, "lock"))?;
            if file.try_lock().map_err(fail)? {
                taken.push((n, file));
                if taken.len() as u32 == key.count {
                    break;
                }
            }
        }
        if let Some(file) = admission.as_mut() {
            let _ = file.unlock();
        }
        Ok((taken.len() as u32 == key.count).then_some(taken))
    }

    /// Takes every lock that is free, in order, and stops at the first one that is not.
    /// `true` once every lock is held.
    pub(crate) fn try_take(&mut self) -> Result<bool> {
        loop {
            let Some(key) = self.current.take().or_else(|| self.pending.next()) else {
                return Ok(true);
            };
            let Some(slots) = self.try_take_slots(&key)? else {
                self.current = Some(key);
                return Ok(false);
            };
            for (n, _) in &slots {
                self.write_record(&key, *n);
            }
            self.held.locks.push(HeldPool { key, slots });
        }
    }

    /// The lock this acquisition waits for and who holds it, if the holders said.
    pub(crate) fn waiting_for(&self) -> String {
        let Some(key) = &self.current else {
            return String::new();
        };
        let mut holders = vec![];
        for n in (0..key.capacity).filter(|n| !key.skip.contains(n)) {
            // A record counts only while its slot is held.
            let held = match fslock::LockFile::open(&key.slot_path(n, "lock")) {
                Ok(mut file) => !file.try_lock().unwrap_or(true),
                Err(_) => false,
            };
            if !held {
                continue;
            }
            if let Some(record) = std::fs::read_to_string(key.slot_path(n, "json"))
                .ok()
                .and_then(|text| serde_json::from_str::<HolderRecord>(&text).ok())
            {
                holders.push(record.describe());
            }
        }
        let holders: String = holders.iter().map(|h| format!(", {h}")).collect();
        format!("lock {}{holders}", key.display)
    }

    /// Waits until every lock is held. `waited` is the time this task already spent waiting
    /// for locks, and this wait adds to it: a task that gave its locks back to wait for a
    /// `--jobs` slot keeps only the `timeout` it has left, and the slot wait does not count.
    /// `report` prints a line now and every 30 seconds; `check_interruption` ends the wait as
    /// soon as the run is stopping.
    pub(crate) async fn wait(
        mut self,
        timeout: Option<Duration>,
        waited: &mut Duration,
        report: impl Fn(&str),
        check_interruption: impl Fn() -> Result<()>,
    ) -> Result<HeldLocks> {
        let start = Instant::now();
        let mut reported_at: Option<Instant> = None;
        let mut poll = FIRST_POLL;
        while !self.try_take()? {
            check_interruption()?;
            if let Some(timeout) = timeout
                && *waited + start.elapsed() >= timeout
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
                human(*waited + start.elapsed())
            ));
        }
        *waited += start.elapsed();
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
            .map(|pool| pool.key.display.as_str())
            .collect::<Vec<_>>();
        format!(
            "{} {}",
            if names.len() == 1 { "lock" } else { "locks" },
            names.join(", ")
        )
    }

    fn write_record(&self, key: &LockKey, slot: u32) {
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
            && let Err(err) = crate::file::write(key.slot_path(slot, "json"), json)
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
    fn nested_tasks_only_add_locks_after_the_ones_they_inherit() {
        let task = Task {
            name: "child".into(),
            ..Default::default()
        };
        let held: BTreeSet<String> = ["b".to_string()].into();
        assert!(check_nested_order(&task, &held, &[LockKey::named("c")]).is_ok());
        assert!(check_nested_order(&task, &BTreeSet::new(), &[LockKey::named("a")]).is_ok());
        assert!(check_nested_order(&task, &held, &[]).is_ok());
        let err = check_nested_order(&task, &held, &[LockKey::named("a")]).unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot take lock a while a parent task holds b"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_retry_keeps_the_time_left_to_wait() {
        let task = Task {
            name: "retry".into(),
            ..Default::default()
        };
        let key = LockKey::named(&format!("budget-{}", std::process::id()));
        std::fs::create_dir_all(locks_dir()).unwrap();
        let mut holder = fslock::LockFile::open(&key.slot_path(0, "lock")).unwrap();
        assert!(holder.try_lock().unwrap());

        let mut acquisition = LockAcquisition::new(&task, vec![key]).unwrap();
        assert!(!acquisition.try_take().unwrap());
        // Five seconds already spent waiting before the locks were given back.
        let mut waited = Duration::from_secs(5);
        let started = Instant::now();
        let err = acquisition
            .wait(Some(Duration::from_secs(1)), &mut waited, |_| {}, || Ok(()))
            .await
            .err()
            .expect("the budget was already spent");
        assert!(err.to_string().contains("timed out after 1.0s"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(1));

        // Once the lock is free, the wait adds its own time to what the task spent before.
        drop(holder);
        let key = LockKey::named(&format!("budget-{}", std::process::id()));
        let acquisition = LockAcquisition::new(&task, vec![key]).unwrap();
        let mut waited = Duration::from_secs(2);
        let held = acquisition
            .wait(
                Some(Duration::from_secs(10)),
                &mut waited,
                |_| {},
                || Ok(()),
            )
            .await
            .unwrap();
        drop(held);
        assert!(waited >= Duration::from_secs(2) && waited < Duration::from_secs(3));
    }

    #[test]
    fn durations_read_at_a_glance() {
        assert_eq!(human(Duration::from_millis(1500)), "1.5s");
        assert_eq!(human(Duration::from_secs(182)), "3m02s");
        assert_eq!(human(Duration::from_secs(7260)), "2h01m");
    }

    #[test]
    fn inherited_locks_are_read_from_the_env() {
        let env = BTreeMap::from([(LOCKS_ENV.to_string(), "a,b,c:1".to_string())]);
        assert_eq!(
            inherited(&env),
            ["a", "b", "c:1"].map(String::from).into_iter().collect()
        );
        assert!(inherited(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn a_slot_id_belongs_to_its_lock() {
        assert_eq!(pool_of("cpu:1"), "cpu");
        assert_eq!(pool_of("cpu"), "cpu");
        assert_eq!(pool_of("cpu0"), "cpu0");
        assert_eq!(pool_of("a:b"), "a:b");
    }

    #[test]
    fn a_count_above_the_pool_is_lowered_to_it() {
        let key = LockKey::pool("cpu", 9, 4);
        assert_eq!((key.count, key.capacity), (4, 4));
        let key = LockKey::pool("cpu", 3, 1);
        assert_eq!((key.count, key.capacity), (1, 1));
        assert_eq!(key.display, "cpu");
        assert_eq!(LockKey::pool("cpu", 2, 8).display, "cpu (2 of 8 slots)");
    }

    #[test]
    fn slots_are_named_after_the_lock() {
        let key = LockKey::pool("cpu", 1, 4);
        assert_eq!(key.slot_id(0), "cpu");
        assert_eq!(key.slot_id(2), "cpu:2");
        assert_ne!(key.slot_path(0, "lock"), key.slot_path(1, "lock"));
    }

    #[test]
    fn a_parent_that_holds_slots_covers_a_smaller_need() {
        let keys = vec![LockKey::pool("cpu", 2, 4)];
        let held: BTreeSet<String> = ["cpu", "cpu:1", "cpu:3"].map(String::from).into();
        assert!(still_needed(&keys, &held).is_empty());
        let held: BTreeSet<String> = ["cpu", "cpu:1"].map(String::from).into();
        assert!(still_needed(&keys, &held).is_empty());
        let held: BTreeSet<String> = ["cpu:1"].map(String::from).into();
        let needed = still_needed(&keys, &held);
        assert_eq!(needed.len(), 1);
        assert_eq!(needed[0].count, 1);
        assert_eq!(needed[0].skip, BTreeSet::from([1]));
        let task = Task {
            name: "child".into(),
            ..Default::default()
        };
        let err = check_nested_order(&task, &held, &needed).unwrap_err();
        assert!(
            err.to_string().contains("cannot take more of lock"),
            "{err}"
        );
    }

    #[test]
    fn several_slots_are_taken_all_or_none() {
        let task = Task {
            name: "pool".into(),
            ..Default::default()
        };
        let name = format!("pool-{}", std::process::id());
        // Two runs share a pool of four: 3 + 2 does not fit, 3 + 1 does.
        let mut first = LockAcquisition::new(&task, vec![LockKey::pool(&name, 3, 4)]).unwrap();
        assert!(first.try_take().unwrap());
        let mut second = LockAcquisition::new(&task, vec![LockKey::pool(&name, 2, 4)]).unwrap();
        assert!(!second.try_take().unwrap());
        // The failed attempt kept nothing, so a single slot is still free.
        let mut third = LockAcquisition::new(&task, vec![LockKey::pool(&name, 1, 4)]).unwrap();
        assert!(third.try_take().unwrap());
        let held = first.into_held();
        assert_eq!(
            held.slot_ids(),
            [name.clone(), format!("{name}:1"), format!("{name}:2")]
        );
        assert_eq!(third.into_held().slot_ids(), [format!("{name}:3")]);
        // Once the first run ends, the second fits.
        drop(held);
        assert!(second.try_take().unwrap());
    }
}
