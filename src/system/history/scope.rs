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

use std::collections::BTreeSet;
use std::future::Future;
use std::path::{Path, PathBuf};
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
    /// Branch observed under the operation lock, before our own boundary.
    starting_head: Option<String>,
    pending: Pending,
    /// Paths the outcome capture reads live and promotes (the affected
    /// paths of a rollback, undo, or apply).
    promote: Vec<PathBuf>,
}

type Shared = Arc<Mutex<Writer>>;

static CURRENT: Mutex<Option<Shared>> = Mutex::new(None);
static INITIALIZING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

/// Whether an operation is open in this process.
pub(crate) fn is_active() -> bool {
    lock_unpoisoned(&CURRENT).is_some()
}

/// History-driven writes require recoverable preimages. Ordinary bootstrap
/// retains its existing ability to deploy large files and special targets.
pub(crate) fn requires_recovery_preimage() -> bool {
    lock_unpoisoned(&CURRENT)
        .as_ref()
        .is_some_and(|writer| lock_unpoisoned(writer).kind() != OperationKind::Bootstrap)
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
        Self::begin_with_wait(kind, command, dry_run, OPERATION_LOCK_WAIT).await
    }

    /// The watcher retries contention later rather than delaying its event loop.
    pub(crate) async fn begin_automatic_apply() -> Result<Self> {
        Self::begin_with_wait(
            OperationKind::Apply,
            "dotfiles pull",
            false,
            std::time::Duration::ZERO,
        )
        .await
    }

    async fn begin_with_wait(
        kind: OperationKind,
        command: &str,
        dry_run: bool,
        wait: std::time::Duration,
    ) -> Result<Self> {
        if dry_run {
            return Ok(Self(None));
        }
        if !Settings::get().history.enabled
            && matches!(kind, OperationKind::Capture | OperationKind::Bootstrap)
        {
            debug!("history: disabled by settings");
            return Ok(Self(None));
        }
        let _initializing = INITIALIZING.lock().await;
        if std::env::var_os(ENV_VAR).is_some() {
            debug!("history: attached to the parent mise operation");
            return Ok(Self(None));
        }
        if lock_unpoisoned(&CURRENT).is_some() {
            debug!("history: an operation is already open");
            return Ok(Self(None));
        }
        let tracked = TrackedSet::effective().await?;
        let command = command.to_owned();
        let writer = tokio::task::spawn_blocking(move || {
            Writer::begin(&dirs::STATE, kind, &command, tracked, wait)
        })
        .await??;
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
                    let mut writer = lock_unpoisoned(shared);
                    writer.tracked = tracked;
                }
            }
            Err(err) => warn!("history: keeping the tracked set from before the run: {err:#}"),
        }
    }

    /// Runs `f` inside an operation for `command`, a single-part command,
    /// and finishes it with the outcome.
    pub(crate) async fn wrap<T, F>(command: &str, dry_run: bool, f: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let scope = Self::begin(command, dry_run).await?;
        let result = f.await;
        scope.refresh_tracked().await;
        let summary = Summary { message: None };
        scope.finish(
            result.as_ref().err().map(|err| format!("{err:#}")),
            Some(summary),
        );
        result
    }

    /// Changes the pending outcome record (its `to`, `undoes`, `affected`,
    /// message) before it is written.
    pub(crate) fn with_operation(&self, f: impl FnOnce(&mut Operation)) {
        if let Some(shared) = &self.0 {
            let mut writer = lock_unpoisoned(shared);
            f(writer.operation_mut());
            if let Err(err) = writer.write_pending() {
                warn!("history: could not persist the operation record: {err:#}");
            }
        }
    }

    /// External commands cannot journal individual writes. Capture all tracked
    /// entries live (including manual-save files) and keep the label on the
    /// pending record so crash recovery can identify the operation too.
    pub(crate) fn prepare_capture(&self, label: Option<&str>) {
        if let Some(shared) = &self.0 {
            let mut writer = lock_unpoisoned(shared);
            writer.promote = writer
                .tracked
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
            writer.pending.checkpoint.labels = label.into_iter().map(str::to_owned).collect();
            writer.operation_mut().message = label.map(str::to_owned);
            if let Err(err) = writer.write_pending() {
                warn!("history: could not persist the capture label: {err:#}");
            }
        }
    }

    /// The protective checkpoint this operation took, if any.
    pub(crate) fn before(&self) -> Option<(u64, String)> {
        self.0
            .as_ref()
            .and_then(|shared| lock_unpoisoned(shared).before.clone())
    }

    pub(crate) fn validate_starting_head(&self, expected: Option<&str>) -> Result<()> {
        let Some(shared) = &self.0 else {
            eyre::bail!("incoming application requires an active recovery operation");
        };
        let writer = lock_unpoisoned(shared);
        if writer.starting_head.as_deref() != expected {
            eyre::bail!(
                "local saved history changed since planning; nothing was applied; run pull again"
            );
        }
        Ok(())
    }

    /// Retakes the protective checkpoint, replacing the earlier one, when
    /// files changed between it and the verified plan.
    pub(crate) fn recapture_before(&self, paths: &[PathBuf]) -> Result<()> {
        let Some(shared) = &self.0 else {
            return Ok(());
        };
        let mut writer = lock_unpoisoned(shared);
        // held across the reservation, the capture, and the removal: the
        // capture writes the index and a checkpoint ref, which a concurrent
        // `mise bootstrap dotfiles save` must not interleave with
        let _store_lock = writer.store.lock()?;
        let previous = writer.before.take();
        let id = writer.store.reserve_id()?;
        writer.capture_before(id, paths);
        match (writer.before.is_some(), previous) {
            // Keep the earlier boundary as ordinary history.
            (true, Some(_)) => {}
            // it did not: keep the earlier protective checkpoint, which is
            // still what the pending record points at
            (false, Some(previous)) => {
                writer.operation_mut().before = Some(previous.1.clone());
                writer.before = Some(previous);
            }
            (_, None) => {}
        }
        Ok(())
    }

    /// Marks paths the outcome capture reads live and promotes.
    pub(crate) fn promote(&self, paths: &[PathBuf]) {
        if let Some(shared) = &self.0 {
            lock_unpoisoned(shared)
                .promote
                .extend(paths.iter().cloned());
        }
    }

    /// Captures the outcome and marks the operation completed, or failed
    /// when `error` is set.
    pub(crate) fn finish(self, error: Option<String>, summary: Option<Summary>) {
        self.finish_with_writes(error, summary, true);
    }

    /// The failed batch could not be safely recovered. Keep its journal until
    /// recovery succeeds; an ordinary failed command does not imply this.
    pub(crate) fn finish_incomplete(self, error: Option<String>, summary: Option<Summary>) {
        self.finish_with_writes(error, summary, false);
    }

    fn finish_with_writes(
        mut self,
        error: Option<String>,
        summary: Option<Summary>,
        writes_finished: bool,
    ) {
        let Some(shared) = self.0.take() else {
            return;
        };
        Self::clear_current();
        if let Err(err) = lock_unpoisoned(&shared).finish(error, summary, writes_finished) {
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
        wait: std::time::Duration,
    ) -> Result<Self> {
        let store = Store::open_in(state_dir)?;
        let lock = take_operation_lock_with_wait(&store, &tracked, wait)?;
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
        let starting_head = store
            .repo()
            .map(|repo| repo.ref_oid(super::shadow::HistoryRepo::HISTORY_REF))
            .transpose()?
            .flatten();
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
            id: uuid.clone(),
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
            sources: vec![],
            directories: vec![],
            directory_modes: Default::default(),
            message: None,
            journal: vec![],
        };
        let pending = Pending {
            id: outcome_id,
            recovery: store::RecoveryState::Pending,
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
        };
        // the pending record exists before anything is mutated
        store::write_pending_in(state_dir, &pending)?;
        let mut writer = Self {
            store,
            tracked,
            _lock: lock,
            before: None,
            starting_head,
            pending,
            promote: vec![],
        };
        // A fresh pull adopts origin's existing ancestry after its journaled
        // writes. An empty before commit here would create an unrelated root.
        if (kind != OperationKind::Apply || writer.starting_head.is_some())
            && records_file_history(&writer.store, &writer.tracked, Some(kind))?
        {
            writer.capture_before(before_id, &[]);
        }
        Ok(writer)
    }

    /// The protective checkpoint. A failure is recorded rather than
    /// propagated: the run must go on, and the outcome says what happened.
    fn capture_before(&mut self, id: u64, paths: &[PathBuf]) {
        let mut draft = Draft::new(before_trigger(self.kind()));
        draft.protective = true;
        // Restoration must protect the actual selected live files, including
        // unsaved manual entries, without saving unrelated manual edits.
        draft.explicit_paths = paths.to_vec();
        // `capture -- command` explicitly saves the tracked interval, including
        // manual-save entries. Ordinary bootstrap boundaries must not save
        // unrelated manual edits merely because an operation ran.
        if self.kind() == OperationKind::Capture {
            draft.explicit_paths = self
                .tracked
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
        }
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
        if self.kind() == OperationKind::Bootstrap
            && let Some((_, before)) = &self.before
            && let JournalEntry::PathChanged { path, prior, .. } =
                &self.operation().journal[seq as usize]
        {
            match self.store.protect_manual_preimage(
                before,
                &self.tracked,
                path,
                prior,
                &self.operation().journal[..seq as usize],
            ) {
                Ok(Some(entry)) => {
                    self.operation_mut().before = Some(entry.checkpoint.uuid.clone());
                    self.before = Some((entry.id, entry.checkpoint.uuid));
                }
                Ok(None) => {}
                Err(err) => {
                    warn!(
                        "history: cannot preserve the operation's manual-file preimage: {err:#}; historical undo will be unavailable"
                    );
                    self.before = None;
                    self.operation_mut().before = None;
                }
            }
            self.write_pending()?;
        }
        Ok(seq)
    }

    fn write_pending(&self) -> Result<()> {
        store::write_pending_in(self.store.state_dir(), &self.pending)
    }

    fn finish(
        &mut self,
        error: Option<String>,
        summary: Option<Summary>,
        writes_finished: bool,
    ) -> Result<()> {
        // Completed writes remain in place on an ordinary command failure;
        // only entries without a matching completion need recovery. When the
        // caller reports incomplete application, retry the whole journal.
        self.pending.recovery = if writes_finished {
            store::RecoveryState::UnfinishedWrites
        } else {
            store::RecoveryState::Pending
        };
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
        // Persist the outcome before recovery. A failure or concurrent edit
        // must leave the only recovery record intact for a later retry.
        self.write_pending()?;
        recover_pending(&state_dir, &mut self.pending)?;
        let checkpoint = &self.pending.checkpoint;
        let operation = self.operation().clone();
        let mut draft = Draft::new(checkpoint.trigger);
        draft.uuid = Some(checkpoint.uuid.clone());
        draft.labels = checkpoint.labels.clone();
        draft.operation = Some(operation.clone());
        draft.explicit_paths = self.promote.clone();
        // Bootstrap writes are explicit saves of the paths it actually
        // changed, including manual-save destinations. Do not promote
        // unrelated manual edits merely because an operation ran.
        draft
            .explicit_paths
            .extend(operation.journal.iter().filter_map(|entry| {
                let JournalEntry::Committed { seq, .. } = entry else {
                    return None;
                };
                match operation.journal.get(*seq as usize) {
                    Some(JournalEntry::PathChanged { path, .. }) => Some(path.clone()),
                    _ => None,
                }
            }));
        if records_file_history(&self.store, &self.tracked, Some(self.kind()))?
            && let Outcome::Unavailable(reason) =
                self.store
                    .attempt_locked(&self.tracked, draft, Some(self.pending.id))?
        {
            warn!("history: operation completed without a Git history commit: {reason}");
        }
        let uuid = self.pending.checkpoint.uuid.clone();
        store::remove_pending_in(&state_dir, &uuid);
        super::recovery::discard(&state_dir, &operation.journal)?;
        store::remove_marker_in(&state_dir);

        Ok(())
    }
}

