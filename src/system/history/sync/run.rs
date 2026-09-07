//! One synchronization: fetch the setup branch and other machines'
//! recovery refs, run the transition table, publish this machine's
//! changes (leased; on a rejection fetch again and retry), upload eligible
//! checkpoints, record the durable state, and derive `sync.json` with what
//! is pending: incoming changes to apply, conflicts to decide, uploads
//! still to do. Captures never wait on the network; this never changes a
//! live file (application is its own operation).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use super::SyncMode;
use super::format::{self, RepoState};
use super::layout::{Located, Roots, is_configuration};
use super::network::{PushOutcome, Remote, UPSTREAM_REF};
use super::reconcile::{self, Conflict, Object, PathPlan};
use super::{backup, publish, share, state};
use crate::file::display_path;
use crate::system::history::checkpoint::Store;
use crate::system::history::config::OriginTomlConfig;
use crate::system::history::store::{self as hstore, Entry};
use crate::system::history::tracked::TrackedSet;

const PUSH_RETRIES: usize = 5;

/// An incoming change waiting for `mise bootstrap dotfiles pull` (or automatic
/// application in `sync` mode).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PendingApplication {
    pub branch_path: String,
    /// `None` deletes the local file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<Object>,
    /// Configuration, whose change may alter declarations.
    pub configuration: bool,
    /// The state to record once the write succeeds.
    pub next: state::SyncRecord,
    /// Saved local version used to compute this application. Absence is
    /// significant: saving a newer version never authorizes overwriting it.
    #[serde(default)]
    pub local: Option<Object>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Resolution {
    pub local: Option<Object>,
    pub remote: Option<Object>,
    pub live: Option<Object>,
    pub take_remote: bool,
}

/// Derived from the repository after every sync; rebuilt when it
/// disagrees.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct SyncStatus {
    /// Choices are bound to all three observed versions, not just a path.
    #[serde(default)]
    pub resolutions: BTreeMap<String, Resolution>,
    /// A failed application is not cleared by successful network traffic.
    /// An explicit pull retries the complete batch after inspecting it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_publish: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fetch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_apply: Option<String>,
    #[serde(default)]
    pub uploaded: BTreeSet<String>,
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
    #[serde(default)]
    pub pending_applications: Vec<PendingApplication>,
    /// Incoming configuration changed declarations: run `mise bootstrap`.
    #[serde(default)]
    pub declarations_changed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// When the current run of failed syncs began; a success clears it. An
    /// origin that has never answered has no last success to measure from,
    /// so `mise doctor` measures the failure from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failing_since: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub consecutive_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_commit: Option<String>,
    /// The user confirmed adopting an unmarked repository.
    #[serde(default)]
    pub adopted: bool,
    /// Checkpoints recorded before the origin was connected are not
    /// uploaded unless `--include-existing` was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_since: Option<String>,
    /// The repository this state belongs to; another one starts afresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_branch: Option<String>,
    /// Whether this conflict-paused episode has already been observed.
    #[serde(default)]
    pub conflict_pause_observed: bool,
    /// `origin --remove` was run: the recorded repository no longer stands
    /// in for a declaration.
    #[serde(default)]
    pub disconnected: bool,
    /// How this machine's refs on the origin were written (`backup::scheme`);
    /// absent means plaintext. A change replaces them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_scheme: Option<String>,
    #[serde(default)]
    pub backup_policy: Option<String>,
    /// Why uploads are being skipped (encryption on without usable
    /// recipients); publication and fetching continue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_error: Option<String>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

pub(crate) fn status_path(state_dir: &Path) -> PathBuf {
    hstore::store_dir_in(state_dir).join("sync.json")
}

pub(crate) fn read_status(state_dir: &Path) -> Result<SyncStatus> {
    let path = status_path(state_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(SyncStatus::default()),
        Err(err) => return Err(err).wrap_err_with(|| format!("cannot read sync state {}; repair its permissions or restore it from a backup before syncing", display_path(&path))),
    };
    serde_json::from_str(&text).wrap_err_with(|| format!("invalid sync state {}; restore a valid copy before syncing; the existing state has been preserved", display_path(&path)))
}

pub(crate) fn write_status(state_dir: &Path, status: &SyncStatus) -> Result<()> {
    hstore::write_json(&status_path(state_dir), status)
}

#[cfg(test)]
mod status_read_tests {
    use super::*;

