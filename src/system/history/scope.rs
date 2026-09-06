//! The operation a mutating command records into.
//!
//! An [`OperationScope`] is opened at the start of a mutating command and
//! finished at its end. It owns the operation lock for its whole lifetime,
//! writes the recovery marker and the pending outcome record before any
//! mutation, takes the protective `*-before` checkpoint, and captures the
//! outcome at the end. It is process-global so the apply code deep inside
//! `system::*` can append journal entries through [`record`] without every
//! signature threading a writer, and it exports [`ENV_VAR`] so child `mise`
//! processes spawned by hooks attach to the parent's operation instead of
//! opening their own.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use eyre::{Result, bail};

use super::checkpoint::{Draft, Outcome, Store};
use super::journal::JournalEntry;
use super::store::{
    self, Changes, Checkpoint, DescriptionSource, Operation, OperationKind, OperationMarker,
    OperationStatus, Pending, Summary, TreeInfo, Trigger,
};
use super::tracked::TrackedSet;
use crate::config::Settings;
use crate::dirs;
use crate::env;
use crate::lock_file::LockFile;

/// Set in the environment while an operation is open; a child mise that
/// sees it records nothing of its own.
pub(crate) const ENV_VAR: &str = "__MISE_HISTORY_OPERATION";

struct Writer {
    store: Store,
    tracked: TrackedSet,
    /// Held for the operation's lifetime.
    _lock: fslock::LockFile,
    before: Option<(u64, String)>,
    pending: Pending,
}

type Shared = Arc<Mutex<Writer>>;

static CURRENT: Mutex<Option<Shared>> = Mutex::new(None);

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Appends an entry to the open operation's journal, if any, and persists
/// it. Returns the entry's index, or an error when it could not be written
/// to disk (the caller decides whether to proceed).
pub(crate) fn record(entry: JournalEntry) -> Result<Option<u32>> {
    let shared = lock_unpoisoned(&CURRENT).clone();
    match shared {
        Some(shared) => lock_unpoisoned(&shared).record(entry).map(Some),
        None => Ok(None),
    }
}

/// Stores journal content in the repository for the open operation and
/// returns its blob oid, or `None` when no operation (or no git) is open.
pub(crate) fn store_blob(sha256: &str, bytes: &[u8]) -> Result<Option<String>> {
    let shared = lock_unpoisoned(&CURRENT).clone();
    match shared {
        Some(shared) => lock_unpoisoned(&shared).store_blob(sha256, bytes),
        None => Ok(None),
    }
}

/// Whether an operation is open in this process.
pub(crate) fn is_active() -> bool {
    lock_unpoisoned(&CURRENT).is_some()
}

/// RAII handle for the operation a command records into. Inactive scopes
/// (dry runs, recording disabled, nested commands) are no-ops so callers
/// never branch on them.
#[must_use = "finish the scope so the operation is recorded"]
pub(crate) struct OperationScope(Option<Shared>);

impl OperationScope {
    /// Opens an operation for `command` unless nothing should be recorded.
    /// Fails when another history operation holds the operation lock.
    pub(crate) async fn begin(command: &str, dry_run: bool) -> Result<Self> {
        Self::begin_kind(OperationKind::Bootstrap, command, dry_run).await
    }

    pub(crate) async fn begin_kind(
        kind: OperationKind,
        command: &str,
        dry_run: bool,
    ) -> Result<Self> {
        if dry_run {
            return Ok(Self(None));
        }
        if !Settings::get().history.enabled {
            debug!("history: disabled by settings");
            return Ok(Self(None));
        }
        if std::env::var_os(ENV_VAR).is_some() {
            debug!("history: attached to the parent mise operation");
            return Ok(Self(None));
        }
        if lock_unpoisoned(&CURRENT).is_some() {
            debug!("history: an operation is already open");
            return Ok(Self(None));
        }
        let tracked = TrackedSet::effective().await?;
        let writer = Writer::begin(&dirs::STATE, kind, command, tracked)?;
        let uuid = writer.pending.checkpoint.uuid.clone();
        debug!("history: recording operation {uuid}");
        let shared = Arc::new(Mutex::new(writer));
        *lock_unpoisoned(&CURRENT) = Some(shared.clone());
        env::set_var(ENV_VAR, uuid);
        Ok(Self(Some(shared)))
    }