/// Bootstrap without enrollment still needs private write recovery, but must
/// not create an unrelated Git root before a later `--from-git` setup.
/// Explicit command captures can request labeled empty boundaries; existing
/// Git history also keeps its boundaries after all paths are untracked.
fn records_file_history(
    store: &Store,
    tracked: &TrackedSet,
    kind: Option<OperationKind>,
) -> Result<bool> {
    if !Settings::get().history.enabled {
        return Ok(false);
    }
    Ok(kind == Some(OperationKind::Capture)
        || !tracked.entries.is_empty()
        || !tracked.manifest.enrollment.is_empty()
        || store
            .repo()
            .map(|repo| repo.ref_oid(super::shadow::HistoryRepo::HISTORY_REF))
            .transpose()?
            .flatten()
            .is_some())
}

/// Takes the operation lock, or fails naming the operation that holds it.
/// A marker whose lock is free is stale: its pending record is closed as
/// failed first. Every write to the store that is not an operation of its
/// own (an explicit save) holds this for its duration too, so it never
/// interleaves with a bootstrap, rollback, or undo.
pub(crate) fn take_operation_lock(store: &Store, tracked: &TrackedSet) -> Result<fslock::LockFile> {
    take_operation_lock_with_wait(store, tracked, OPERATION_LOCK_WAIT)
}