    #[test]
    fn missing_sync_status_is_empty_but_invalid_state_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_status(dir.path()).unwrap().uploaded.is_empty());
        let path = status_path(dir.path());
        crate::file::create_dir_all(path.parent().unwrap()).unwrap();
        let invalid = b"{\"resolutions\": broken";
        std::fs::write(&path, invalid).unwrap();
        assert!(
            read_status(dir.path())
                .unwrap_err()
                .to_string()
                .contains("invalid sync state")
        );
        assert_eq!(std::fs::read(&path).unwrap(), invalid);
    }

    #[test]
    fn unreadable_sync_status_is_not_treated_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::file::create_dir_all(status_path(dir.path())).unwrap();
        assert!(
            read_status(dir.path())
                .unwrap_err()
                .to_string()
                .contains("cannot read sync state")
        );
    }
}

#[derive(Debug, Default)]
pub(crate) struct SyncOutcome {
    pub published: Option<String>,
    pub uploaded: usize,
    pub pruned_remote: usize,
    /// Refs deleted from the origin because the backup scheme changed.
    pub replaced_remote: usize,
    /// Uploads were skipped: encryption is on without usable recipients.
    pub backup_error: Option<String>,
    pub pending: usize,
    pub conflicts: usize,
    pub fetched_upstream: Option<String>,
}

pub(crate) struct SyncRequest {
    pub fetch_only: bool,
    /// Save the tracked set first, so what is published is what is on
    /// disk. The watcher passes `false`: it saves on its own schedule, and a
    /// throttled file's held version or a manual-save entry's unsaved edits
    /// must not reach the repository through a sync.
    pub capture: bool,
    /// The repository to use instead of `[history.origin]` (onboarding,
    /// before the configuration that declares it is in place).
    pub origin: Option<OriginTomlConfig>,
    /// No network: reconcile against the branch as last fetched and record
    /// what is pending (after an incoming configuration declared more).
    pub offline: bool,
    /// A preview: nothing is announced (the caller puts the recorded state
    /// back afterwards).
    pub dry_run: bool,
}

impl SyncRequest {
    pub(crate) fn new(fetch_only: bool) -> Self {
        Self {
            fetch_only,
            capture: true,
            origin: None,
            offline: false,
            dry_run: false,
        }
    }
}

/// The connected origin, or why there is none.
pub(crate) fn origin() -> Result<OriginTomlConfig> {
    if let Some((_, origin)) = crate::system::history::config::origin()? {
        return Ok(origin);
    }
    // recorded when it was connected: a fresh machine's declaration may
    // still be on its way in the configuration being pulled
    let status = read_status(&crate::dirs::STATE)?;
    if let (Some(url), Some(branch), false) =
        (status.origin_url, status.origin_branch, status.disconnected)
    {
        let mut origin = OriginTomlConfig::plain(url, branch);
        // refs written encrypted stay that way: with the declaration (and
        // its recipients) gone, uploads are skipped, never made plaintext
        origin.encrypt_backups = status
            .backup_scheme
            .as_deref()
            .is_some_and(|scheme| scheme != backup::PLAIN_SCHEME);
        return Ok(origin);
    }
    bail!(
        "no setup repository is connected; `mise bootstrap dotfiles origin set <url>` connects one"
    )
}