    /// Reloads the tracked set so the outcome capture covers what the
    /// operation declared or removed (a new track entry, a new destination).
    pub(crate) async fn refresh_tracked(&self) {
        if self.0.is_none() {
            return;
        }
        let tracked = match crate::config::Config::reset().await {
            Ok(config) => TrackedSet::from_config(&config),
            Err(err) => Err(err),
        };
        match tracked {
            Ok(tracked) => {
                if let Some(shared) = &self.0 {
                    lock_unpoisoned(shared).tracked = tracked;
                }
            }
            Err(err) => warn!("history: keeping the tracked set from before the run: {err:#}"),
        }
    }

    /// Runs `f` inside an operation for `command`, a single-part command,
    /// and finishes it with the outcome.
    pub(crate) async fn wrap<T, F>(command: &str, part: &str, dry_run: bool, f: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let scope = Self::begin(command, dry_run).await?;
        let result = f.await;
        scope.refresh_tracked().await;
        let _ = part;
        let summary = Summary { message: None };
        scope.finish(
            result.as_ref().err().map(|err| format!("{err:#}")),
            Some(summary),
        );
        result
    }

    /// Captures the outcome and marks the operation completed, or failed
    /// when `error` is set.
    pub(crate) fn finish(mut self, error: Option<String>, summary: Option<Summary>) {
        let Some(shared) = self.0.take() else {
            return;
        };
        Self::clear_current();
        if let Err(err) = lock_unpoisoned(&shared).finish(error, summary) {
            warn!("history: could not finish the operation record: {err:#}");
        }
    }

    fn clear_current() {
        *lock_unpoisoned(&CURRENT) = None;
        env::remove_var(ENV_VAR);
    }
}

impl Drop for OperationScope {
    fn drop(&mut self) {
        let Some(shared) = self.0.take() else {
            return;
        };
        Self::clear_current();
        if std::thread::panicking() {
            return;
        }
        if let Err(err) = lock_unpoisoned(&shared).abandon() {
            warn!("history: could not mark the operation failed: {err:#}");
        }
    }
}

impl Writer {
    fn begin(
        state_dir: &Path,
        kind: OperationKind,
        command: &str,
        tracked: TrackedSet,
    ) -> Result<Self> {
        let store = Store::open_in(state_dir)?;
        let lock = take_operation_lock(&store, &tracked)?;
        let argv: Vec<String> = env::ARGS.read().unwrap().iter().skip(1).cloned().collect();
        // What the user typed is the better label; the caller's name is the
        // fallback for in-process callers with no argv (tests).
        let command = if argv.is_empty() {
            command.to_string()
        } else {
            shell_words::join(&argv)
        };
        let uuid = store::new_uuid();
        let _store_lock = store.lock()?;
        let before_id = store.reserve_id()?;
        let outcome_id = store.reserve_id()?;
        store::write_marker_in(
            state_dir,
            &OperationMarker {
                uuid: uuid.clone(),
                kind,
                started_at: store::now_rfc3339(),
                command: command.clone(),
            },
        )?;
        let operation = Operation {
            kind,
            status: OperationStatus::Pending,
            command,
            argv,
            cwd: dirs::CWD.clone().unwrap_or_default(),
            user: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .ok(),
            finished_at: None,
            error: None,
            before: None,
            to: None,
            undoes: None,
            applied: None,
            affected: vec![],
            message: None,
            journal: vec![],
        };
        let pending = Pending {
            id: outcome_id,
            checkpoint: Checkpoint {
                schema_version: store::SCHEMA_VERSION,
                uuid,
                machine: store.machine().clone(),
                created_at: store::now_rfc3339(),
                mise_version: crate::cli::version::VERSION_PLAIN.clone(),
                trigger: outcome_trigger(kind),
                description: String::new(),
                description_source: DescriptionSource::Computed,
                summary: String::new(),
                task: None,
                labels: vec![],
                pinned: false,
                tree: TreeInfo {
                    snapshot: None,
                    available: store.unavailable().is_none(),
                    reason: store.unavailable().map(str::to_string),
                    roots: vec![],
                    coverage: Default::default(),
                    modes: Default::default(),
                },
                changes: Changes::default(),
                operation: Some(operation),
            },
            blobs: BTreeMap::new(),
        };
        // the pending record exists before anything is mutated
        store::write_pending_in(state_dir, &pending)?;
        let mut writer = Self {
            store,
            tracked,
            _lock: lock,
            before: None,
            pending,
        };
        writer.capture_before(before_id);
        Ok(writer)
    }