/// Network synchronization may skip a capture while a write operation owns
/// the files, but must never save an intermediate application state.
pub(crate) fn try_operation_lock(
    store: &Store,
    tracked: &TrackedSet,
) -> Result<Option<fslock::LockFile>> {
    let Some(lock) = LockFile::new(&store::operation_lock_in(store.state_dir())).try_lock()? else {
        return Ok(None);
    };
    recover_stale(store, tracked)?;
    Ok(Some(lock))
}

fn take_operation_lock_with_wait(
    store: &Store,
    tracked: &TrackedSet,
    wait: std::time::Duration,
) -> Result<fslock::LockFile> {
    let lock = acquire_operation_lock(store, wait)?;
    recover_stale(store, tracked)?;
    Ok(lock)
}

/// Recovery must be able to acquire the operation lock without first retrying
/// the very transaction whose concurrent edits require an explicit decision.
pub(crate) fn recovery_lock(store: &Store) -> Result<fslock::LockFile> {
    acquire_operation_lock(store, std::time::Duration::ZERO)
}

fn acquire_operation_lock(store: &Store, wait: std::time::Duration) -> Result<fslock::LockFile> {
    let state_dir = store.state_dir();
    let path = store::operation_lock_in(state_dir);
    // a running operation (the watcher applying incoming changes, say) is
    // usually over in moments: wait for it a bounded while before failing
    let deadline = std::time::Instant::now() + wait;
    let mut announced = false;
    let lock = loop {
        if let Some(lock) = LockFile::new(&path).try_lock()? {
            break lock;
        }
        let marker = store::read_marker_in(state_dir)?;
        if std::time::Instant::now() >= deadline {
            match marker {
                Some(marker) => bail!(
                    "another history operation is running: {} since {} ({})",
                    marker.kind.as_str(),
                    marker.started_at,
                    marker.command
                ),
                None => bail!("another history operation is running"),
            }
        }
        if !announced {
            announced = true;
            info!(
                "history: waiting for another history operation to finish{}",
                marker
                    .map(|marker| format!(": {} ({})", marker.kind.as_str(), marker.command))
                    .unwrap_or_default()
            );
        }
        std::thread::sleep(OPERATION_LOCK_POLL);
    };
    Ok(lock)
}