/// Runs one synchronization.
pub(crate) fn sync(
    store: &Store,
    tracked: &TrackedSet,
    request: &SyncRequest,
) -> Result<SyncOutcome> {
    let _sync_lock = lock(store)?;
    let origin = match &request.origin {
        Some(origin) => origin.clone(),
        None => origin()?,
    };
    // resolved before anything is fetched; an unusable declaration skips
    // uploads below rather than failing the sync (publication and fetching
    // do not depend on it)
    let encryption = backup::BackupEncryption::resolve(
        &origin,
        request.capture && console::user_attended_stderr(),
    );
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("synchronizing requires git"))?;
    let mode = SyncMode::current()?;
    let state_dir = store.state_dir();
    let mut status = read_status(state_dir)?;
    let machine = store.machine().clone();
    let remote = Remote::new(repo, &origin.url);
    let mut outcome = SyncOutcome::default();

    let result = (|| -> Result<()> {
        if !request.offline {
            // a branch that vanished from a repository this machine had
            // synced with is not an empty upstream: reading it as one would
            // queue the deletion of every file it held
            let found = remote.fetch_pruning(&origin.branch)?;
            if !found && status.upstream_commit.is_some() {
                bail!(
                    "the setup branch `{}` is not at {} any more (renamed, or deleted?); nothing was changed. `mise bootstrap dotfiles origin set {} --branch <name>` follows a renamed branch",
                    origin.branch,
                    origin.url,
                    origin.url
                );
            }
            if !found && repo.ref_oid(UPSTREAM_REF)?.is_some() {
                repo.delete_ref(UPSTREAM_REF)?;
            }
            status.last_fetch = Some(hstore::now_rfc3339());
        }
        let mut upstream_commit = repo.ref_oid(UPSTREAM_REF)?;
        outcome.fetched_upstream = upstream_commit.clone();
        let repo_state = format::detect(repo, upstream_commit.as_deref())?;
        repo_state.check()?;
        if repo_state == RepoState::Unmarked && !status.adopted {
            bail!(
                "{} is an existing repository without the mise marker; `mise bootstrap dotfiles origin set {}` previews how it would be adopted",
                origin.url,
                origin.url
            );
        }
        // ours is the saved version: save what is live first, so a fresh
        // machine's existing files take part in adoption
        if request.capture {
            capture_now(store, tracked);
        }
        let entries = store.list()?;
        let encrypted_paths: BTreeSet<String> = tracked
            .walk()?
            .entries
            .iter()
            .filter(|entry| entry.policy.encrypt)
            .map(|entry| display_path(&entry.path))
            .collect();
        let shared = share::current(repo, store, tracked)?;
        let unsaved = unsaved_paths(repo, tracked, &shared)?;
        let publish = mode.publishes() && !request.fetch_only && !request.offline;
        let mut plans;
        let mut attempts = 0;
        loop {
            attempts += 1;
            let upstream = reconcile::upstream_with_interaction(
                repo,
                upstream_commit.as_deref(),
                request.capture && console::user_attended_stderr(),
            )?;
            let sync_state = state::load(repo)?;
            plans = prepare(
                repo,
                tracked,
                &shared.objects(),
                &upstream,
                &sync_state,
                &unsaved,
                &mut status,
            )?;
            // Observing identical versions establishes a baseline even when
            // there is no publication (including fetch-only connections).
            let mut observed = sync_state.clone();
            for plan in &plans {
                if plan.is_noop() {
                    observed.insert(plan.branch_path.clone(), plan.next.clone());
                }
            }
            if observed != sync_state {
                state::save(repo, &observed, "observed identical versions")?;
            }
            let changes: BTreeMap<String, Option<Object>> = plans
                .iter()
                .filter_map(|plan| {
                    plan.publish
                        .clone()
                        .map(|object| (plan.branch_path.clone(), object))
                })
                .collect();
            if !publish
                || status.application_failure.is_some()
                || status.validation_error.is_some()
                // Apply incoming configuration before publishing encrypted
                // content under a potentially superseded recipient policy.
                || (matches!(repo_state, RepoState::Marked(2))
                    && plans.iter().any(|plan| is_configuration(&plan.branch_path) && plan.apply.is_some()))
                || plans.iter().any(|plan| plan.conflict.is_some())
            {
                break;
            }
            let add_marker = matches!(repo_state, RepoState::Empty | RepoState::Unmarked);
            let publication = publish::Publication {
                upstream_commit: upstream_commit.as_deref(),
                changes: super::files::publication(
                    repo,
                    upstream_commit.as_deref(),
                    &shared,
                    &changes,
                    request.capture && console::user_attended_stderr(),
                )?,
                add_marker,
                message: publish::message(&machine.name, &changes),
            };
            let Some(commit) = publish::build(repo, &publication)? else {
                break;
            };
            match publish::push(&remote, &origin.branch, &commit, upstream_commit.as_deref())? {
                PushOutcome::Done => {
                    outcome.published = Some(commit.clone());
                    status.last_publish = Some(hstore::now_rfc3339());
                    repo.update_ref(UPSTREAM_REF, &commit, upstream_commit.as_deref())?;
                    upstream_commit = Some(commit);
                    // the published plans are acknowledged now
                    let mut next_state = sync_state.clone();
                    for plan in &plans {
                        if plan.publish.is_some() {
                            next_state.insert(plan.branch_path.clone(), plan.next.clone());
                        }
                    }
                    for plan in &plans {
                        if plan.publish.is_none() && plan.apply.is_none() && plan.conflict.is_none()
                        {
                            next_state.insert(plan.branch_path.clone(), plan.next.clone());
                        }
                    }
                    state::save(repo, &next_state, "published")?;
                    // the applications and conflicts are relative to the new head
                    let upstream = reconcile::upstream_with_interaction(
                        repo,
                        upstream_commit.as_deref(),
                        request.capture && console::user_attended_stderr(),
                    )?;
                    plans = prepare(
                        repo,
                        tracked,
                        &shared.objects(),
                        &upstream,
                        &next_state,
                        &unsaved,
                        &mut status,
                    )?;
                    break;
                }
                PushOutcome::Rejected(reason) if attempts < PUSH_RETRIES => {
                    debug!("history sync: publication rejected, fetching again: {reason}");
                    remote.fetch(&origin.branch)?;
                    upstream_commit = repo.ref_oid(UPSTREAM_REF)?;
                }
                PushOutcome::Rejected(reason) => {
                    bail!("publication kept being rejected after {attempts} attempts: {reason}")
                }
            }
        }
        status.upstream_commit = upstream_commit.clone();
        record_pending(&mut status, &plans, &Roots::current(), &shared.objects());
        if publish {
            match &encryption {
                Err(err) => {
                    // recorded, not raised: the setup branch above is
                    // published either way, and nothing is ever uploaded in
                    // plaintext because the recipients are missing
                    status.backup_error = Some(format!("{err:#}"));
                    outcome.backup_error = status.backup_error.clone();
                }
                Ok(encryption) => {
                    status.backup_error = None;
                    let scheme = backup::scheme(encryption.as_ref());
                    let policy = crate::hash::hash_sha256_to_str(
                        &encrypted_paths
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    let policy_changed = status.backup_policy.as_deref() != Some(&policy)
                        && (!encrypted_paths.is_empty() || status.backup_policy.is_some());
                    if backup::scheme_changes(&status, &scheme) || policy_changed {
                        let (uploaded, replaced) = backup::replace(
                            &remote,
                            repo,
                            &entries,
                            &machine.id,
                            &mut status,
                            encryption.as_ref(),
                            &encrypted_paths,
                        )?;
                        status.backup_policy = Some(policy);
                        outcome.uploaded = uploaded;
                        outcome.replaced_remote = replaced;
                    }
                    let since = status.upload_since.clone();
                    let uploadable: Vec<Entry> = entries
                        .iter()
                        .filter(|entry| {
                            since
                                .as_deref()
                                .is_none_or(|s| entry.checkpoint.created_at.as_str() >= s)
                        })
                        .cloned()
                        .collect();
                    outcome.uploaded += backup::upload(
                        &remote,
                        repo,
                        &uploadable,
                        &machine.id,
                        &mut status.uploaded,
                        encryption.as_ref(),
                        &encrypted_paths,
                    )?;
                    outcome.pruned_remote =
                        backup::prune_remote(&remote, &entries, &machine.id, &mut status.uploaded)?;
                }
            }
        }
        outcome.pending = status.pending_applications.len();
        outcome.conflicts = status.conflicts.len();
        status.last_error = None;
        status.failing_since = None;
        status.consecutive_failures = 0;
        status.backoff_until = None;
        if !request.dry_run {
            notify_new_conflicts(&mut status);
        }
        Ok(())
    })();
    if let Err(err) = &result {
        status.pending_applications.clear();
        status.last_error = Some(format!("{err:#}"));
        status.failing_since.get_or_insert_with(hstore::now_rfc3339);
        status.consecutive_failures = status.consecutive_failures.saturating_add(1);
    }
    if let Err(write_error) = write_status(state_dir, &status) {
        return match result {
            Err(error) => Err(error.wrap_err(format!(
                "also failed to record sync health: {write_error:#}"
            ))),
            Ok(()) => Err(write_error),
        };
    }
    result.map(|()| outcome)
}

