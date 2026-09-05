//! The foreground watcher process behind `mise bootstrap dotfiles watch` and the
//! `history-watch` built-in service: installs filesystem watches for the
//! tracked set, batches changes, and saves checkpoints. Captures never wait
//! on the network and never run while another history operation holds the
//! operation lock; they are deferred and retried.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eyre::{Result, bail};
use globset::GlobSet;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer_opt};
use serde_json::json;
use tokio::sync::mpsc;

use super::batcher::Batcher;
use super::noise::{self, NoiseMonitor, NoisyPath, NoisyRecord};
use super::plan::{Anchor, Mode, PathKind, WatchPlan};
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::lock_file::LockFile;
use crate::system::history::checkpoint::{Draft, Outcome, Store};
use crate::system::history::store::{self, Trigger};
use crate::system::history::tracked::{self, TrackedSet, hard_exclusions, normalize};

/// How long the debouncer coalesces raw filesystem events before they reach
/// the batcher, which applies the configured quiet period on top.
const COALESCE: Duration = Duration::from_millis(500);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(5 * 60);

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
    let Some(_watch_lock) = LockFile::new(&watch_lock_in(store.state_dir())).try_lock()? else {
        out.emit(
            "already-running",
            "another watcher is running for this store",
            json!({}),
        );
        return Ok(0);
    };
    let settings = Settings::get();
    let intervals = Intervals::from_settings(&settings);
    let mut state = State::load().await?;
    let mut capture = Capture::new(store, out);
    capture.attempt(&state.tracked, "startup reconcile");
    if opts.once {
        return Ok(0);
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
    let mut installed = install(&mut debouncer, &[], &state.plan.anchors, &capture.out)?;
    if installed.is_empty() {
        bail!("no watch could be installed for the tracked set");
    }
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

    let mut batcher = Batcher::new(intervals.debounce, intervals.max_interval);
    let mut noise = NoiseMonitor::new();
    let mut shutdown = Shutdown::new()?;
    let mut next_reconcile = intervals
        .reconcile
        .map(|every| tokio::time::Instant::now() + every);
    loop {
        let flush_at = batcher.deadline().map(|at| capture.not_before(at));
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
                let Some(result) = received else { break };
                let now = Instant::now();
                let mut config_changed = false;
                let mut rescan = false;
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
                                let path = normalize(path);
                                if state.is_config_file(&path) {
                                    config_changed = true;
                                }
                                if !state.relevant(&path) {
                                    debug!("history watch: ignoring {}", path.display());
                                    continue;
                                }
                                if let Some(count) = noise.record(&path, now) {
                                    capture.out.emit(
                                        "noise",
                                        &format!(
                                            "{} changed {count} times in 10 minutes; exclude it with `mise bootstrap dotfiles exclude '{}'` or track it with `--no-autosave` if that is not wanted",
                                            display_path(&path),
                                            display_path(&path)
                                        ),
                                        json!({ "path": display_path(&path), "changes": count }),
                                    );
                                    capture.remember_noisy(&noise, now);
                                }
                                batcher.note(path, now);
                            }
                        }
                    }
                    Err(errors) => {
                        for err in errors {
                            capture.out.emit("error", &format!("watch error: {err}"), json!({ "message": err.to_string() }));
                        }
                    }
                }
                if config_changed {
                    match state.reload().await {
                        Ok(true) => {
                            installed = install(&mut debouncer, &installed, &state.plan.anchors, &capture.out)?;
                            capture.out.emit(
                                "replan",
                                &format!("configuration changed; watching {} anchor(s)", installed.len()),
                                json!({ "anchors": installed.len() }),
                            );
                            let pending = batcher.drain();
                            capture.attempt(&state.tracked, "configuration changed");
                            for path in pending {
                                if state.relevant(&path) {
                                    batcher.note(path, now);
                                }
                            }
                        }
                        Ok(false) => {
                            capture.out.emit("disabled", "history was disabled; stopping", json!({}));
                            return Ok(0);
                        }
                        Err(err) => capture.out.emit(
                            "error",
                            &format!("configuration could not be reloaded; keeping the previous tracked set: {err:#}"),
                            json!({ "message": format!("{err:#}") }),
                        ),
                    }
                } else if rescan {
                    batcher.drain();
                    capture.attempt(&state.tracked, "rescan");
                }
            }
            _ = flush => {
                let now = Instant::now();
                let ready = batcher.flush(now);
                if !ready.is_empty() {
                    let done = capture.attempt(&state.tracked, &describe(&ready));
                    if !done {
                        for path in ready {
                            batcher.note(path, now);
                        }
                    }
                }
                noise.prune(now);
            }
            _ = reconcile => {
                if let Some(every) = intervals.reconcile {
                    next_reconcile = Some(tokio::time::Instant::now() + every);
                }
                if state.plan.pending.iter().any(|path| path.exists())
                    && let Ok(true) = state.reload().await
                {
                    installed = install(&mut debouncer, &installed, &state.plan.anchors, &capture.out)?;
                }
                capture.attempt(&state.tracked, "reconcile");
            }
            _ = shutdown.wait() => {
                // a full reconcile, not only the batch: a change still inside
                // the coalescing window has not reached the batcher yet
                batcher.drain();
                capture.attempt(&state.tracked, "shutdown");
                capture.out.emit("stopped", "stopping", json!({}));
                break;
            }
        }
    }
    debouncer.stop();
    Ok(0)
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

