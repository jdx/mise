//! The foreground watcher process behind `mise bootstrap dotfiles watch` and
//! the `history-watch` built-in service: installs filesystem watches for the
//! tracked set, schedules each changed path on its own (a constantly
//! rewritten file is saved ever more rarely, never floods the history, and
//! never delays an ordinary edit), saves checkpoints, and persists its
//! health for `mise doctor` and `mise bootstrap dotfiles status`. Captures
//! never wait on the network and never run while another history operation
//! holds the operation lock; they are deferred and retried.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eyre::{Result, bail};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer_opt};
use serde_json::json;
use tokio::sync::mpsc;

use super::noise::{self, NoisyPath, NoisyRecord};
use super::plan::{Anchor, Mode, PathKind, WatchPlan};
use super::schedule::{self, Adjustment, Limits, PersistedSchedule, Schedule};
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::lock_file::LockFile;
use crate::system::history::checkpoint::{Draft, Outcome, Store};
use crate::system::history::health::{self, Health, ThrottledPath};
use crate::system::history::store::{self, Trigger};
use crate::system::history::tracked::{
    self, ExcludeSet, TrackedSet, hard_exclusions, normalize, normalize_target,
};

/// How long the debouncer coalesces raw filesystem events before they reach
/// the scheduler, which applies the configured quiet period on top.
const COALESCE: Duration = Duration::from_millis(500);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(5 * 60);
/// What a final capture does with throttled files: everything live when
/// the watcher stops for good, the schedule respected when the service is
/// about to restart it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Restart {
    Final,
    Held,
}

/// How often a start retries the watch lock a status probe may be holding
/// for a moment.
const WATCH_LOCK_TRIES: u32 = 5;
const WATCH_LOCK_RETRY: Duration = Duration::from_millis(200);

/// How often, and how many times, the shutdown capture waits for a running
/// history operation to finish before giving up.
const SHUTDOWN_RETRY_EVERY: Duration = Duration::from_secs(1);
const SHUTDOWN_RETRIES: usize = 10;

/// What became of a capture attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attempt {
    /// It ran (a checkpoint was written, or nothing had changed).
    Done,
    /// Another history operation holds the lock; retried later.
    Deferred,
    /// It failed; retried after the backoff.
    Failed,
}

pub(crate) struct WatchOptions {
    /// Reconcile once and exit.
    pub once: bool,
    /// One JSON object per line instead of log lines.
    pub json: bool,
}