/// How long a status update waits for a running sync or pull to finish.
pub(crate) const STATUS_LOCK_WAIT: Duration = Duration::from_secs(5);
const STATUS_LOCK_POLL: Duration = Duration::from_millis(100);

fn lock_path(state_dir: &Path) -> PathBuf {
    hstore::store_dir_in(state_dir).join("sync.lock")
}

/// The sync lock: every reader-then-writer of `sync.json` holds it. A sync
/// or pull takes it for its whole duration and fails at once when another
/// holds it.
pub(crate) fn lock(store: &Store) -> Result<fslock::LockFile> {
    lock_in(store.state_dir())
}

pub(crate) fn lock_in(state_dir: &Path) -> Result<fslock::LockFile> {
    crate::lock_file::LockFile::new(&lock_path(state_dir))
        .try_lock()?
        .ok_or_else(|| eyre::eyre!("another setup sync or pull is running; retry shortly"))
}

/// Changes one thing in `sync.json` under the sync lock, reading the record
/// again first, so a sync or pull that finished meanwhile is not written
/// over with an older copy. Waits up to `wait` for a running one;
/// `Duration::ZERO` tries once.
pub(crate) fn update_status(
    state_dir: &Path,
    wait: Duration,
    mutate: impl FnOnce(&mut SyncStatus),
) -> Result<()> {
    let _lock = lock_wait(state_dir, wait)?;
    let mut status = read_status(state_dir)?;
    mutate(&mut status);
    write_status(state_dir, &status)
}