struct Intervals {
    debounce: Duration,
    max_interval: Duration,
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
            debounce: parse(
                "debounce",
                &settings.history.watch.debounce,
                Duration::from_secs(2),
            ),
            max_interval: parse(
                "max_interval",
                &settings.history.watch.max_interval,
                Duration::from_secs(30),
            ),
            reconcile: (!reconcile.is_zero()).then_some(reconcile),
        }
    }
}

/// The tracked set, its watch plan, and the filters applied to events.
struct State {
    tracked: TrackedSet,
    plan: WatchPlan,
    exclude: GlobSet,
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
        let plan = build_plan(&tracked);
        Ok(Self {
            tracked,
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
        *self = Self::from_tracked(tracked)?;
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

    /// Whether a change to `path` is one the watcher saves.
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
        match self.tracked.entry_for(path) {
            Some(entry) => entry.policy.autosave,
            None => false,
        }
    }
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
fn install(
    debouncer: &mut Debouncer<RecommendedWatcher, NoCache>,
    installed: &[Anchor],
    wanted: &[Anchor],
    out: &Output,
) -> Result<Vec<Anchor>> {
    let mut current: Vec<Anchor> = vec![];
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
                out.emit(
                    "degraded",
                    &format!(
                        "cannot watch {}: the system's watch limit is reached; reconciliation still saves it (on Linux raise fs.inotify.max_user_watches)",
                        display_path(&anchor.path)
                    ),
                    json!({ "path": display_path(&anchor.path) }),
                );
            }
            Err(err) => out.emit(
                "degraded",
                &format!(
                    "cannot watch {}: {err}; reconciliation still saves it",
                    display_path(&anchor.path)
                ),
                json!({ "path": display_path(&anchor.path), "message": err.to_string() }),
            ),
        }
    }
    Ok(current)
}

/// Captures with the operation lock respected and failures backed off.
struct Capture {
    store: Store,
    out: Output,
    backoff: Duration,
    retry_at: Option<Instant>,
}

impl Capture {
    fn new(store: Store, out: Output) -> Self {
        Self {
            store,
            out,
            backoff: BACKOFF_MIN,
            retry_at: None,
        }
    }

    /// A flush deadline no earlier than the current backoff allows.
    fn not_before(&self, at: Instant) -> Instant {
        match self.retry_at {
            Some(retry) if retry > at => retry,
            _ => at,
        }
    }

    /// Saves a checkpoint of the tracked set. Returns whether the attempt
    /// ran (a deferred or failed attempt leaves its paths pending).
    fn attempt(&mut self, tracked: &TrackedSet, reason: &str) -> bool {
        if let Some(retry) = self.retry_at
            && Instant::now() < retry
        {
            return false;
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
                    return false;
                }
                Err(err) => {
                    self.fail(reason, &format!("{err:#}"));
                    return false;
                }
            };
        let result = self.store.attempt(tracked, Draft::new(Trigger::Edit));
        drop(operation);
        match result {
            Ok(Outcome::Created(entry)) => {
                self.recovered();
                self.out.emit(
                    "captured",
                    &format!(
                        "saved checkpoint {} ({reason}): {}",
                        entry.id, entry.checkpoint.description
                    ),
                    json!({ "id": entry.id, "uuid": entry.checkpoint.uuid, "description": entry.checkpoint.description, "reason": reason }),
                );
                true
            }
            Ok(Outcome::Unchanged) => {
                self.recovered();
                self.out.emit(
                    "unchanged",
                    &format!("nothing to save ({reason})"),
                    json!({ "reason": reason }),
                );
                true
            }
            Ok(Outcome::Unavailable(message)) => {
                self.fail(reason, &message);
                false
            }
            Err(err) => {
                self.fail(reason, &format!("{err:#}"));
                false
            }
        }
    }

    fn fail(&mut self, reason: &str, message: &str) {
        self.out.emit(
            "error",
            &format!("could not save ({reason}): {message}; retrying in {:?}", self.backoff),
            json!({ "reason": reason, "message": message, "retry_in_secs": self.backoff.as_secs() }),
        );
        self.retry_at = Some(Instant::now() + self.backoff);
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
    }

    fn recovered(&mut self) {
        self.backoff = BACKOFF_MIN;
        self.retry_at = None;
    }

    fn remember_noisy(&self, monitor: &NoiseMonitor, now: Instant) {
        let path = noisy_path_in(self.store.state_dir());
        let mut record = NoisyRecord::default();
        for (noisy, count) in monitor.noisy(now) {
            record.paths.insert(
                display_path(&noisy),
                NoisyPath {
                    changes_per_10m: count,
                    last_seen: store::now_rfc3339(),
                },
            );
        }
        if let Err(err) = noise::write(&path, &record) {
            debug!("history watch: could not write {}: {err}", path.display());
        }
    }
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
                "error" | "degraded" | "noise" => warn!("history watch: {message}"),
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