/// Runs the watcher; returns the process exit code.
pub(crate) async fn run(opts: WatchOptions) -> Result<i32> {
    let out = Output { json: opts.json };
    if !Settings::get().history.enabled {
        out.emit(
            "disabled",
            "history is disabled (history.enabled = false)",
            json!({}),
        );
        return Ok(0);
    }
    let store = Store::open()?;
    if let Some(reason) = store.unavailable() {
        out.emit("unavailable", &format!("cannot watch: {reason}"), json!({}));
        return Ok(1);
    }
    // a status probe (`doctor`, `status`, `track`) takes the lock for a
    // moment to see whether a watcher holds it: a start that lands on that
    // moment tries again rather than concluding another watcher runs
    let mut watch_lock = None;
    for attempt in 0..WATCH_LOCK_TRIES {
        if let Some(lock) = LockFile::new(&watch_lock_in(store.state_dir())).try_lock()? {
            watch_lock = Some(lock);
            break;
        }
        if attempt + 1 < WATCH_LOCK_TRIES {
            tokio::time::sleep(WATCH_LOCK_RETRY).await;
        }
    }
    let Some(_watch_lock) = watch_lock else {
        out.emit(
            "already-running",
            "another watcher is running for this store",
            json!({}),
        );
        return Ok(0);
    };
    let settings = Settings::get();
    let mut intervals = Intervals::from_settings(&settings);
    let mut state = State::load().await?;
    let mut capture = Capture::new(store, out, intervals.limits.clone());
    capture.health.watcher.started_at = Some(store::now_rfc3339());
    if opts.once {
        // A one-shot capture has no installed filesystem watches. Failures
        // from a previous watch installation do not describe this run.
        capture.health.watcher.degraded.clear();
        // the restored schedule applies to this capture too: a throttled
        // file whose save is not due is held, not read live
        let outcome = capture.reconcile(&state.tracked, "startup reconcile");
        capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
        capture.write_health();
        return Ok(match outcome {
            Attempt::Done => 0,
            Attempt::Deferred => {
                capture.out.emit(
                    "unsaved",
                    "nothing was saved: another history operation is running; run again once it finished",
                    json!({ "reason": "deferred" }),
                );
                1
            }
            Attempt::Failed => {
                capture.out.emit(
                    "unsaved",
                    &format!(
                        "nothing was saved: {}",
                        capture
                            .health
                            .watcher
                            .last_error
                            .as_deref()
                            .unwrap_or("the capture failed")
                    ),
                    json!({ "reason": "failed" }),
                );
                1
            }
        });
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
        COALESCE,
        None,
        move |result| {
            let _ = tx.send(result);
        },
        NoCache,
        notify::Config::default(),
    )?;
    let mut installed = match install(&mut debouncer, &[], &state.plan.anchors, &mut capture) {
        Ok(installed) if !installed.is_empty() => installed,
        outcome => {
            // what changed while the watcher was down is saved before it
            // gives up on watching (waiting a moment for a running
            // operation), and why it gave up, and whether that final save
            // happened, is on record for `doctor` and `status`
            let err = match outcome {
                Ok(_) => eyre::eyre!("no watch could be installed for the tracked set"),
                Err(err) => err,
            };
            stop_after_install_failure(&mut capture, &state.tracked, &err, "installed").await;
            debouncer.stop();
            return Ok(1);
        }
    };
    // the first capture comes after the watches are in place, so an edit
    // landing between the two reaches the scheduler instead of waiting for
    // the next reconcile
    capture.reconcile(&state.tracked, "startup reconcile");
    capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
    capture.out.emit(
        "started",
        &format!(
            "watching {} anchor(s) for {} tracked entr{}",
            installed.len(),
            state.tracked.entries.len(),
            if state.tracked.entries.len() == 1 {
                "y"
            } else {
                "ies"
            }
        ),
        json!({ "anchors": installed.len(), "pending": state.plan.pending.len() }),
    );
    capture.write_health();

    let mut shutdown = Shutdown::new()?;
    let mut next_reconcile = intervals
        .reconcile
        .map(|every| tokio::time::Instant::now() + every);
    loop {
        // the next save, or the retry of a deferred or failed capture,
        // whichever comes first
        let flush_at = match (
            capture.schedule.deadline().map(|at| capture.not_before(at)),
            capture.retry_due(),
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let flush = async {
            match flush_at {
                Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
                None => std::future::pending::<()>().await,
            }
        };
        let reconcile = async {
            match next_reconcile {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            received = rx.recv() => {
                let Some(result) = received else {
                    capture.out.emit(
                        "error",
                        "the filesystem watch stopped delivering events; stopping so the service restarts it",
                        json!({ "message": "watch channel closed" }),
                    );
                    // the service restarts the watcher: throttled files
                    // stay held, so a failure that persists does not save
                    // them on every restart
                    finish(&mut capture, &state.tracked, Restart::Held).await;
                    // Transport failure is not a capture failure: finish may
                    // have saved successfully. Preserve its actual outcome.
                    capture.health.watcher.degraded.push("the filesystem watch stopped".into());
                    capture.write_health();
                    debouncer.stop();
                    return Ok(1);
                };
                let now = Instant::now();
                let mut config_changed = false;
                let mut rescan = false;
                let mut pending_appeared = false;
                // a watched directory itself changed (replaced, recreated,
                // renamed): its watch may be dead
                let mut anchor_changed = false;
                let mut throttled_changed = false;
                match result {
                    Ok(events) => {
                        for event in events {
                            trace!("history watch: {:?} {:?}", event.kind, event.paths);
                            if event.kind.is_access() {
                                continue;
                            }
                            if event.need_rescan() {
                                rescan = true;
                            }
                            for path in &event.paths {
                                // the parent resolved, the final link kept:
                                // a tracked link is scheduled as the link
                                let path = normalize_target(path);
                                if state.is_config_file(&path) {
                                    config_changed = true;
                                }
                                // a tracked path that did not exist is watched
                                // through an ancestor: something appearing on
                                // the way to it means the plan can move closer
                                if state.plan.pending.iter().any(|pending| pending.starts_with(&path)) {
                                    pending_appeared = true;
                                }
                                if installed.iter().any(|anchor| anchor.path == path) {
                                    anchor_changed = true;
                                }
                                // a link that appeared or changed may point
                                // somewhere new: the derived entries follow
                                if path.is_symlink() && state.relevant(&path) {
                                    pending_appeared = true;
                                }
                                if !state.relevant(&path) {
                                    debug!("history watch: ignoring {}", path.display());
                                    continue;
                                }
                                // files are scheduled, never directories: a
                                // held directory would hold everything in it
                                if path.is_dir() && !path.is_symlink() {
                                    continue;
                                }
                                capture.schedule.note(path.clone(), now);
                                if capture.schedule.is_throttled(&path) {
                                    throttled_changed = true;
                                }
                            }
                        }
                    }
                    Err(errors) => {
                        for err in errors {
                            capture.out.emit("error", &format!("watch error: {err}"), json!({ "message": err.to_string() }));
                        }
                    }
                }
                // a throttled file's unsaved changes are visible to status
                // and doctor as they happen, not only after its next save
                if throttled_changed {
                    capture.persist_schedule();
                    capture.write_health();
                }
                if config_changed {
                    match state.reload().await {
                        Ok(true) => {
                            // Config and root replacement events can share a
                            // batch. Recreate watches even for retained paths,
                            // whose old inode may no longer exist.
                            installed = match reinstall(&mut debouncer, &installed, &state.plan.anchors, &mut capture) {
                        Ok(installed) => installed,
                        Err(err) => {
                            stop_after_install_failure(&mut capture, &state.tracked, &err, "re-installed").await;
                            debouncer.stop();
                            return Ok(1);
                        }
                    };
                            // the timing settings may have changed with it
                            apply_intervals(&mut capture, &mut intervals, &mut next_reconcile);
                            capture.out.emit(
                                "replan",
                                &format!("configuration changed; watching {} anchor(s)", installed.len()),
                                json!({ "anchors": installed.len() }),
                            );
                            // a path the new configuration no longer
                            // autosaves (excluded, untracked, switched to
                            // manual saving) leaves the schedule: no capture
                            // from now on holds it or carries its old
                            // version forward as if it were still eligible.
                            // A path that is missing right now (a symlink
                            // target between two versions, say) keeps its
                            // throttling while something still declares it;
                            // one nothing declares any more leaves like any
                            // other
                            prune_schedule(&mut capture, &state);
                            // the configuration that changed is what this
                            // capture is for: never held back
                            let config_dir = state.config_dir.clone();
                            let held: Vec<PathBuf> = capture
                                .schedule
                                .held_paths(now)
                                .into_iter()
                                .filter(|path| !path.starts_with(&config_dir))
                                .collect();
                            if capture.attempt(&state.tracked, "configuration changed", &held) == Attempt::Done {
                                for path in capture.schedule.due_paths(now).into_iter().chain(
                                    capture
                                        .schedule
                                        .held_paths(now)
                                        .into_iter()
                                        .filter(|path| path.starts_with(&config_dir)),
                                ) {
                                    capture.schedule.saved(&path, now);
                                }
                                capture.schedule.prune(now);
                                capture.persist_schedule();
                            }
                            capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
                            capture.write_health();
                        }
                        Ok(false) => {
                            stop_disabled(&mut capture, &state.tracked).await;
                            debouncer.stop();
                            return Ok(0);
                        }
                        Err(err) => capture.out.emit(
                            "error",
                            &format!("configuration could not be reloaded; keeping the previous tracked set: {err:#}"),
                            json!({ "message": format!("{err:#}") }),
                        ),
                    }
                } else if rescan {
                    // the backend lost track: every watch is made anew
                    match state.reload().await {
                        Ok(true) => {
                            installed = match reinstall(&mut debouncer, &installed, &state.plan.anchors, &mut capture) {
                                Ok(installed) => installed,
                                Err(err) => {
                                    stop_after_install_failure(&mut capture, &state.tracked, &err, "re-installed").await;
                                    debouncer.stop();
                                    return Ok(1);
                                }
                            };
                            apply_intervals(&mut capture, &mut intervals, &mut next_reconcile);
                            prune_schedule(&mut capture, &state);
                        }
                        Ok(false) => {
                            stop_disabled(&mut capture, &state.tracked).await;
                            debouncer.stop();
                            return Ok(0);
                        }
                        Err(err) => capture.out.emit(
                            "error",
                            &format!("configuration could not be reloaded; keeping the previous tracked set: {err:#}"),
                            json!({ "message": format!("{err:#}") }),
                        ),
                    }
                    capture.reconcile(&state.tracked, "rescan");
                    capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
                    capture.write_health();
                } else if pending_appeared || anchor_changed {
                    match state.reload().await {
                        Ok(true) => {}
                        Ok(false) => {
                            stop_disabled(&mut capture, &state.tracked).await;
                            debouncer.stop();
                            return Ok(0);
                        }
                        Err(err) => {
                            capture.out.emit(
                                "error",
                                &format!("configuration could not be reloaded; keeping the previous tracked set: {err:#}"),
                                json!({ "message": format!("{err:#}") }),
                            );
                            continue;
                        }
                    }
                    apply_intervals(&mut capture, &mut intervals, &mut next_reconcile);
                    // a replaced directory keeps its path but not its
                    // watch: an anchor that changed is watched anew
                    installed = match if anchor_changed {
                        reinstall(&mut debouncer, &installed, &state.plan.anchors, &mut capture)
                    } else {
                        install(&mut debouncer, &installed, &state.plan.anchors, &mut capture)
                    } {
                        Ok(installed) => installed,
                        Err(err) => {
                            stop_after_install_failure(&mut capture, &state.tracked, &err, "re-installed").await;
                            debouncer.stop();
                            return Ok(1);
                        }
                    };
                    // what the new set no longer covers (a link's old
                    // target, say) leaves the schedule
                    prune_schedule(&mut capture, &state);
                    // an edit that landed while the watches were being
                    // remade is saved now, not at the next reconciliation
                    if anchor_changed {
                        capture.reconcile(&state.tracked, "watches reinstalled");
                        capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
                    }
                    capture.out.emit(
                        "replan",
                        &format!("a tracked path appeared; watching {} anchor(s)", installed.len()),
                        json!({ "anchors": installed.len(), "pending": state.plan.pending.len() }),
                    );
                    capture.write_health();
                }
            }
            _ = flush => {
                let now = Instant::now();
                let due = capture.schedule.due_paths(now);
                let retrying = capture.retry_due().is_some_and(|at| at <= now);
                if !due.is_empty() || retrying {
                    let held = capture.schedule.held_paths(now);
                    let reason = if due.is_empty() {
                        "retry".to_string()
                    } else {
                        describe(&due)
                    };
                    let done = capture.attempt(&state.tracked, &reason, &held) == Attempt::Done;
                    if done {
                        for path in &due {
                            match capture.schedule.saved(path, now) {
                                Adjustment::Stretched => {
                                    let interval = capture.schedule.get(path).map(|s| s.interval).unwrap_or_default();
                                    capture.out.emit(
                                        "throttled",
                                        &format!(
                                            "{} keeps changing; saving it every {} now (up to {}). Exclude it with `mise bootstrap dotfiles exclude '{}'` if it is a log, cache, or database, or track it with `--no-autosave` and save it explicitly",
                                            display_path(path),
                                            humantime(interval),
                                            humantime(capture.schedule.limits().max),
                                            display_path(path)
                                        ),
                                        json!({ "path": display_path(path), "interval_secs": interval.as_secs() }),
                                    );
                                }
                                Adjustment::Reset => capture.out.emit(
                                    "settled",
                                    &format!("{} settled; saving it promptly again", display_path(path)),
                                    json!({ "path": display_path(path) }),
                                ),
                                Adjustment::Unchanged => {}
                            }
                        }
                        capture.schedule.prune(now);
                        capture.persist_schedule();
                        capture.write_health();
                    }
                }
            }
            _ = reconcile => {
                if let Some(every) = intervals.reconcile {
                    next_reconcile = Some(tokio::time::Instant::now() + every);
                }
                // the tracked set and every watch are made anew: a pending
                // path that appeared is watched, a replaced directory's dead
                // watch is replaced, and what the set no longer covers
                // leaves the schedule
                match state.reload().await {
                    Ok(true) => {
                        installed = match reinstall(&mut debouncer, &installed, &state.plan.anchors, &mut capture) {
                            Ok(installed) => installed,
                            Err(err) => {
                                stop_after_install_failure(&mut capture, &state.tracked, &err, "re-installed").await;
                                debouncer.stop();
                                return Ok(1);
                            }
                        };
                        apply_intervals(&mut capture, &mut intervals, &mut next_reconcile);
                        prune_schedule(&mut capture, &state);
                    }
                    Ok(false) => {
                        stop_disabled(&mut capture, &state.tracked).await;
                        debouncer.stop();
                        return Ok(0);
                    }
                    Err(err) => capture.out.emit(
                        "error",
                        &format!("configuration could not be reloaded; keeping the previous tracked set: {err:#}"),
                        json!({ "message": format!("{err:#}") }),
                    ),
                }
                capture.reconcile(&state.tracked, "reconcile");
                capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
                capture.write_health();
            }
            _ = shutdown.wait() => {
                finish(&mut capture, &state.tracked, Restart::Final).await;
                break;
            }
        }
    }
    debouncer.stop();
    Ok(0)
}

/// The final capture before the process ends: a full capture, not only the
/// due paths (a change still inside the coalescing window has not reached
/// the scheduler yet, and a throttled file's final state is saved now). The
/// backoff does not apply, and a running operation is given a moment.
/// The watches could not be re-installed after a replan: what is pending is
/// saved and the failure recorded before the process exits, so the service
/// restarts it and status says why.
/// Stops after the watches could not be `installed` (at startup) or
/// `re-installed` (a replan): a final capture first, then the reason on
/// record, including a final capture that could not run.
async fn stop_after_install_failure(
    capture: &mut Capture,
    tracked: &TrackedSet,
    err: &eyre::Report,
    phase: &str,
) {
    capture.out.emit(
        "error",
        &format!("the watches could not be {phase}; stopping so the service restarts it: {err:#}"),
        json!({ "message": format!("{err:#}") }),
    );
    let saved = finish(capture, tracked, Restart::Held).await;
    // recorded after the final capture, which would clear it
    let unsaved = match saved {
        Attempt::Done => String::new(),
        Attempt::Deferred => {
            "; the final capture did not run: another history operation held the lock".to_string()
        }
        Attempt::Failed => "; the final capture failed".to_string(),
    };
    capture.health.watcher.last_error = Some(format!(
        "the watches could not be {phase}: {err:#}{unsaved}"
    ));
    capture.health.watcher.last_error_at = Some(store::now_rfc3339());
    capture.health.watcher.consecutive_failures += 1;
    capture.write_health();
}

/// Returns how the final capture went.
/// The final capture before the process ends. A stop for good (a signal,
/// history switched off) saves everything live, a throttled file's final
/// state included. A stop the service will undo by restarting the watcher
/// (`Restart::Held`, the watches could not be installed) keeps holding
/// throttled files: a failure that persists would otherwise save them on
/// every restart.
async fn finish(capture: &mut Capture, tracked: &TrackedSet, restart: Restart) -> Attempt {
    // a full capture, not only the due paths: a change still
    // inside the coalescing window has not reached the scheduler
    // yet. The backoff does not apply, and a running operation is
    // given a moment to finish
    let now = Instant::now();
    let held = match restart {
        Restart::Held => capture.schedule.held_paths(now),
        Restart::Final => vec![],
    };
    capture.retry_at = None;
    let mut outcome = capture.attempt(tracked, "shutdown", &held);
    for _ in 0..SHUTDOWN_RETRIES {
        if outcome != Attempt::Deferred {
            break;
        }
        tokio::time::sleep(SHUTDOWN_RETRY_EVERY).await;
        capture.retry_at = None;
        outcome = capture.attempt(tracked, "shutdown", &held);
    }
    if outcome == Attempt::Done {
        match restart {
            Restart::Final => capture.schedule.clear_pending(now),
            Restart::Held => {
                for path in capture.schedule.due_paths(now) {
                    capture.schedule.saved(&path, now);
                }
                capture.schedule.prune(now);
            }
        }
    } else {
        let pending =
            capture.schedule.held_paths(now).len() + capture.schedule.due_paths(now).len();
        capture.out.emit(
            "unsaved",
            &format!("stopping with {pending} pending path(s) unsaved; the next start saves them"),
            json!({ "pending": pending }),
        );
    }
    capture.persist_schedule();
    capture.out.emit("stopped", "stopping", json!({}));
    capture.write_health();
    outcome
}

fn describe(paths: &[PathBuf]) -> String {
    let mut names: Vec<String> = paths.iter().map(display_path).collect();
    names.sort();
    let extra = names.len().saturating_sub(3);
    names.truncate(3);
    if extra > 0 {
        format!("{} +{extra} more changed", names.join(", "))
    } else {
        format!("{} changed", names.join(", "))
    }
}

pub(crate) fn humantime(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Intervals {
    limits: Limits,
    reconcile: Option<Duration>,
}

impl Intervals {
    fn from_settings(settings: &Settings) -> Self {
        let parse = |name: &str, value: &str, default: Duration| {
            crate::duration::parse_duration(value).unwrap_or_else(|err| {
                warn!("history.watch.{name}: {err}; using {default:?}");
                default
            })
        };
        let reconcile = parse(
            "reconcile",
            &settings.history.watch.reconcile,
            Duration::from_secs(600),
        );
        Self {
            limits: Limits {
                base: parse(
                    "debounce",
                    &settings.history.watch.debounce,
                    Duration::from_secs(2),
                ),
                max: parse(
                    "max_interval",
                    &settings.history.watch.max_interval,
                    Duration::from_secs(24 * 3600),
                ),
            },
            reconcile: (!reconcile.is_zero()).then_some(reconcile),
        }
    }
}

/// The tracked set, its watch plan, and the filters applied to events.
struct State {
    /// The declared set, what captures walk.
    tracked: TrackedSet,
    /// The declared set plus derived entries, what is watched.
    watched: TrackedSet,
    /// Links inside tracked directories seen by a walk (a link whose target
    /// is missing now derives nothing, but still declares where it points,
    /// wherever that is by now).
    derived_links: Vec<PathBuf>,
    plan: WatchPlan,
    exclude: ExcludeSet,
    hard: Vec<PathBuf>,
    config_dir: PathBuf,
}

impl State {
    async fn load() -> Result<Self> {
        let tracked = TrackedSet::effective().await?;
        Self::from_tracked(tracked)
    }

    fn from_tracked(tracked: TrackedSet) -> Result<Self> {
        let exclude = tracked.exclude_set()?;
        let watched = watched_set(&tracked)?;
        let plan = build_plan(&watched);
        let derived_links = watched
            .entries
            .iter()
            .filter(|entry| entry.kind == tracked::EntryKind::Derived)
            .filter_map(|entry| entry.source.clone())
            .collect();
        Ok(Self {
            tracked,
            watched,
            derived_links,
            plan,
            exclude,
            hard: hard_exclusions(),
            config_dir: normalize(&tracked::global_config_dir()),
        })
    }

    /// Reloads the configuration; `Ok(false)` when history was disabled.
    async fn reload(&mut self) -> Result<bool> {
        Config::reset().await?;
        if !Settings::get().history.enabled {
            return Ok(false);
        }
        let tracked = TrackedSet::effective().await?;
        let mut fresh = Self::from_tracked(tracked)?;
        // a link whose target is between two versions derives nothing
        // right now; the link is remembered while it is one, and consulted
        // for where it points now (it may have been retargeted meanwhile)
        for link in self.derived_links.drain(..) {
            if link.is_symlink() && !fresh.derived_links.contains(&link) {
                fresh.derived_links.push(link);
            }
        }
        *self = fresh;
        Ok(true)
    }

    /// A change to a mise configuration file: the tracked set and the
    /// settings may differ now.
    fn is_config_file(&self, path: &Path) -> bool {
        path.starts_with(&self.config_dir)
            && (path.extension().is_some_and(|ext| ext == "toml")
                || path
                    .components()
                    .any(|component| component.as_os_str() == "conf.d"))
    }

    /// Whether a change to `path` is one the watcher saves: under a
    /// declared or derived entry (a symlink target inside the home
    /// directory), autosaved, and not excluded.
    fn relevant(&self, path: &Path) -> bool {
        if self.hard.iter().any(|dir| path.starts_with(dir)) {
            return false;
        }
        if path
            .components()
            .any(|component| component.as_os_str() == ".git")
        {
            return false;
        }
        if self.exclude.is_match(path) {
            return false;
        }
        match self.watched.entry_for(path) {
            Some(entry) => entry.policy.autosave,
            None => false,
        }
    }

    /// Whether a path that does not exist right now may still be one the
    /// watcher saves once it is back: under a declared autosave entry, or
    /// where a tracked symlink (through any links on the way) points, its
    /// target between two versions, say. A path nothing declares for
    /// automatic saving any more is not kept for being missing.
    fn may_cover_missing(&self, path: &Path) -> bool {
        if self.hard.iter().any(|dir| path.starts_with(dir)) || self.exclude.is_match(path) {
            return false;
        }
        if self
            .watched
            .entry_for(path)
            .is_some_and(|entry| entry.policy.autosave && entry.kind != tracked::EntryKind::Derived)
            || self.tracked.entry_for(path).is_some_and(|entry| {
                entry.policy.autosave
                    && !tracked::is_refused_root(&entry.path, &normalize(&crate::dirs::HOME))
            })
        {
            return true;
        }
        if self.watched.entries.iter().any(|entry| {
            entry.policy.autosave
                && entry.path.is_symlink()
                && tracked::link_target(&entry.path).is_some_and(|target| path.starts_with(target))
        }) {
            return true;
        }
        // a link inside a tracked directory that points there now (a
        // retargeted link declares its new target, not the old one)
        self.derived_links.iter().any(|link| {
            link.is_symlink()
                && self.relevant(link)
                && tracked::link_target(link).is_some_and(|now| path.starts_with(now))
        })
    }
}

/// The set the watcher plans and filters by: the declared entries plus the
/// derived ones the walk discovers (targets of tracked symlinks).
fn watched_set(tracked: &TrackedSet) -> Result<TrackedSet> {
    let walk = tracked.walk()?;
    let mut watched = tracked.clone();
    // an entry the walker refuses (the home directory or above) captures
    // nothing, so it is not watched either: a watch there would schedule
    // the whole tree for captures that cannot store it
    let home = normalize(&crate::dirs::HOME);
    watched.entries = walk
        .entries
        .into_iter()
        .filter(|entry| !tracked::is_refused_root(&entry.path, &home))
        .collect();
    Ok(watched)
}

fn build_plan(tracked: &TrackedSet) -> WatchPlan {
    let paths = tracked
        .entries
        .iter()
        .filter(|entry| entry.policy.autosave)
        .map(|entry| {
            let kind = match std::fs::symlink_metadata(&entry.path) {
                Ok(meta) if meta.is_dir() => PathKind::Directory,
                Ok(_) => PathKind::File,
                Err(_) => PathKind::Missing,
            };
            (entry.path.clone(), kind)
        });
    WatchPlan::build(paths, |path| {
        path.ancestors()
            .skip(1)
            .find(|ancestor| ancestor.is_dir())
            .map(Path::to_path_buf)
    })
}

/// Installs the plan's anchors, removing the ones no longer wanted.
/// Returns the anchors now installed.
/// History was switched off while the watcher ran: what is still pending
/// is saved under the set that was in force, like any stop.
async fn stop_disabled(capture: &mut Capture, tracked: &TrackedSet) {
    capture
        .out
        .emit("disabled", "history was disabled; stopping", json!({}));
    finish(capture, tracked, Restart::Final).await;
}

/// The timing settings as they are now, after a reload of the
/// configuration: the schedule's limits and the reconciliation timer follow
/// an edit to `history.watch.*` without a restart.
fn apply_intervals(
    capture: &mut Capture,
    intervals: &mut Intervals,
    next_reconcile: &mut Option<tokio::time::Instant>,
) {
    let fresh = Intervals::from_settings(&Settings::get());
    if fresh.limits != *capture.schedule.limits() {
        capture.schedule.set_limits(fresh.limits.clone());
    }
    if fresh.reconcile != intervals.reconcile {
        *next_reconcile = fresh
            .reconcile
            .map(|every| tokio::time::Instant::now() + every);
    }
    *intervals = fresh;
}

/// Drops from the schedule what the tracked set no longer covers (excluded,
/// untracked, switched to manual saving, a link's old target): no capture
/// from now on holds it or carries its old version forward. A path that is
/// missing right now keeps its throttling while something still declares
/// it.
fn prune_schedule(capture: &mut Capture, state: &State) {
    capture
        .schedule
        .retain(|path| state.relevant(path) || (!path.exists() && state.may_cover_missing(path)));
    capture.persist_schedule();
}

/// Every watch anew: the one on a directory that was replaced or recreated
/// keeps its path but is dead, and only a fresh watch on the new inode
/// delivers events again.
fn reinstall(
    debouncer: &mut Debouncer<RecommendedWatcher, NoCache>,
    installed: &[Anchor],
    wanted: &[Anchor],
    capture: &mut Capture,
) -> Result<Vec<Anchor>> {
    for anchor in installed {
        if let Err(err) = debouncer.unwatch(&anchor.path) {
            debug!("history watch: unwatch {}: {err}", anchor.path.display());
        }
    }
    install(debouncer, &[], wanted, capture)
}

fn install(
    debouncer: &mut Debouncer<RecommendedWatcher, NoCache>,
    installed: &[Anchor],
    wanted: &[Anchor],
    capture: &mut Capture,
) -> Result<Vec<Anchor>> {
    let mut current: Vec<Anchor> = vec![];
    capture.health.watcher.degraded.clear();
    for anchor in installed {
        if wanted.contains(anchor) {
            current.push(anchor.clone());
        } else if let Err(err) = debouncer.unwatch(&anchor.path) {
            debug!("history watch: unwatch {}: {err}", anchor.path.display());
        }
    }
    for anchor in wanted {
        if current.contains(anchor) {
            continue;
        }
        let mode = match anchor.mode {
            Mode::Recursive => RecursiveMode::Recursive,
            Mode::Flat => RecursiveMode::NonRecursive,
        };
        match debouncer.watch(&anchor.path, mode) {
            Ok(()) => current.push(anchor.clone()),
            Err(err) if matches!(err.kind, notify::ErrorKind::MaxFilesWatch) => {
                if current.is_empty() {
                    bail!(
                        "cannot watch {}: the system's watch limit is reached (on Linux raise fs.inotify.max_user_watches)",
                        display_path(&anchor.path)
                    );
                }
                let message = format!(
                    "cannot watch {}: the system's watch limit is reached; reconciliation still saves it (on Linux raise fs.inotify.max_user_watches)",
                    display_path(&anchor.path)
                );
                capture.health.watcher.degraded.push(message.clone());
                capture.out.emit(
                    "degraded",
                    &message,
                    json!({ "path": display_path(&anchor.path) }),
                );
            }
            Err(err) => {
                let message = format!(
                    "cannot watch {}: {err}; reconciliation still saves it",
                    display_path(&anchor.path)
                );
                capture.health.watcher.degraded.push(message.clone());
                capture.out.emit(
                    "degraded",
                    &message,
                    json!({ "path": display_path(&anchor.path), "message": err.to_string() }),
                );
            }
        }
    }
    // nothing watched while something should be: no event would ever
    // arrive, so the caller stops (and the service restarts it) instead of
    // running blind until a reconciliation that may be disabled
    if current.is_empty() && !wanted.is_empty() {
        bail!("no watch could be installed for the tracked set");
    }
    Ok(current)
}

/// Captures with the operation lock respected, failures backed off, the
/// per-path schedule applied, and health persisted.
struct Capture {
    store: Store,
    out: Output,
    schedule: Schedule,
    health: Health,
    backoff: Duration,
    retry_at: Option<Instant>,
    /// Why the last attempt did not run, while a retry is pending.
    retry_kind: Option<Attempt>,
}

impl Capture {
    fn new(store: Store, out: Output, limits: Limits) -> Self {
        let mut schedule = Schedule::new(limits);
        let persisted: PersistedSchedule =
            std::fs::read_to_string(schedule_path_in(store.state_dir()))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
        let now = Instant::now();
        let now_epoch = epoch_secs();
        schedule.restore(&persisted, now, now_epoch);
        // a throttled file rewritten while the watcher was down has a change
        // pending: held until its next save is due, like any other
        for (path, record) in &persisted.paths {
            let path = PathBuf::from(path);
            let Some(saved) = record.saved_epoch_secs else {
                continue;
            };
            let changed_since = std::fs::symlink_metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                // not strictly after: both are whole seconds, and a change
                // in the same second as the save must count as pending (a
                // false positive only holds the file until its save is due)
                .is_some_and(|modified| modified.as_secs() >= saved);
            if changed_since && schedule.get(&path).is_some_and(|s| !s.pending()) {
                schedule.mark_pending(path, now);
            }
        }
        let health = health::read(store.state_dir()).unwrap_or_default();
        Self {
            store,
            out,
            schedule,
            health,
            backoff: BACKOFF_MIN,
            retry_at: None,
            retry_kind: None,
        }
    }

    /// When a deferred or failed capture is retried, if one is pending.
    fn retry_due(&self) -> Option<Instant> {
        self.retry_kind.and(self.retry_at)
    }

    /// A whole-set capture (a reconcile or rescan) that respects the
    /// schedule: held paths are carried forward, and the paths that were due
    /// count as saved so they are not saved again at their own deadline.
    fn reconcile(&mut self, tracked: &TrackedSet, reason: &str) -> Attempt {
        let now = Instant::now();
        let held = self.schedule.held_paths(now);
        let due = self.schedule.due_paths(now);
        let outcome = self.attempt(tracked, reason, &held);
        if outcome == Attempt::Done {
            for path in &due {
                self.schedule.saved(path, now);
            }
            self.schedule.prune(now);
            self.persist_schedule();
        }
        outcome
    }

    /// A flush deadline no earlier than the current backoff allows.
    fn not_before(&self, at: Instant) -> Instant {
        match self.retry_at {
            Some(retry) if retry > at => retry,
            _ => at,
        }
    }

    /// Saves a checkpoint of the tracked set, with `held` paths carried
    /// forward from the newest checkpoint instead of read live. Returns
    /// whether the attempt ran (a deferred or failed attempt leaves its
    /// paths pending).
    fn attempt(&mut self, tracked: &TrackedSet, reason: &str, held: &[PathBuf]) -> Attempt {
        if let Some(retry) = self.retry_at
            && Instant::now() < retry
        {
            return self.retry_kind.unwrap_or(Attempt::Failed);
        }
        let operation =
            match LockFile::new(&store::operation_lock_in(self.store.state_dir())).try_lock() {
                Ok(Some(lock)) => lock,
                Ok(None) => {
                    self.out.emit(
                        "deferred",
                        "another history operation is running; saving afterwards",
                        json!({ "reason": reason }),
                    );
                    self.retry_at = Some(Instant::now() + BACKOFF_MIN);
                    self.retry_kind = Some(Attempt::Deferred);
                    return Attempt::Deferred;
                }
                Err(err) => {
                    self.fail(reason, &format!("{err:#}"));
                    return Attempt::Failed;
                }
            };
        let mut draft = Draft::new(Trigger::Edit);
        draft.held = held.to_vec();
        let result = self.store.attempt(tracked, draft);
        drop(operation);
        match result {
            Ok(Outcome::Created(entry)) => {
                self.recovered();
                self.health.watcher.last_capture = Some(store::now_rfc3339());
                self.out.emit(
                    "captured",
                    &format!(
                        "saved checkpoint {} ({reason}): {}",
                        entry.id, entry.checkpoint.description
                    ),
                    json!({ "id": entry.id, "uuid": entry.checkpoint.uuid, "description": entry.checkpoint.description, "reason": reason }),
                );
                self.retry_kind = None;
                Attempt::Done
            }
            Ok(Outcome::Unchanged) => {
                self.recovered();
                self.health.watcher.last_capture = Some(store::now_rfc3339());
                self.out.emit(
                    "unchanged",
                    &format!("nothing to save ({reason})"),
                    json!({ "reason": reason }),
                );
                self.retry_kind = None;
                Attempt::Done
            }
            Ok(Outcome::Unavailable(message)) => {
                self.fail(reason, &message);
                Attempt::Failed
            }
            Err(err) => {
                self.fail(reason, &format!("{err:#}"));
                Attempt::Failed
            }
        }
    }

    fn fail(&mut self, reason: &str, message: &str) {
        self.out.emit(
            "error",
            &format!(
                "could not save ({reason}): {message}; retrying in {:?}",
                self.backoff
            ),
            json!({ "reason": reason, "message": message, "retry_in_secs": self.backoff.as_secs() }),
        );
        self.retry_at = Some(Instant::now() + self.backoff);
        self.retry_kind = Some(Attempt::Failed);
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
        self.health.watcher.last_error = Some(message.to_string());
        self.health.watcher.last_error_at = Some(store::now_rfc3339());
        self.health.watcher.consecutive_failures += 1;
        self.write_health();
    }

    fn recovered(&mut self) {
        self.backoff = BACKOFF_MIN;
        self.retry_at = None;
        self.retry_kind = None;
        self.health.watcher.last_error = None;
        self.health.watcher.last_error_at = None;
        self.health.watcher.consecutive_failures = 0;
    }

    fn persist_schedule(&self) {
        let persisted = self.schedule.persist(Instant::now(), epoch_secs());
        let path = schedule_path_in(self.store.state_dir());
        if let Err(err) = store::write_json(&path, &persisted) {
            debug!("history watch: could not write {}: {err}", path.display());
        }
        // what `paths --noisy` lists
        let mut record = NoisyRecord::default();
        for (path, schedule) in self.schedule.throttled() {
            record.paths.insert(
                display_path(&path),
                NoisyPath {
                    interval_secs: schedule.interval.as_secs(),
                    pending_changes: schedule.changes,
                    last_seen: schedule
                        .last_seen
                        .map(|seen| rfc3339_ago(Instant::now().saturating_duration_since(seen)))
                        .unwrap_or_else(|| "unknown".into()),
                },
            );
        }
        let noisy = noisy_path_in(self.store.state_dir());
        if let Err(err) = noise::write(&noisy, &record) {
            debug!("history watch: could not write {}: {err}", noisy.display());
        }
    }

    fn write_health(&mut self) {
        let now = Instant::now();
        self.health.throttled = self
            .schedule
            .throttled()
            .into_iter()
            .map(|(path, schedule)| ThrottledPath {
                path: display_path(&path),
                interval_secs: schedule.interval.as_secs(),
                last_saved: schedule
                    .last_saved
                    .map(|saved| rfc3339_ago(now.saturating_duration_since(saved))),
                pending_changes: schedule.changes,
                heavy: schedule.interval >= schedule::HEAVY_INTERVAL,
            })
            .collect();
        if let Err(err) = health::write(self.store.state_dir(), &mut self.health) {
            debug!("history watch: could not write health: {err}");
        }
    }
}

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rfc3339_ago(ago: Duration) -> String {
    let at = chrono::Utc::now() - chrono::Duration::from_std(ago).unwrap_or_default();
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Clone, Copy)]
struct Output {
    json: bool,
}

impl Output {
    fn emit(&self, event: &str, message: &str, mut fields: serde_json::Value) {
        if self.json {
            if let Some(object) = fields.as_object_mut() {
                object.insert("event".into(), json!(event));
                object.insert("message".into(), json!(message));
                object.insert("at".into(), json!(store::now_rfc3339()));
            }
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{fields}");
            let _ = stdout.flush();
        } else {
            match event {
                "error" | "degraded" => warn!("history watch: {message}"),
                "unchanged" | "deferred" => debug!("history watch: {message}"),
                _ => info!("history watch: {message}"),
            }
        }
    }
}

/// The lock a running watcher holds; `mise bootstrap dotfiles status` reads it.
pub(crate) fn watch_lock_in(state_dir: &Path) -> PathBuf {
    store::store_dir_in(state_dir).join("watch.lock")
}

pub(crate) fn noisy_path_in(state_dir: &Path) -> PathBuf {
    store::store_dir_in(state_dir).join("noisy.json")
}

pub(crate) fn schedule_path_in(state_dir: &Path) -> PathBuf {
    store::store_dir_in(state_dir).join("watch-schedule.json")
}

/// Whether a watcher currently holds the lock for this store.
pub(crate) fn is_running(state_dir: &Path) -> bool {
    matches!(
        LockFile::new(&watch_lock_in(state_dir)).try_lock(),
        Ok(None)
    )
}

struct Shutdown {
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(unix)]
    hangup: tokio::signal::unix::Signal,
    #[cfg(windows)]
    ctrl_break: tokio::signal::windows::CtrlBreak,
}

impl Shutdown {
    fn new() -> Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            terminate: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
            #[cfg(unix)]
            hangup: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?,
            #[cfg(windows)]
            ctrl_break: tokio::signal::windows::ctrl_break()?,
        })
    }

    async fn wait(&mut self) {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = self.terminate.recv() => {}
                _ = self.hangup.recv() => {}
            }
        }
        #[cfg(windows)]
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = self.ctrl_break.recv() => {}
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::system::files::{FileMode, FilePolicy};
    use crate::system::history::tracked::{EntryKind, TrackedEntry};

    fn state_of(tracked: TrackedSet, config_dir: PathBuf) -> State {
        State {
            watched: tracked.clone(),
            derived_links: vec![],
            plan: build_plan(&tracked),
            exclude: tracked.exclude_set().unwrap(),
            hard: vec![],
            config_dir,
            tracked,
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_path_keeps_its_schedule_only_while_something_declares_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = normalize(dir.path());
        let hypr = root.join("hypr");
        std::fs::create_dir_all(&hypr).unwrap();
        // a tracked link whose target, through another link, is between
        // two versions
        let link = root.join("link");
        std::os::unix::fs::symlink(root.join("hop"), &link).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere/target"), root.join("hop")).unwrap();
        let policy = FilePolicy::for_mode(FileMode::Track);
        let mut tracked = TrackedSet {
            entries: vec![
                TrackedEntry::new(hypr.clone(), EntryKind::Track, "track", policy),
                TrackedEntry::new(link.clone(), EntryKind::Track, "track", policy),
            ],
            exclude: vec![format!("{}/hypr/plugins/**", root.display())],
            invalid: vec![],
        };
        let state = state_of(tracked.clone(), root.join("mise"));
        assert!(state.may_cover_missing(&hypr.join("bindings.lua")));
        assert!(state.may_cover_missing(&root.join("elsewhere/target")));
        assert!(!state.may_cover_missing(&hypr.join("plugins/state.json")));
        assert!(!state.may_cover_missing(&root.join("untracked/state.json")));

        // a link inside the tracked directory whose target went missing:
        // remembered from the last walk while the link still points there
        let inner = hypr.join("inner-link");
        std::os::unix::fs::symlink(root.join("elsewhere/inner"), &inner).unwrap();
        let mut remembered = state_of(tracked.clone(), root.join("mise"));
        remembered.derived_links = vec![inner.clone()];
        assert!(remembered.may_cover_missing(&root.join("elsewhere/inner")));
        // retargeted while its new target is missing: the new target is
        // what it declares now, the old one no longer
        std::fs::remove_file(&inner).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere/moved"), &inner).unwrap();
        assert!(remembered.may_cover_missing(&root.join("elsewhere/moved")));
        assert!(!remembered.may_cover_missing(&root.join("elsewhere/inner")));
        std::fs::remove_file(&inner).unwrap();
        assert!(!remembered.may_cover_missing(&root.join("elsewhere/moved")));

        // untracked, or switched to manual saving: nothing keeps it
        tracked.entries[0].policy.autosave = false;
        tracked.entries.pop();
        let state = state_of(tracked, root.join("mise"));
        assert!(!state.may_cover_missing(&hypr.join("bindings.lua")));
        assert!(!state.may_cover_missing(&root.join("elsewhere/target")));
    }
}