pub(crate) fn lock_wait(state_dir: &Path, wait: Duration) -> Result<fslock::LockFile> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(lock) = crate::lock_file::LockFile::new(&lock_path(state_dir)).try_lock()? {
            return Ok(lock);
        }
        if Instant::now() >= deadline {
            bail!("another setup sync or pull is running; retry shortly");
        }
        std::thread::sleep(STATUS_LOCK_POLL);
    }
}

/// A desktop notification for conflicts that newly need a decision, when
/// `history.notify` is on. Notify once per whole-setup pause, not per path.
/// Never blocks; a failure is only logged.
fn notify_new_conflicts(status: &mut SyncStatus) {
    notify_conflicts_with(
        status,
        crate::config::Settings::get().history.notify,
        crate::system::history::notify::send,
    );
}

fn notify_conflicts_with(status: &mut SyncStatus, enabled: bool, send: impl FnOnce(&str, &str)) {
    let current: BTreeSet<String> = status
        .conflicts
        .iter()
        .map(|conflict| conflict.branch_path.clone())
        .collect();
    if !current.is_empty() && !status.conflict_pause_observed && enabled {
        let roots = Roots::current();
        let lines: Vec<String> = status
            .conflicts
            .iter()
            .take(1)
            .map(|conflict| {
                let path = roots
                    .locate(&conflict.branch_path)
                    .path()
                    .map(display_path)
                    .unwrap_or_else(|| conflict.branch_path.clone());
                let mut chars = path.chars().filter(|ch| !ch.is_control());
                let mut path: String = chars.by_ref().take(80).collect();
                if chars.next().is_some() {
                    path.push('…');
                }
                path
            })
            .collect();
        let more = current.len().saturating_sub(1);
        let body = if more > 0 {
            let noun = if more == 1 { "file" } else { "files" };
            format!("{} and {more} other {noun}", lines.join(""))
        } else {
            lines.join("\n")
        };
        send(
            "mise: dotfile sync paused",
            &format!(
                "Conflicting changes in {body}.\nLocal saves still work. For resolution steps, run:\nmise bootstrap dotfiles status"
            ),
        );
    }
    status.conflict_pause_observed = !current.is_empty();
}

/// Saves the tracked set now (deduplicated against the newest checkpoint),
/// so the versions this sync publishes are what is on disk.
pub(crate) fn capture_now(store: &Store, tracked: &TrackedSet) {
    use crate::system::history::checkpoint::Draft;
    use crate::system::history::store::Trigger;
    match store.attempt(tracked, Draft::new(Trigger::Edit)) {
        Ok(_) => {}
        Err(err) => warn!("history sync: could not save the current state first: {err:#}"),
    }
}

/// Records what is left to apply or decide, keeping the newest upstream
/// version for a path whose application was already pending.
fn record_pending(
    status: &mut SyncStatus,
    plans: &[PathPlan],
    roots: &Roots,
    shared: &BTreeMap<String, Object>,
) {
    status.conflicts = plans
        .iter()
        .filter_map(|plan| plan.conflict.clone())
        .collect();
    status.pending_applications = plans
        .iter()
        .filter(|plan| plan.conflict.is_none())
        .filter_map(|plan| {
            let object = plan.apply.clone()?;
            roots.locate(&plan.branch_path).path()?;
            Some(PendingApplication {
                branch_path: plan.branch_path.clone(),
                object,
                configuration: is_configuration(&plan.branch_path),
                next: plan.next.clone(),
                local: shared.get(&plan.branch_path).cloned(),
            })
        })
        .collect();
    // said until `mise bootstrap` ran, even once the configuration is written
    status.declarations_changed = status.declarations_changed
        || status
            .pending_applications
            .iter()
            .any(|pending| pending.configuration);
}

/// Rebuild the incoming plan against current saved files and the latest
/// fetched branch. Pull never trusts an old pending plan after a save.
pub(crate) fn refresh(store: &Store, tracked: &TrackedSet, status: &mut SyncStatus) -> Result<()> {
    refresh_with_interaction(store, tracked, status, false)
}