/// How long an operation waits for a running one, and how often it looks.
const OPERATION_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
const OPERATION_LOCK_POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// Closes the pending records of operations that died. Only the holder of
/// the operation lock may call this: a pending record whose operation is
/// still running is not stale, and closing it would leave two checkpoints
/// with one reserved id.
pub(crate) fn recover_stale(store: &Store, tracked: &TrackedSet) -> Result<()> {
    recover_records(store, tracked, store::list_pending_in(store.state_dir())?)
}

/// Retry a single operation under the caller's recovery lock. Accepting live
/// files is deliberately separate from ordinary automatic recovery.
pub(crate) fn recover_operation(
    store: &Store,
    tracked: &TrackedSet,
    uuid: &str,
    keep_current: bool,
) -> Result<()> {
    let mut pending = store::list_pending_in(store.state_dir())?
        .into_iter()
        .filter(|(_, record)| record.checkpoint.uuid == uuid)
        .collect::<Vec<_>>();
    if pending.is_empty() {
        bail!("no pending operation {uuid}");
    }
    if keep_current {
        for (_, record) in &mut pending {
            record.recovery = store::RecoveryState::Finished;
            if let Some(operation) = &mut record.checkpoint.operation {
                operation.status = OperationStatus::Failed;
                operation.finished_at = Some(store::now_rfc3339());
                operation.error =
                    Some("current live files explicitly accepted during recovery".into());
            }
            store::write_pending_in(store.state_dir(), record)?;
        }
    }
    recover_records(store, tracked, pending)
}

