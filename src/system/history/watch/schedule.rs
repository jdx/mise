//! Per-file adaptive save scheduling. Ordinary edits save promptly: a
//! changed path is saved once it has been quiet for its settle time.
//! Sustained churn stretches that path's own interval — doubling on every
//! save that follows another one closely with several changes in between,
//! up to a maximum — so a constantly rewritten file is still saved
//! periodically but never floods the history. A busy path never delays an
//! ordinary one: every path is scheduled on its own. When a churning path
//! settles, its final state is captured after its settle time, and a
//! sustained quiet period resets the interval so a brief pause does not
//! restart aggressive saving.
//!
//! Every threshold is a named constant here; the schedule is pure and
//! clock-injected so each rule is tested without a filesystem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A save counts as churn — and doubles the interval — when the path
/// changed again within its settle time after the previous save (it never
/// really stopped) and at least this many changes arrived since. A person
/// saving from an editor every few seconds leaves gaps longer than the
/// settle time, so ordinary editing is never stretched.
pub(crate) const CHURN_CHANGES: u32 = 2;
/// A path quiet for this many intervals (at least `RESET_MIN`) forgets its
/// backoff.
pub(crate) const RESET_FACTOR: u32 = 4;
pub(crate) const RESET_MIN: Duration = Duration::from_secs(5 * 60);
/// The settle time of a stretched path: a fraction of its interval, never
/// below the base quiet period nor above this cap, so a settled file is
/// captured promptly whatever its interval grew to.
pub(crate) const SETTLE_DIVISOR: u32 = 8;
pub(crate) const SETTLE_MAX: Duration = Duration::from_secs(5 * 60);
/// A path whose interval reached this is reported as heavily throttled.
pub(crate) const HEAVY_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    /// The base quiet period (`history.watch.debounce`).
    pub base: Duration,
    /// The longest periodic interval (`history.watch.max_interval`).
    pub max: Duration,
}

impl Limits {
    fn clamp(&self, interval: Duration) -> Duration {
        interval.max(self.base).min(self.max.max(self.base))
    }
}

/// One path's scheduling state.
#[derive(Clone, Debug)]
pub(crate) struct PathSchedule {
    /// The current periodic interval.
    pub interval: Duration,
    /// When the path last changed (`None`: no change pending).
    pub last_change: Option<Instant>,
    /// When the pending batch of changes began.
    pub pending_since: Option<Instant>,
    /// Changes since the last save.
    pub changes: u32,
    pub last_saved: Option<Instant>,
}

impl PathSchedule {
    fn new(base: Duration) -> Self {
        Self {
            interval: base,
            last_change: None,
            pending_since: None,
            changes: 0,
            last_saved: None,
        }
    }

    pub(crate) fn pending(&self) -> bool {
        self.last_change.is_some()
    }

    /// Quiet needed before a pending change is saved.
    fn settle(&self, limits: &Limits) -> Duration {
        (self.interval / SETTLE_DIVISOR)
            .max(limits.base)
            .min(SETTLE_MAX)
    }

    /// When this path's pending change is due: quiet for its settle time,
    /// or its interval since the batch began, whichever comes first.
    pub(crate) fn due(&self, limits: &Limits) -> Option<Instant> {
        let last = self.last_change?;
        let quiet = last + self.settle(limits);
        let periodic = self.pending_since.unwrap_or(last) + self.interval;
        Some(quiet.min(periodic))
    }

    fn reset_after(&self) -> Duration {
        (self.interval * RESET_FACTOR).max(RESET_MIN)
    }
}

/// The persisted part: what a restart must not forget.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct PersistedSchedule {
    #[serde(default)]
    pub paths: BTreeMap<String, PersistedPath>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct PersistedPath {
    pub interval_secs: u64,
    /// When the path was last saved (unix seconds), for the reset rule and
    /// for telling a changed file from a quiet one after a restart.
    #[serde(default)]
    pub saved_epoch_secs: Option<u64>,
    /// Changes seen since that save, and when the pending batch began and
    /// last changed, so a restart neither forgets a pending save nor takes
    /// it early.
    #[serde(default)]
    pub pending_changes: u32,
    #[serde(default)]
    pub pending_since_epoch_secs: Option<u64>,
    #[serde(default)]
    pub last_change_epoch_secs: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct Schedule {
    limits: Limits,
    paths: BTreeMap<PathBuf, PathSchedule>,
}

/// What a save of a path reported: whether its interval changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Adjustment {
    Unchanged,
    Stretched,
    Reset,
}