pub(crate) fn refresh_with_interaction(
    store: &Store,
    tracked: &TrackedSet,
    status: &mut SyncStatus,
    interactive: bool,
) -> Result<()> {
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("planning requires git"))?;
    let shared = share::current(repo, store, tracked)?;
    let upstream = reconcile::upstream_with_interaction(
        repo,
        repo.ref_oid(UPSTREAM_REF)?.as_deref(),
        interactive,
    )?;
    let plans = prepare(
        repo,
        tracked,
        &shared.objects(),
        &upstream,
        &state::load(repo)?,
        &unsaved_paths(repo, tracked, &shared)?,
        status,
    )?;
    status.upstream_commit = upstream.commit;
    record_pending(status, &plans, &Roots::current(), &shared.objects());
    Ok(())
}

/// First resolve configuration, then discover its complete incoming write
/// set in memory. Publication never gets ahead of this second preflight.
fn prepare(
    repo: &crate::system::history::shadow::HistoryRepo,
    tracked: &TrackedSet,
    shared: &BTreeMap<String, Object>,
    upstream: &reconcile::Upstream,
    sync_state: &state::SyncState,
    unsaved: &BTreeSet<String>,
    status: &mut SyncStatus,
) -> Result<Vec<PathPlan>> {
    let roots = Roots::current();
    let reconcile_set = |set: &TrackedSet| {
        let selected = reconcile::Upstream {
            commit: upstream.commit.clone(),
            files: upstream
                .files
                .iter()
                .filter(|(path, _)| eligible(&roots, set, path))
                .map(|(path, object)| (path.clone(), object.clone()))
                .collect(),
        };
        let mut plans = reconcile::reconcile(repo, shared, &selected, sync_state, unsaved)?;
        // Old acknowledgements are not authority to delete a path this
        // machine no longer declares or selects.
        plans.retain(|plan| eligible(&roots, set, &plan.branch_path));
        Ok::<_, eyre::Report>(plans)
    };
    let mut plans = reconcile_set(tracked)?;
    apply_resolutions(repo, status, shared, upstream, &mut plans)?;
    status.validation_error = None;
    let has_incoming_config = plans
        .iter()
        .any(|plan| is_configuration(&plan.branch_path) && plan.apply.is_some());
    if has_incoming_config {
        let validation = (|| -> Result<()> {
            let prospective = super::preflight::prospective(repo, tracked, &plans)?;
            plans = reconcile_set(&prospective)?;
            apply_resolutions(repo, status, shared, upstream, &mut plans)?;
            super::preflight::sources(repo, &prospective, &plans)
        })();
        if let Err(error) = validation {
            status.validation_error = Some(format!("{error:#}"));
            for plan in &mut plans {
                if is_configuration(&plan.branch_path) && plan.apply.is_some() {
                    plan.conflict = Some(Conflict {
                        branch_path: plan.branch_path.clone(),
                        kind: reconcile::ConflictKind::InvalidIncoming,
                        local: shared.get(&plan.branch_path).cloned(),
                        remote: upstream.files.get(&plan.branch_path).cloned(),
                        base: plan.next.acknowledged.clone(),
                    });
                    plan.publish = None;
                }
            }
        }
    }
    Ok(plans)
}