    /// The protective checkpoint. A failure is recorded rather than
    /// propagated: the run must go on, and the outcome says what happened.
    fn capture_before(&mut self, id: u64) {
        let mut draft = Draft::new(before_trigger(self.kind()));
        draft.protective = true;
        match self.store.attempt_locked(&self.tracked, draft, Some(id)) {
            Ok(Outcome::Created(entry)) => {
                self.operation_mut().before = Some(entry.checkpoint.uuid.clone());
                self.before = Some((entry.id, entry.checkpoint.uuid));
            }
            Ok(Outcome::Unchanged) => {}
            Ok(Outcome::Unavailable(reason)) => {
                warn!("history: no content snapshot before this run: {reason}");
                self.pending.checkpoint.tree.available = false;
                self.pending.checkpoint.tree.reason = Some(reason);
            }
            Err(err) => {
                warn!("history: snapshot before this run failed: {err:#}");
                self.pending.checkpoint.tree.available = false;
                self.pending.checkpoint.tree.reason = Some(format!("{err:#}"));
            }
        }
        if let Err(err) = self.write_pending() {
            warn!("history: could not persist the operation record: {err:#}");
        }
    }

    fn kind(&self) -> OperationKind {
        self.operation().kind
    }

    fn operation(&self) -> &Operation {
        self.pending
            .checkpoint
            .operation
            .as_ref()
            .expect("an operation record always has an operation")
    }

    fn operation_mut(&mut self) -> &mut Operation {
        self.pending
            .checkpoint
            .operation
            .as_mut()
            .expect("an operation record always has an operation")
    }

    fn record(&mut self, entry: JournalEntry) -> Result<u32> {
        let seq = self.operation().journal.len() as u32;
        self.operation_mut().journal.push(entry);
        self.write_pending()
            .map_err(|err| eyre::eyre!("could not persist the operation journal: {err:#}"))?;
        Ok(seq)
    }

    fn store_blob(&mut self, sha256: &str, bytes: &[u8]) -> Result<Option<String>> {
        let Some(repo) = self.store.repo() else {
            return Ok(None);
        };
        if let Some(oid) = self.pending.blobs.get(sha256) {
            return Ok(Some(oid.clone()));
        }
        let oid = repo.hash_blob(bytes)?;
        self.pending.blobs.insert(sha256.to_string(), oid.clone());
        self.write_pending()?;
        Ok(Some(oid))
    }

    fn write_pending(&self) -> Result<()> {
        store::write_pending_in(self.store.state_dir(), &self.pending)
    }

    fn finish(&mut self, error: Option<String>, summary: Option<Summary>) -> Result<()> {
        let status = if error.is_some() {
            OperationStatus::Failed
        } else {
            OperationStatus::Completed
        };
        {
            let operation = self.operation_mut();
            operation.status = status;
            operation.finished_at = Some(store::now_rfc3339());
            operation.error = error;
            if let Some(summary) = summary {
                operation.message = summary.message;
            }
        }
        self.write_outcome()
    }

    fn abandon(&mut self) -> Result<()> {
        {
            let operation = self.operation_mut();
            operation.status = OperationStatus::Failed;
            operation.finished_at = Some(store::now_rfc3339());
            operation.error = Some("the command exited before finishing".into());
        }
        self.write_outcome()
    }

    /// Captures the outcome checkpoint and closes the operation.
    fn write_outcome(&mut self) -> Result<()> {
        let state_dir = self.store.state_dir().to_path_buf();
        let _store_lock = self.store.lock()?;
        let checkpoint = &self.pending.checkpoint;
        let operation = self.operation().clone();
        let mut draft = Draft::new(checkpoint.trigger);
        draft.uuid = Some(checkpoint.uuid.clone());
        draft.operation = Some(operation.clone());
        draft.blobs = self.pending.blobs.clone();
        let outcome = self
            .store
            .attempt_locked(&self.tracked, draft, Some(self.pending.id))?;
        let uuid = self.pending.checkpoint.uuid.clone();
        store::remove_pending_in(&state_dir, &uuid);
        store::remove_marker_in(&state_dir);
        let Outcome::Created(entry) = outcome else {
            return Ok(());
        };
        // A completed run that journaled no file change and whose outcome
        // equals its protective checkpoint left no trace worth keeping. The
        // outcome's changes are relative to the protective checkpoint, with
        // carried-forward manual-save entries already discounted.
        let noop = operation.status == OperationStatus::Completed
            && operation.journal.is_empty()
            && entry.checkpoint.tree.snapshot.is_some()
            && self.before.is_some()
            && entry.checkpoint.changes.is_empty();
        if noop {
            debug!("history: operation {uuid} changed nothing, not keeping it");
            self.store.remove(entry.id)?;
            if let Some((before_id, _)) = &self.before {
                self.store.remove(*before_id)?;
            }
            return Ok(());
        }
        Ok(())
    }
}

