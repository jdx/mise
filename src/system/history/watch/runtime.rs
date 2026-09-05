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
use globset::GlobSet;
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
use crate::system::history::tracked::{self, TrackedSet, hard_exclusions, normalize};

/// How long the debouncer coalesces raw filesystem events before they reach
/// the scheduler, which applies the configured quiet period on top.
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
    let mut capture = Capture::new(store, out, intervals.limits.clone());
    capture.health.watcher.started_at = Some(store::now_rfc3339());
    capture.attempt(&state.tracked, "startup reconcile", &[]);
    capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
    capture.write_health();
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
    let mut installed = install(&mut debouncer, &[], &state.plan.anchors, &mut capture)?;
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
    capture.write_health();

    let mut shutdown = Shutdown::new()?;
    let mut next_reconcile = intervals
        .reconcile
        .map(|every| tokio::time::Instant::now() + every);
    loop {
        let flush_at = capture.schedule.deadline().map(|at| capture.not_before(at));
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
                                capture.schedule.note(path, now);
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
                            installed = install(&mut debouncer, &installed, &state.plan.anchors, &mut capture)?;
                            capture.out.emit(
                                "replan",
                                &format!("configuration changed; watching {} anchor(s)", installed.len()),
                                json!({ "anchors": installed.len() }),
                            );
                            // the configuration that changed is what this
                            // capture is for: never held back
                            let config_dir = state.config_dir.clone();
                            let held: Vec<PathBuf> = capture
                                .schedule
                                .held_paths(now)
                                .into_iter()
                                .filter(|path| !path.starts_with(&config_dir))
                                .collect();
                            if capture.attempt(&state.tracked, "configuration changed", &held) {
                                for path in capture.schedule.due_paths(now).into_iter().chain(
                                    capture
                                        .schedule
                                        .held_paths(now)
                                        .into_iter()
                                        .filter(|path| path.starts_with(&config_dir)),
                                ) {
                                    capture.schedule.saved(&path, now);
                                }
                            }
                            capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
                            capture.write_health();
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
                    let held = capture.schedule.held_paths(now);
                    capture.attempt(&state.tracked, "rescan", &held);
                }
            }
            _ = flush => {
                let now = Instant::now();
                let due = capture.schedule.due_paths(now);
                if !due.is_empty() {
                    let held = capture.schedule.held_paths(now);
                    let done = capture.attempt(&state.tracked, &describe(&due), &held);
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
                if state.plan.pending.iter().any(|path| path.exists())
                    && let Ok(true) = state.reload().await
                {
                    installed = install(&mut debouncer, &installed, &state.plan.anchors, &mut capture)?;
                }
                let now = Instant::now();
                let held = capture.schedule.held_paths(now);
                capture.attempt(&state.tracked, "reconcile", &held);
                capture.health.watcher.last_reconcile = Some(store::now_rfc3339());
                capture.write_health();
            }
            _ = shutdown.wait() => {
                // a full capture, not only the due paths: a change still
                // inside the coalescing window has not reached the scheduler
                // yet, and a throttled file's final state is saved now
                let now = Instant::now();
                if capture.attempt(&state.tracked, "shutdown", &[]) {
                    capture.schedule.clear_pending(now);
                }
                capture.persist_schedule();
                capture.out.emit("stopped", "stopping", json!({}));
                capture.write_health();
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
}

impl Capture {
    fn new(store: Store, out: Output, limits: Limits) -> Self {
        let mut schedule = Schedule::new(limits);
        let persisted: PersistedSchedule =
            std::fs::read_to_string(schedule_path_in(store.state_dir()))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
        schedule.restore(&persisted, epoch_secs());
        let health = health::read(store.state_dir()).unwrap_or_default();
        Self {
            store,
            out,
            schedule,
            health,
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

    /// Saves a checkpoint of the tracked set, with `held` paths carried
    /// forward from the newest checkpoint instead of read live. Returns
    /// whether the attempt ran (a deferred or failed attempt leaves its
    /// paths pending).
    fn attempt(&mut self, tracked: &TrackedSet, reason: &str, held: &[PathBuf]) -> bool {
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
                true
            }
            Ok(Outcome::Unchanged) => {
                self.recovered();
                self.health.watcher.last_capture = Some(store::now_rfc3339());
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
            &format!(
                "could not save ({reason}): {message}; retrying in {:?}",
                self.backoff
            ),
            json!({ "reason": reason, "message": message, "retry_in_secs": self.backoff.as_secs() }),
        );
        self.retry_at = Some(Instant::now() + self.backoff);
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
        self.health.watcher.last_error = Some(message.to_string());
        self.health.watcher.last_error_at = Some(store::now_rfc3339());
        self.health.watcher.consecutive_failures += 1;
        self.write_health();
    }

    fn recovered(&mut self) {
        self.backoff = BACKOFF_MIN;
        self.retry_at = None;
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
                    last_seen: store::now_rfc3339(),
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