fn apply_resolutions(
    repo: &crate::system::history::shadow::HistoryRepo,
    status: &mut SyncStatus,
    shared: &BTreeMap<String, Object>,
    upstream: &reconcile::Upstream,
    plans: &mut [PathPlan],
) -> Result<()> {
    let roots = Roots::current();
    let mut invalid = vec![];
    for (path, choice) in &status.resolutions {
        let live = match roots.locate(path).path() {
            Some(local) => match super::apply::live_object(repo, local) {
                Ok(live) => live,
                Err(_) => {
                    invalid.push(path.clone());
                    continue;
                }
            },
            None => {
                invalid.push(path.clone());
                continue;
            }
        };
        if shared.get(path) != choice.local.as_ref()
            || upstream.files.get(path) != choice.remote.as_ref()
            || live != choice.live
        {
            invalid.push(path.clone());
            continue;
        }
        if let Some(plan) = plans.iter_mut().find(|plan| plan.branch_path == *path) {
            plan.conflict = None;
            if choice.take_remote {
                plan.publish = None;
                plan.apply = Some(choice.remote.clone());
                plan.next.reconciled = choice.remote.clone();
            } else {
                plan.apply = None;
                plan.publish = Some(choice.local.clone());
                let oid = choice.local.clone();
                plan.next.acknowledged = oid.clone();
                plan.next.reconciled = oid.clone();
                plan.next.applied = oid;
            }
        }
    }
    for path in invalid {
        status.resolutions.remove(&path);
    }
    // A clean text merge is not sufficient: inspect every incoming live
    // path before any publication, so unsaved or staged edits pause the
    // whole setup instead of being discovered after other files shipped.
    for plan in plans {
        let Some(incoming) = &plan.apply else {
            continue;
        };
        let located = roots.locate(&plan.branch_path);
        let Some(path) = located.path() else {
            continue;
        };
        let mut kind = None;
        if let Some((_, oid)) = incoming
            && is_configuration(&plan.branch_path)
            && path.extension().is_some_and(|ext| ext == "toml")
            && toml::from_str::<toml::Value>(&String::from_utf8_lossy(&repo.cat_object(oid)?))
                .is_err()
        {
            kind = Some(reconcile::ConflictKind::InvalidIncoming);
        }
        match super::apply::live_object(repo, path) {
            Ok(live)
                if live.as_ref() != shared.get(&plan.branch_path)
                    && !status
                        .resolutions
                        .get(&plan.branch_path)
                        .is_some_and(|r| r.take_remote) =>
            {
                kind = kind.or(Some(reconcile::ConflictKind::UnsavedEdits));
            }
            Err(_) => kind = kind.or(Some(reconcile::ConflictKind::TypeChange)),
            _ => {}
        }
        if super::apply::git_status(path)?.is_some_and(|s| {
            s.chars()
                .next()
                .is_some_and(|c| c != ' ' && c != '?' && c != '!')
        }) {
            kind = kind.or(Some(reconcile::ConflictKind::StagedEdits));
        }
        if let Some(kind) = kind {
            plan.conflict = Some(Conflict {
                branch_path: plan.branch_path.clone(),
                kind,
                local: shared.get(&plan.branch_path).cloned(),
                remote: upstream.files.get(&plan.branch_path).cloned(),
                base: plan.next.acknowledged.clone(),
            });
            plan.publish = None;
        }
    }
    Ok(())
}

/// Whether an upstream path belongs on this machine: configuration and
/// sources always; a tracked entry's stream only when it is the one this
/// machine selects (its variant, or the base stream when it has none), so
/// another platform's version is never applied here and never read as a
/// change. Undeclared paths wait for prospective incoming configuration;
/// their absence from this machine is not a publication of a deletion.
pub(super) fn eligible(roots: &Roots, tracked: &TrackedSet, branch_path: &str) -> bool {
    match roots.locate(branch_path) {
        Located::Tracked { path, variant } => match tracked.entry_for(&path) {
            Some(entry) => entry.policy.share && entry.variant == variant,
            None => false,
        },
        Located::Config(_) | Located::Source(_) | Located::Marker => true,
        Located::Unmapped => false,
    }
}

/// A bootstrap finished: the declarations that arrived through sync are
/// applied now, so `status` stops asking for one.
pub(crate) fn bootstrap_completed() {
    let state_dir: &Path = &crate::dirs::STATE;
    let status = match read_status(state_dir) {
        Ok(status) => status,
        Err(err) => {
            warn!("history: could not record that the bootstrap ran: {err:#}");
            return;
        }
    };
    if !status.declarations_changed {
        return;
    }
    // under the sync lock, changing only this: a sync that finished during
    // the bootstrap keeps its conflicts, pending changes, and uploads
    if let Err(err) = update_status(state_dir, STATUS_LOCK_WAIT, |status| {
        status.declarations_changed = false;
    }) {
        debug!("history: could not record that the bootstrap ran: {err}");
    }
}