/// Takes the operation lock, or fails naming the operation that holds it.
/// A marker whose lock is free is stale: its pending record is closed as
/// failed first. Every write to the store that is not an operation of its
/// own (an explicit save) holds this for its duration too, so it never
/// interleaves with a bootstrap, rollback, or undo.
pub(crate) fn take_operation_lock(store: &Store, tracked: &TrackedSet) -> Result<fslock::LockFile> {
    let state_dir = store.state_dir();
    let lock = LockFile::new(&store::operation_lock_in(state_dir)).try_lock()?;
    let Some(lock) = lock else {
        let marker = store::read_marker_in(state_dir)?;
        match marker {
            Some(marker) => bail!(
                "another history operation is running: {} since {} ({})",
                marker.kind.as_str(),
                marker.started_at,
                marker.command
            ),
            None => bail!("another history operation is running"),
        }
    };
    recover_stale(store, tracked)?;
    Ok(lock)
}

/// Closes the pending records of operations that died. Only the holder of
/// the operation lock may call this: a pending record whose operation is
/// still running is not stale, and closing it would leave two checkpoints
/// with one reserved id.
pub(crate) fn recover_stale(store: &Store, tracked: &TrackedSet) -> Result<()> {
    let state_dir = store.state_dir();
    let pending = store::list_pending_in(state_dir)?;
    if pending.is_empty() {
        store::remove_marker_in(state_dir);
        return Ok(());
    }
    let _store_lock = store.lock()?;
    let index = store::load_index_in(state_dir)?;
    for (path, mut record) in pending {
        if index.by_uuid(&record.checkpoint.uuid).is_some() {
            warn!(
                "history: dropping a stale pending record for checkpoint {}, which was recorded",
                record.checkpoint.uuid
            );
            let _ = std::fs::remove_file(&path);
            continue;
        }
        warn!(
            "history: operation {} did not finish; recording it as failed",
            record.checkpoint.uuid
        );
        if let Some(operation) = record.checkpoint.operation.as_mut() {
            operation.status = OperationStatus::Failed;
            operation.finished_at = Some(store::now_rfc3339());
            operation.error = Some("the command exited before finishing".into());
        }
        let mut draft = Draft::new(record.checkpoint.trigger);
        draft.uuid = Some(record.checkpoint.uuid.clone());
        draft.operation = record.checkpoint.operation.clone();
        draft.blobs = record.blobs.clone();
        // the record is the only trace of what the crashed run changed:
        // keep it until the failed operation is recorded
        match store.attempt_locked(tracked, draft, Some(record.id)) {
            Ok(_) => {
                let _ = std::fs::remove_file(&path);
            }
            Err(err) => warn!(
                "history: could not close operation {}; keeping {} for the next run: {err:#}",
                record.checkpoint.uuid,
                crate::file::display_path(&path)
            ),
        }
    }
    store::remove_marker_in(state_dir);
    Ok(())
}

fn before_trigger(kind: OperationKind) -> Trigger {
    match kind {
        OperationKind::Bootstrap => Trigger::BootstrapBefore,
        OperationKind::Rollback | OperationKind::BootstrapRollback => Trigger::RollbackBefore,
        OperationKind::Undo => Trigger::UndoBefore,
        OperationKind::Apply => Trigger::ApplyBefore,
    }
}

fn outcome_trigger(kind: OperationKind) -> Trigger {
    match kind {
        OperationKind::Bootstrap => Trigger::Bootstrap,
        OperationKind::Rollback | OperationKind::BootstrapRollback => Trigger::Rollback,
        OperationKind::Undo => Trigger::Undo,
        OperationKind::Apply => Trigger::Apply,
    }
}