fn recover_records(
    store: &Store,
    tracked: &TrackedSet,
    pending: Vec<(PathBuf, Pending)>,
) -> Result<()> {
    let state_dir = store.state_dir();
    if pending.is_empty() {
        store::remove_marker_in(state_dir);
        return Ok(());
    }
    // An outcome commit may have reached Git before its derived index was
    // written. Rebuild before deciding whether recovery must record it.
    let index = store.rebuild_index()?;
    let _store_lock = store.lock()?;
    let mut recorded_operations = BTreeSet::new();
    for entry in &index.entries {
        if let Some(operation) = store::read_meta_cache_in(state_dir, &entry.uuid)?
            .and_then(|checkpoint| checkpoint.operation)
        {
            recorded_operations.insert(operation.id);
        }
    }
    for (path, mut record) in pending {
        let interrupted = record
            .checkpoint
            .operation
            .as_ref()
            .is_some_and(|operation| operation.status == OperationStatus::Pending);
        if record
            .checkpoint
            .operation
            .as_ref()
            .is_some_and(|op| recorded_operations.contains(&op.id))
        {
            warn!(
                "history: dropping a stale pending record for checkpoint {}, which was recorded",
                record.checkpoint.uuid
            );
            if let Some(operation) = &record.checkpoint.operation {
                super::recovery::discard_pending(
                    state_dir,
                    &operation.journal,
                    &record.checkpoint.uuid,
                )?;
            }
            std::fs::remove_file(&path)?;
            continue;
        }
        recover_pending(state_dir, &mut record)?;
        warn!(
            "history: recovering interrupted operation {}",
            record.checkpoint.uuid
        );
        if let Some(operation) = record.checkpoint.operation.as_mut()
            && interrupted
        {
            operation.status = OperationStatus::Failed;
            operation.finished_at = Some(store::now_rfc3339());
            operation.error = Some("the command exited before finishing; files observed during recovery may include later edits".into());
        }
        let mut draft = Draft::new(record.checkpoint.trigger);
        draft.uuid = Some(record.checkpoint.uuid.clone());
        draft.labels = record.checkpoint.labels.clone();
        if record
            .checkpoint
            .operation
            .as_ref()
            .is_some_and(|op| op.kind == OperationKind::Capture)
        {
            draft.explicit_paths = tracked
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
        }
        draft.operation = record.checkpoint.operation.clone();
        // the record is the only trace of what the crashed run changed:
        // keep it until the failed operation is recorded
        let captured = if records_file_history(
            store,
            tracked,
            record.checkpoint.operation.as_ref().map(|op| op.kind),
        )? {
            store.attempt_locked(tracked, draft, Some(record.id))
        } else {
            Ok(Outcome::Unchanged)
        };
        match captured {
            Ok(_) => {
                if let Some(operation) = &record.checkpoint.operation {
                    super::recovery::discard_pending(
                        state_dir,
                        &operation.journal,
                        &record.checkpoint.uuid,
                    )?;
                }
                std::fs::remove_file(&path)?;
            }
            Err(err) => warn!(
                "history: could not close operation {}; keeping {} for the next run: {err:#}",
                record.checkpoint.uuid,
                crate::file::display_path(&path)
            ),
        }
    }
    if store::list_pending_in(state_dir)?.is_empty() {
        store::remove_marker_in(state_dir);
    }
    Ok(())
}