/// Manual-save entries whose live file differs from the saved version:
/// an incoming change there is held instead of applied.
fn unsaved_paths(
    repo: &crate::system::history::shadow::HistoryRepo,
    tracked: &TrackedSet,
    shared: &share::ShareReport,
) -> Result<BTreeSet<String>> {
    let Some(checkpoint) = shared.checkpoint.as_deref() else {
        return Ok(BTreeSet::new());
    };
    let walk = tracked.walk()?;
    let mut unsaved = BTreeSet::new();
    for (branch_path, file) in &shared.files {
        let Some((_, policy)) = walk.files.get(&file.local) else {
            continue;
        };
        if policy.autosave {
            continue;
        }
        let live = match super::apply::live_object(repo, &file.local) {
            Ok(live) => live,
            Err(_) => {
                // A changed type or unreadable live file must hold incoming
                // application, not abort fetching and eligible backups.
                unsaved.insert(branch_path.clone());
                continue;
            }
        };
        if live != Some((file.mode.clone(), file.oid.clone())) {
            debug!(
                "history sync: {} has unsaved edits (checkpoint {})",
                display_path(&file.local),
                checkpoint
            );
            unsaved.insert(branch_path.clone());
        }
    }
    Ok(unsaved)
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    fn conflict(path: &str) -> Conflict {
        Conflict {
            branch_path: path.into(),
            kind: reconcile::ConflictKind::SameHunk,
            local: None,
            remote: None,
            base: None,
        }
    }

    #[test]
    fn one_notification_per_pause_and_another_after_recovery() {
        let mut status = SyncStatus {
            conflicts: vec![conflict("tracked/home/.zshrc")],
            ..Default::default()
        };
        let mut calls = 0;
        notify_conflicts_with(&mut status, true, |title, body| {
            assert!(title.contains("dotfile sync paused"));
            assert!(body.contains(".zshrc"));
            calls += 1;
        });
        status.conflicts.push(conflict("tracked/home/.gitconfig"));
        notify_conflicts_with(&mut status, true, |_, _| calls += 1);
        status.conflicts.remove(0);
        notify_conflicts_with(&mut status, true, |_, _| calls += 1);
        assert_eq!(calls, 1);
        status.conflicts.clear();
        notify_conflicts_with(&mut status, true, |_, _| calls += 1);
        assert!(!status.conflict_pause_observed);
        status.conflicts.push(conflict("tracked/home/.gitconfig"));
        notify_conflicts_with(&mut status, true, |_, _| calls += 1);
        assert_eq!(calls, 2);
    }

    #[test]
    fn notification_keeps_the_action_visible_for_long_or_unusual_paths() {
        let mut status = SyncStatus {
            conflicts: vec![
                conflict(&format!("tracked/home/\n{}", "x".repeat(200))),
                conflict("tracked/home/.gitconfig"),
            ],
            ..Default::default()
        };
        notify_conflicts_with(&mut status, true, |_, body| {
            assert!(body.contains("… and 1 other file"));
            assert!(body.contains("Local saves still work."));
            assert!(body.ends_with("mise bootstrap dotfiles status"));
            assert_eq!(body.lines().count(), 3);
            assert!(body.chars().count() < 250);
        });
    }

    #[test]
    fn explicit_opt_out_is_preserved() {
        let mut status = SyncStatus {
            conflicts: vec![conflict("tracked/home/.zshrc")],
            ..Default::default()
        };
        notify_conflicts_with(&mut status, false, |_, _| panic!("notifications disabled"));
        assert!(status.conflict_pause_observed);
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn conflict() -> Conflict {
        Conflict {
            branch_path: "tracked/home/.zshrc".to_string(),
            kind: reconcile::ConflictKind::SameHunk,
            local: None,
            remote: None,
            base: None,
        }
    }

    #[test]
    fn omitted_failure_fields_default_to_no_failures() {
        let status: SyncStatus = serde_json::from_str("{}").unwrap();
        assert_eq!(status.failing_since, None);
        assert_eq!(status.consecutive_failures, 0);
        let text = serde_json::to_string(&status).unwrap();
        assert!(!text.contains("consecutive_failures"));
    }

    #[test]
    fn an_update_changes_only_its_field_in_the_current_record() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path();
        let mut status = SyncStatus {
            declarations_changed: true,
            last_error: Some("unreachable".to_string()),
            ..Default::default()
        };
        write_status(state_dir, &status).unwrap();
        // a sync finishes meanwhile and records a conflict: the update reads
        // that record, not the one it started from
        status.conflicts.push(conflict());
        write_status(state_dir, &status).unwrap();
        update_status(state_dir, Duration::ZERO, |status| {
            status.declarations_changed = false;
        })
        .unwrap();
        let current = read_status(state_dir).unwrap();
        assert!(!current.declarations_changed);
        assert_eq!(current.conflicts.len(), 1);
        assert_eq!(current.last_error.as_deref(), Some("unreachable"));
    }

    #[test]
    fn an_update_gives_way_to_a_running_sync() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path();
        std::fs::create_dir_all(hstore::store_dir_in(state_dir)).unwrap();
        let held = lock_in(state_dir).unwrap();
        let err = update_status(state_dir, Duration::ZERO, |status| {
            status.declarations_changed = false;
        })
        .unwrap_err();
        assert!(err.to_string().contains("another setup sync or pull"));
        drop(held);
        update_status(state_dir, Duration::ZERO, |status| {
            status.declarations_changed = false;
        })
        .unwrap();
    }
}