impl Schedule {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            paths: BTreeMap::new(),
        }
    }

    pub(crate) fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Restores stretched intervals from a previous run, with their last
    /// save and pending batch placed on this run's clock (`now` is the
    /// instant that corresponds to the wall clock `now_epoch`), so the
    /// schedule continues where it stopped: a path quiet longer than its
    /// reset period comes back at the base interval, and a pending save is
    /// neither forgotten nor taken early.
    pub(crate) fn restore(&mut self, persisted: &PersistedSchedule, now: Instant, now_epoch: u64) {
        let ago = |epoch: u64| {
            now.checked_sub(Duration::from_secs(now_epoch.saturating_sub(epoch)))
                .unwrap_or(now)
        };
        for (path, record) in &persisted.paths {
            let interval = self.limits.clamp(Duration::from_secs(record.interval_secs));
            if interval <= self.limits.base {
                continue;
            }
            let mut schedule = PathSchedule::new(self.limits.base);
            schedule.interval = interval;
            let quiet_for = record
                .saved_epoch_secs
                .map(|saved| now_epoch.saturating_sub(saved));
            if quiet_for.is_some_and(|quiet| Duration::from_secs(quiet) >= schedule.reset_after()) {
                continue;
            }
            schedule.last_saved = record.saved_epoch_secs.map(ago);
            if record.pending_changes > 0 {
                schedule.changes = record.pending_changes;
                schedule.pending_since =
                    Some(record.pending_since_epoch_secs.map(ago).unwrap_or(now));
                schedule.last_change = Some(record.last_change_epoch_secs.map(ago).unwrap_or(now));
            }
            self.paths.insert(PathBuf::from(path), schedule);
        }
    }

    /// The persisted form. Only stretched paths matter.
    pub(crate) fn persist(&self, now: Instant, now_epoch: u64) -> PersistedSchedule {
        let mut out = PersistedSchedule::default();
        let epoch =
            |at: Instant| now_epoch.saturating_sub(now.saturating_duration_since(at).as_secs());
        for (path, schedule) in &self.paths {
            if schedule.interval <= self.limits.base {
                continue;
            }
            out.paths.insert(
                path.to_string_lossy().into_owned(),
                PersistedPath {
                    interval_secs: schedule.interval.as_secs(),
                    saved_epoch_secs: schedule.last_saved.map(epoch),
                    pending_changes: schedule.changes,
                    pending_since_epoch_secs: schedule.pending_since.map(epoch),
                    last_change_epoch_secs: schedule.last_change.map(epoch),
                },
            );
        }
        out
    }

    /// Applies new limits (a settings change while running): every interval
    /// is clamped into the new range.
    pub(crate) fn set_limits(&mut self, limits: Limits) {
        for schedule in self.paths.values_mut() {
            schedule.interval = limits.clamp(schedule.interval);
        }
        self.limits = limits;
    }

    /// Whether `path` is throttled (its interval is above the base).
    pub(crate) fn is_throttled(&self, path: &Path) -> bool {
        self.paths
            .get(path)
            .is_some_and(|schedule| schedule.interval > self.limits.base)
    }

    /// A change to `path` was seen.
    pub(crate) fn note(&mut self, path: PathBuf, now: Instant) {
        let base = self.limits.base;
        let schedule = self
            .paths
            .entry(path)
            .or_insert_with(|| PathSchedule::new(base));
        // a path quiet for its reset period comes back at the base interval
        if let Some(saved) = schedule.last_saved
            && schedule.last_change.is_none()
            && now.saturating_duration_since(saved) >= schedule.reset_after()
        {
            schedule.interval = base;
        }
        schedule.pending_since.get_or_insert(now);
        schedule.last_change = Some(now);
        schedule.changes += 1;
    }

    /// The next moment any pending path is due.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.paths
            .values()
            .filter_map(|schedule| schedule.due(&self.limits))
            .min()
    }

    /// The pending paths that are due now.
    pub(crate) fn due_paths(&self, now: Instant) -> Vec<PathBuf> {
        self.paths
            .iter()
            .filter(|(_, schedule)| schedule.due(&self.limits).is_some_and(|due| due <= now))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// The pending paths that are not due yet: their live content is held
    /// back from captures other paths trigger, so a capture for another
    /// path cannot defeat this path's throttling.
    pub(crate) fn held_paths(&self, now: Instant) -> Vec<PathBuf> {
        self.paths
            .iter()
            .filter(|(_, schedule)| schedule.due(&self.limits).is_some_and(|due| due > now))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Records that `path` was saved now and adapts its interval.
    pub(crate) fn saved(&mut self, path: &Path, now: Instant) -> Adjustment {
        let base = self.limits.base;
        let Some(schedule) = self.paths.get_mut(path) else {
            return Adjustment::Unchanged;
        };
        let settle = schedule.settle(&self.limits);
        let churn = match (schedule.last_saved, schedule.pending_since) {
            (Some(previous), Some(since)) => {
                since.saturating_duration_since(previous) <= settle
                    && schedule.changes >= CHURN_CHANGES
            }
            _ => false,
        };
        let reset = schedule.last_saved.is_some_and(|previous| {
            now.saturating_duration_since(previous) >= schedule.reset_after()
        }) && schedule.interval > base;
        let adjustment = if churn && schedule.interval < self.limits.max {
            schedule.interval = self.limits.clamp(schedule.interval * 2);
            Adjustment::Stretched
        } else if reset {
            schedule.interval = base;
            Adjustment::Reset
        } else {
            Adjustment::Unchanged
        };
        schedule.last_saved = Some(now);
        schedule.last_change = None;
        schedule.pending_since = None;
        schedule.changes = 0;
        adjustment
    }

    /// Forgets pending changes for every path (after a full capture that
    /// read everything live, such as an explicit save or a shutdown).
    pub(crate) fn clear_pending(&mut self, now: Instant) {
        for schedule in self.paths.values_mut() {
            if schedule.pending() {
                schedule.last_saved = Some(now);
                schedule.last_change = None;
                schedule.pending_since = None;
                schedule.changes = 0;
            }
        }
    }

    /// Stretched paths, for status and doctor.
    pub(crate) fn throttled(&self) -> Vec<(PathBuf, &PathSchedule)> {
        self.paths
            .iter()
            .filter(|(_, schedule)| schedule.interval > self.limits.base)
            .map(|(path, schedule)| (path.clone(), schedule))
            .collect()
    }

    pub(crate) fn get(&self, path: &Path) -> Option<&PathSchedule> {
        self.paths.get(path)
    }

    /// Drops paths that are neither pending nor saved within their reset
    /// period (a recent save is what lets the next one be recognized as
    /// churn; a stretched path past its reset period would come back at the
    /// base interval anyway, so it is forgotten rather than reported).
    pub(crate) fn prune(&mut self, now: Instant) {
        self.paths.retain(|_, schedule| {
            schedule.pending()
                || schedule.last_saved.is_some_and(|saved| {
                    now.saturating_duration_since(saved) < schedule.reset_after()
                })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn limits() -> Limits {
        Limits {
            base: secs(2),
            max: secs(24 * 3600),
        }
    }

    #[test]
    fn an_ordinary_edit_saves_after_the_base_quiet() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        schedule.note("a".into(), start);
        assert_eq!(schedule.deadline(), Some(start + secs(2)));
        assert!(schedule.due_paths(start + secs(1)).is_empty());
        assert_eq!(
            schedule.due_paths(start + secs(2)),
            vec![PathBuf::from("a")]
        );
        assert_eq!(
            schedule.saved(Path::new("a"), start + secs(2)),
            Adjustment::Unchanged
        );
        assert_eq!(schedule.get(Path::new("a")).unwrap().interval, secs(2));
    }

    #[test]
    fn churn_stretches_the_interval_and_keeps_periodic_saves() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from("state.json");
        let mut now = start;
        let mut saves = 0;
        // one change per second for a long time: saves keep happening, ever
        // more rarely
        for tick in 0..2000u64 {
            now = start + secs(tick);
            schedule.note(path.clone(), now);
            if schedule.due_paths(now).contains(&path) {
                schedule.saved(&path, now);
                saves += 1;
            }
        }
        let interval = schedule.get(&path).unwrap().interval;
        assert!(interval >= secs(256), "interval only reached {interval:?}");
        assert!(saves < 30, "{saves} saves for 2000 changes");
        // still pending and still due periodically, never later than the interval
        let due = schedule.get(&path).unwrap().due(schedule.limits()).unwrap();
        assert!(due <= now + interval);
    }

    #[test]
    fn a_settled_file_is_captured_promptly() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from("state.json");
        for tick in 0..600u64 {
            let now = start + secs(tick);
            schedule.note(path.clone(), now);
            if schedule.due_paths(now).contains(&path) {
                schedule.saved(&path, now);
            }
        }
        let interval = schedule.get(&path).unwrap().interval;
        assert!(interval >= secs(64));
        // the last change settles: due after the settle time, not the interval
        let last = start + secs(600);
        schedule.note(path.clone(), last);
        let settle = schedule.get(&path).unwrap().settle(schedule.limits());
        assert!(settle <= SETTLE_MAX && settle >= secs(2));
        assert_eq!(
            schedule.get(&path).unwrap().due(schedule.limits()),
            Some(last + settle)
        );
    }

    #[test]
    fn a_busy_path_never_delays_an_ordinary_one() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        for tick in 0..100u64 {
            schedule.note("busy".into(), start + secs(tick));
            for path in schedule.due_paths(start + secs(tick)) {
                schedule.saved(&path, start + secs(tick));
            }
        }
        let now = start + secs(100);
        schedule.note("quiet".into(), now);
        schedule.note("busy".into(), now);
        assert_eq!(
            schedule.due_paths(now + secs(2)),
            vec![PathBuf::from("quiet")]
        );
        assert!(
            schedule
                .held_paths(now + secs(2))
                .contains(&PathBuf::from("busy"))
        );
    }

    #[test]
    fn sustained_quiet_resets_but_a_brief_pause_does_not() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from("state.json");
        let mut now = start;
        for tick in 0..300u64 {
            now = start + secs(tick);
            schedule.note(path.clone(), now);
            if schedule.due_paths(now).contains(&path) {
                schedule.saved(&path, now);
            }
        }
        // drain the pending change
        let settle = schedule.get(&path).unwrap().settle(schedule.limits());
        schedule.saved(&path, now + settle);
        let stretched = schedule.get(&path).unwrap().interval;
        assert!(stretched > secs(2));
        // a brief pause: still stretched
        schedule.note(path.clone(), now + settle + secs(60));
        assert_eq!(schedule.get(&path).unwrap().interval, stretched);
        schedule.saved(&path, now + settle + secs(62));
        // a long quiet: back to base on the next change
        let reset_after = schedule.get(&path).unwrap().reset_after();
        schedule.note(path.clone(), now + settle + secs(62) + reset_after);
        assert_eq!(schedule.get(&path).unwrap().interval, secs(2));
    }

    #[test]
    fn an_editor_saving_every_few_seconds_is_never_stretched() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from(".zshrc");
        // a person saves from an editor every five seconds for ten minutes:
        // each change settles before the next, so nothing counts as churn
        for tick in (0..600u64).step_by(5) {
            let now = start + secs(tick);
            schedule.note(path.clone(), now);
            assert_eq!(schedule.due_paths(now + secs(2)), vec![path.clone()]);
            assert_eq!(schedule.saved(&path, now + secs(2)), Adjustment::Unchanged);
            schedule.prune(now + secs(2));
        }
        assert_eq!(schedule.get(&path).unwrap().interval, secs(2));
    }

    #[test]
    fn pruning_between_saves_keeps_what_churn_detection_needs() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from("state.json");
        // a burst: saved, pruned, and immediately busy again
        schedule.note(path.clone(), start);
        schedule.note(path.clone(), start + secs(1));
        schedule.saved(&path, start + secs(2));
        schedule.prune(start + secs(2));
        assert!(
            schedule.get(&path).is_some(),
            "a recently saved path is kept"
        );
        schedule.note(path.clone(), start + secs(3));
        schedule.note(path.clone(), start + secs(4));
        assert_eq!(
            schedule.saved(&path, start + secs(5)),
            Adjustment::Stretched
        );
        assert_eq!(schedule.get(&path).unwrap().interval, secs(4));
        // a path saved long ago is dropped
        let mut old = Schedule::new(limits());
        old.note("once".into(), start);
        old.saved(Path::new("once"), start + secs(2));
        old.prune(start + secs(2) + RESET_MIN);
        assert!(old.get(Path::new("once")).is_none());
    }

    #[test]
    fn a_restart_continues_the_schedule_where_it_stopped() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from("/tmp/state.json");
        for tick in 0..300u64 {
            let now = start + secs(tick);
            schedule.note(path.clone(), now);
            if schedule.due_paths(now).contains(&path) {
                schedule.saved(&path, now);
            }
        }
        // a change is pending and not due yet
        let stopped = start + secs(300);
        schedule.note(path.clone(), stopped);
        let pending_before = schedule.get(&path).unwrap().changes;
        let due_before = schedule.get(&path).unwrap().due(schedule.limits()).unwrap();
        assert!(due_before > stopped + secs(2));
        let persisted = schedule.persist(stopped, 1_000_000);
        // ten seconds later the watcher is back: the same save is due at the
        // same moment, the last save is remembered, and the path is held
        let restarted_at = stopped + secs(10);
        let mut restarted = Schedule::new(limits());
        restarted.restore(&persisted, restarted_at, 1_000_010);
        let restored = restarted.get(&path).unwrap();
        assert_eq!(restored.changes, pending_before);
        assert!(restored.last_saved.is_some());
        assert_eq!(restored.due(restarted.limits()), Some(due_before));
        assert!(restarted.held_paths(restarted_at).contains(&path));
        assert!(restarted.due_paths(restarted_at).is_empty());
        // an overdue save is due at once
        let mut late = Schedule::new(limits());
        late.restore(
            &persisted,
            due_before + secs(1),
            1_000_000 + 1 + (due_before - stopped).as_secs(),
        );
        assert!(late.due_paths(due_before + secs(1)).contains(&path));
    }

    #[test]
    fn new_limits_clamp_every_interval() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from("state.json");
        for tick in 0..600u64 {
            let now = start + secs(tick);
            schedule.note(path.clone(), now);
            if schedule.due_paths(now).contains(&path) {
                schedule.saved(&path, now);
            }
        }
        assert!(schedule.get(&path).unwrap().interval > secs(16));
        schedule.set_limits(Limits {
            base: secs(2),
            max: secs(16),
        });
        assert_eq!(schedule.get(&path).unwrap().interval, secs(16));
        assert!(schedule.is_throttled(&path));
    }

    #[test]
    fn a_throttled_path_past_its_reset_period_is_forgotten() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from("state.json");
        for tick in 0..300u64 {
            let now = start + secs(tick);
            schedule.note(path.clone(), now);
            if schedule.due_paths(now).contains(&path) {
                schedule.saved(&path, now);
            }
        }
        let settle = schedule.get(&path).unwrap().settle(schedule.limits());
        schedule.saved(&path, start + secs(300) + settle);
        let reset_after = schedule.get(&path).unwrap().reset_after();
        schedule.prune(start + secs(300) + settle + reset_after - secs(1));
        assert!(schedule.is_throttled(&path));
        schedule.prune(start + secs(300) + settle + reset_after);
        assert!(schedule.get(&path).is_none());
    }

    #[test]
    fn persisted_intervals_survive_a_restart_unless_quiet_long_enough() {
        let start = Instant::now();
        let mut schedule = Schedule::new(limits());
        let path = PathBuf::from("/tmp/state.json");
        for tick in 0..300u64 {
            let now = start + secs(tick);
            schedule.note(path.clone(), now);
            if schedule.due_paths(now).contains(&path) {
                schedule.saved(&path, now);
            }
        }
        let persisted = schedule.persist(start + secs(300), 1_000_000);
        let record = persisted.paths.get("/tmp/state.json").unwrap();
        assert!(record.interval_secs > 2);

        let mut restarted = Schedule::new(limits());
        restarted.restore(&persisted, start + secs(360), 1_000_060);
        assert_eq!(
            restarted.get(&path).unwrap().interval,
            secs(record.interval_secs)
        );

        let mut later = Schedule::new(limits());
        later.restore(
            &persisted,
            start + secs(300 + 24 * 3600),
            1_000_000 + 24 * 3600,
        );
        assert!(later.get(&path).is_none());
    }
}