fn recover_pending(state_dir: &Path, pending: &mut Pending) -> Result<()> {
    if let Some(operation) = &pending.checkpoint.operation {
        match pending.recovery {
            store::RecoveryState::Pending => {
                super::recovery::recover(state_dir, &operation.journal)?
            }
            store::RecoveryState::UnfinishedWrites => {
                super::recovery::recover_unfinished(state_dir, &operation.journal)?
            }
            store::RecoveryState::Finished => return Ok(()),
        }
    }
    pending.recovery = store::RecoveryState::Finished;
    store::write_pending_in(state_dir, pending)
}

fn before_trigger(kind: OperationKind) -> Trigger {
    match kind {
        OperationKind::Capture => Trigger::CaptureBefore,
        OperationKind::Bootstrap => Trigger::BootstrapBefore,
        OperationKind::Rollback => Trigger::RollbackBefore,
        OperationKind::Undo => Trigger::UndoBefore,
        OperationKind::Apply => Trigger::ApplyBefore,
    }
}

fn outcome_trigger(kind: OperationKind) -> Trigger {
    match kind {
        OperationKind::Capture => Trigger::Capture,
        OperationKind::Bootstrap => Trigger::Bootstrap,
        OperationKind::Rollback => Trigger::Rollback,
        OperationKind::Undo => Trigger::Undo,
        OperationKind::Apply => Trigger::Apply,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::history::shadow::HistoryRepo;

    #[test]
    fn accepting_current_files_preserves_edits_and_other_pending_operations() -> Result<()> {
        use super::super::journal::{Capture, PathSnapshot, PathState};

        let temp = tempfile::tempdir()?;
        let live = temp.path().join("untracked");
        std::fs::write(&live, vec![b'x'; 70_000])?;
        let writer = Writer::begin(
            temp.path(),
            OperationKind::Bootstrap,
            "bootstrap",
            TrackedSet::default(),
            std::time::Duration::ZERO,
        )?;
        let mut pending = writer.pending.clone();
        let prior = PathSnapshot::capture_with(temp.path(), &live, Capture::Full);
        std::fs::write(&live, "operation contents")?;
        pending.checkpoint.operation.as_mut().unwrap().journal = vec![
            JournalEntry::PathChanged {
                part: "files".into(),
                item: "untracked".into(),
                path: live.clone(),
                prior,
            },
            JournalEntry::Committed {
                seq: 0,
                after: PathState::observe(&live),
            },
        ];
        store::write_pending_in(temp.path(), &pending)?;
        drop(writer);
        std::fs::write(&live, "later user edit")?;
        let store = Store::open_in(temp.path())?;
        assert!(recover_stale(&store, &TrackedSet::default()).is_err());
        assert_eq!(std::fs::read_to_string(&live)?, "later user edit");
        assert_eq!(store::list_pending_in(temp.path())?.len(), 1);

        let mut other = pending.clone();
        other.checkpoint.uuid = uuid::Uuid::new_v4().to_string();
        other.checkpoint.operation.as_mut().unwrap().id = other.checkpoint.uuid.clone();
        other.checkpoint.operation.as_mut().unwrap().journal.clear();
        store::write_pending_in(temp.path(), &other)?;
        recover_operation(
            &store,
            &TrackedSet::default(),
            &pending.checkpoint.uuid,
            true,
        )?;
        assert_eq!(std::fs::read_to_string(&live)?, "later user edit");
        let remaining = store::list_pending_in(temp.path())?;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].1.checkpoint.uuid, other.checkpoint.uuid);
        assert_eq!(
            std::fs::read_dir(super::super::journal::blobs_dir_in(temp.path()))?.count(),
            0
        );
        assert!(
            store
                .repo()
                .unwrap()
                .ref_oid(HistoryRepo::HISTORY_REF)?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn committed_outcome_is_not_repeated_after_pending_cleanup_crashes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut writer = Writer::begin(
            temp.path(),
            OperationKind::Capture,
            "capture",
            TrackedSet::default(),
            std::time::Duration::ZERO,
        )?;
        let pending = writer.pending.clone();
        writer.finish(None, None, true)?;
        let head = writer
            .store
            .repo()
            .unwrap()
            .ref_oid(HistoryRepo::HISTORY_REF)?;
        let before = writer.store.list()?;
        // Simulate a crash after committing the outcome, before removing its
        // pending record. Lose the derived index too: only Git is authoritative.
        store::write_pending_in(temp.path(), &pending)?;
        std::fs::remove_file(store::index_dir_in(temp.path()).join("checkpoints.json"))?;
        drop(writer);
        let store = Store::open_in(temp.path())?;
        recover_stale(&store, &TrackedSet::default())?;
        assert!(store::list_pending_in(temp.path())?.is_empty());
        assert_eq!(
            store.repo().unwrap().ref_oid(HistoryRepo::HISTORY_REF)?,
            head
        );
        assert_eq!(store.list()?.len(), before.len());
        Ok(())
    }

    #[test]
    fn unenrolled_bootstrap_keeps_recovery_private_without_creating_history() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut writer = Writer::begin(
            temp.path(),
            OperationKind::Bootstrap,
            "bootstrap",
            TrackedSet::default(),
            std::time::Duration::ZERO,
        )?;
        assert!(
            writer
                .store
                .repo()
                .unwrap()
                .ref_oid(super::super::shadow::HistoryRepo::HISTORY_REF)?
                .is_none()
        );
        assert_eq!(store::list_pending_in(temp.path())?.len(), 1);
        writer.finish(None, None, true)?;
        assert!(
            writer
                .store
                .repo()
                .unwrap()
                .ref_oid(super::super::shadow::HistoryRepo::HISTORY_REF)?
                .is_none()
        );
        assert!(store::list_pending_in(temp.path())?.is_empty());
        Ok(())
    }

    #[test]
    fn application_checks_head_from_before_its_own_protective_commit() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open_in(temp.path())?;
        let repo = store.repo().unwrap();
        let tree =
            super::super::manifest::Manifest::default().write(repo, &repo.empty_object("tree")?)?;
        let planned = repo.commit_tree(&tree, vec![], "planned")?;
        repo.update_history_head(&planned, None)?;
        let saved = repo.commit_tree(&tree, vec![&planned], "concurrent save")?;
        repo.update_history_head(&saved, Some(&planned))?;
        let writer = Writer::begin(
            temp.path(),
            OperationKind::Apply,
            "pull",
            TrackedSet::default(),
            std::time::Duration::ZERO,
        )?;
        let scope = OperationScope(Some(Arc::new(Mutex::new(writer))));
        assert!(scope.validate_starting_head(Some(&planned)).is_err());
        assert!(scope.validate_starting_head(Some(&saved)).is_ok());
        assert_ne!(
            repo.ref_oid(super::super::shadow::HistoryRepo::HISTORY_REF)?
                .as_deref(),
            Some(saved.as_str())
        );
        Ok(())
    }

    #[test]
    fn automatic_operation_lock_does_not_wait() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_in(temp.path()).unwrap();
        let held = LockFile::new(&store::operation_lock_in(temp.path()))
            .try_lock()
            .unwrap()
            .unwrap();
        let started = std::time::Instant::now();
        let result = take_operation_lock_with_wait(
            &store,
            &TrackedSet::default(),
            std::time::Duration::ZERO,
        );
        assert!(result.is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        drop(held);
        assert!(
            take_operation_lock_with_wait(
                &store,
                &TrackedSet::default(),
                std::time::Duration::ZERO,
            )
            .is_ok()
        );
    }
}
