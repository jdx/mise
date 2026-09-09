//! One synchronization: fetch the ordinary origin branch, reconcile against
//! Git ancestry, and publish without rewriting history (on a rejection, fetch
//! again and retry). Record operational health and derive `sync.json` with
//! incoming changes to apply and conflicts to decide.
//! Captures never wait on the network; this never changes a
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
use super::{publish, share, state};
use crate::file::display_path;
use crate::system::history::checkpoint::Store;
use crate::system::history::config::OriginTomlConfig;
use crate::system::history::store as hstore;
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
    pub local_mode: Option<u32>,
    pub remote_mode: Option<u32>,
    pub live_mode: Option<u32>,
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
    pub conflicts: Vec<Conflict>,
    #[serde(default)]
    pub pending_applications: Vec<PendingApplication>,
    /// Repository metadata and inactive streams can change without live writes.
    #[serde(default)]
    pub pending_repository: bool,
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
        assert!(read_status(dir.path()).unwrap().conflicts.is_empty());
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
        return Ok(OriginTomlConfig::plain(url, branch));
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
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("synchronizing requires git"))?;
    let mode = SyncMode::current()?;
    let state_dir = store.state_dir();
    let mut status = read_status(state_dir)?;
    let remote = Remote::new(repo, &origin.url);
    let mut outcome = SyncOutcome::default();

    let result = (|| -> Result<()> {
        if !request.offline {
            // a branch that vanished from a repository this machine had
            // synced with is not an empty upstream: reading it as one would
            // queue the deletion of every file it held
            let found = remote.fetch(&origin.branch)?;
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
        if request.capture
            && !(repo
                .ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?
                .is_none()
                && upstream_commit.is_some()
                && tracked.manifest.enrollment.is_empty())
        {
            capture_now(store, tracked);
        }
        let mut shared = share::current(repo, tracked)?;
        let unsaved = unsaved_paths(repo, tracked, &shared)?;
        let publish = mode.publishes() && !request.fetch_only && !request.offline;
        if publish
            && let Some(head) =
                repo.ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?
        {
            audit_publication(repo, &head, tracked)?;
        }
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
            if !publish
                || status.pending_repository
                || status.application_failure.is_some()
                || status.validation_error.is_some()
                // Publication must wait for the entire incoming setup, not
                // just configuration or encrypted paths. A merge cannot be
                // acknowledged while its live application is still pending.
                || plans.iter().any(|plan| plan.apply.is_some())
                || plans.iter().any(|plan| plan.conflict.is_some())
            {
                break;
            }
            let accepted = plans
                .iter()
                .filter(|plan| plan.conflict.is_none() && plan.apply.is_none())
                .map(|plan| plan.branch_path.clone())
                .collect();
            let Some(commit) = publish::build(
                repo,
                upstream_commit.as_deref(),
                shared.checkpoint.as_deref(),
                &accepted,
            )?
            else {
                break;
            };
            // The candidate may be our own new merge commit. A rejected
            // push must retry from it, without treating it as another
            // writer's change or dropping either parent history.
            shared.checkpoint = Some(commit.clone());
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
        outcome.pending =
            status.pending_applications.len() + usize::from(status.pending_repository);
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
        if !request.dry_run {
            notify_new_conflicts(&mut status);
        }
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

fn audit_publication(
    repo: &crate::system::history::shadow::HistoryRepo,
    head: &str,
    tracked: &TrackedSet,
) -> Result<()> {
    let protected = tracked
        .entries
        .iter()
        .filter(|entry| entry.policy.encrypt)
        .map(|entry| entry.tree_path(&entry.path))
        .collect::<Result<_>>()?;
    super::files::audit_history(repo, head, &protected)
}

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
    crate::lock_file::LockFile::at(&lock_path(state_dir))
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
        if let Some(lock) = crate::lock_file::LockFile::at(&lock_path(state_dir)).try_lock()? {
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
    let capture = || -> Result<()> {
        let Some(_operation) = crate::system::history::scope::try_operation_lock(store, tracked)?
        else {
            debug!("history sync: a write operation is active; keeping the existing saved commits");
            return Ok(());
        };
        store.attempt(tracked, Draft::new(Trigger::Edit))?;
        Ok(())
    };
    match capture() {
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
    let shared = share::current(repo, tracked)?;
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
    unsaved: &BTreeSet<String>,
    status: &mut SyncStatus,
) -> Result<Vec<PathPlan>> {
    let incoming =
        incoming_tracking(repo, tracked).inspect_err(|error| repository_conflict(status, error))?;
    let tracked = &incoming;
    let roots = Roots::current();
    // Git ancestry, not a machine's replaceable acknowledgement cache,
    // determines the shared baseline. Losing local bookkeeping must not
    // turn an ordinary incoming edit into an unrelated adoption conflict.
    let heads = super::graph::Heads::read(repo)?;
    status.upstream_commit = heads.remote.clone();
    status.pending_repository = match &heads.local {
        Some(local) => {
            crate::system::history::manifest::Manifest::read(repo, local)?.as_ref()
                != Some(&tracked.manifest)
        }
        None => heads.remote.is_some(),
    };
    let repository_tree = incoming_repository_tree(repo, tracked)
        .inspect_err(|error| repository_conflict(status, error))?;
    status.pending_repository |= repository_tree.is_some();
    if let Some(tree) = repository_tree.as_deref().or(heads.remote.as_deref()) {
        super::directories::plan(repo, tracked, tree)
            .inspect_err(|error| repository_conflict(status, error))?;
    }
    if heads.remote != upstream.commit {
        bail!("origin changed while planning; reconcile again");
    }
    let baseline = reconcile::upstream(repo, heads.base.as_deref())?;
    let local_manifest = heads
        .local
        .as_deref()
        .map(|head| crate::system::history::manifest::Manifest::read(repo, head))
        .transpose()?
        .flatten()
        .unwrap_or_default();
    let sync_state: state::SyncState = baseline
        .files
        .into_iter()
        .map(|(path, object)| {
            (
                path,
                state::SyncRecord {
                    acknowledged: Some(object.clone()),
                    reconciled: Some(object.clone()),
                    applied: Some(object),
                    upstream_commit: heads.base.clone(),
                },
            )
        })
        .collect();
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
        let mut plans = reconcile::reconcile(repo, shared, &selected, &sync_state, unsaved)?;
        // Old acknowledgements are not authority to delete a path this
        // machine no longer declares or selects.
        plans.retain(|plan| eligible(&roots, set, &plan.branch_path));
        for (path, object) in shared {
            if !eligible(&roots, set, path)
                || local_manifest.file_permissions(path, Some(object))
                    == set.manifest.file_permissions(path, Some(object))
            {
                continue;
            }
            if let Some(plan) = plans.iter_mut().find(|plan| plan.branch_path == *path) {
                if plan.conflict.is_none() && plan.apply.is_none() {
                    plan.apply = Some(Some(object.clone()));
                }
            } else {
                plans.push(PathPlan {
                    branch_path: path.clone(),
                    apply: Some(Some(object.clone())),
                    next: state::SyncRecord {
                        acknowledged: Some(object.clone()),
                        reconciled: Some(object.clone()),
                        applied: Some(object.clone()),
                        upstream_commit: upstream.commit.clone(),
                    },
                    ..Default::default()
                });
            }
        }
        Ok::<_, eyre::Report>(plans)
    };
    let mut plans = reconcile_set(tracked)?;
    apply_resolutions(repo, status, shared, upstream, &mut plans)?;
    status.validation_error = None;
    // Source deletion and invalid source types matter even when the bootstrap
    // configuration itself is unchanged or deliberately not tracked.
    if plans.iter().any(|plan| plan.apply.is_some()) {
        let validation = (|| -> Result<()> {
            let prospective = super::preflight::prospective(repo, tracked, &plans)?;
            plans = reconcile_set(&prospective)?;
            apply_resolutions(repo, status, shared, upstream, &mut plans)?;
            super::preflight::sources(repo, &prospective, &plans)
        })();
        if let Err(error) = validation {
            status.validation_error = Some(format!("{error:#}"));
            for plan in &mut plans {
                if plan.apply.is_some() {
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

fn repository_conflict(status: &mut SyncStatus, error: &eyre::Report) {
    status.pending_repository = true;
    status.validation_error = Some(format!("{error:#}"));
    status.conflicts = vec![Conflict {
        branch_path: crate::system::history::manifest::PATH.into(),
        kind: reconcile::ConflictKind::Repository,
        local: None,
        remote: None,
        base: None,
    }];
}

fn apply_resolutions(
    repo: &crate::system::history::shadow::HistoryRepo,
    status: &mut SyncStatus,
    shared: &BTreeMap<String, Object>,
    upstream: &reconcile::Upstream,
    plans: &mut [PathPlan],
) -> Result<()> {
    let roots = Roots::current();
    let local_head = repo.ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?;
    let saved_manifest = local_head
        .as_deref()
        .map(|head| crate::system::history::manifest::Manifest::read(repo, head))
        .transpose()?
        .flatten()
        .unwrap_or_default();
    let remote_manifest = upstream
        .commit
        .as_deref()
        .map(|head| crate::system::history::manifest::Manifest::read(repo, head))
        .transpose()?
        .flatten()
        .unwrap_or_default();
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
            || saved_manifest.file_permissions(path, shared.get(path)) != choice.local_mode
            || remote_manifest.file_permissions(path, upstream.files.get(path))
                != choice.remote_mode
            || roots
                .locate(path)
                .path()
                .map(super::apply::live_permissions)
                .transpose()?
                .flatten()
                != choice.live_mode
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
    let fresh = repo
        .ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?
        .is_none();
    let incoming_paths: Vec<_> = plans
        .iter()
        .filter(|plan| plan.apply.is_some())
        .filter_map(|plan| {
            roots
                .locate(&plan.branch_path)
                .path()
                .map(Path::to_path_buf)
        })
        .collect();
    let staged = super::apply::staged_paths(incoming_paths.iter().map(PathBuf::as_path))?;
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
                    && !(fresh && live == *incoming)
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
        if !fresh
            && super::apply::live_permissions(path)?
                != saved_manifest.file_permissions(&plan.branch_path, shared.get(&plan.branch_path))
            && !status
                .resolutions
                .get(&plan.branch_path)
                .is_some_and(|r| r.take_remote)
        {
            kind = kind.or(Some(reconcile::ConflictKind::UnsavedEdits));
        }
        if super::apply::has_staged_changes(&staged, path) {
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
            Some(entry) => entry.variant == variant,
            None => false,
        },
        Located::Config(path) => tracked
            .entry_for(&path)
            .is_some_and(|entry| entry.variant.is_none()),
        Located::Marker => false,
        Located::Unmapped => false,
    }
}

/// A fresh store gets its explicit inventory from the fetched ordinary branch.
/// Do not create an unrelated local root just to discover incoming files.
pub(super) fn incoming_tracking(
    repo: &crate::system::history::shadow::HistoryRepo,
    tracked: &TrackedSet,
) -> Result<TrackedSet> {
    use crate::system::history::manifest::Manifest;
    let heads = super::graph::Heads::read(repo)?;
    let Some(remote) = &heads.remote else {
        return Ok(tracked.clone());
    };
    let remote = Manifest::read(repo, remote)?
        .ok_or_else(|| eyre::eyre!("setup repository has no enrollment metadata"))?;
    let manifest = match (&heads.local, &heads.base) {
        (None, _) => remote,
        (Some(local), Some(base)) => Manifest::merge(
            &Manifest::read(repo, base)?.unwrap_or_default(),
            &Manifest::read(repo, local)?.unwrap_or_default(),
            &remote,
        )?,
        (Some(_), None) => bail!(
            "multiple merge bases require explicit Git reconciliation before applying enrollment"
        ),
    };
    let mut incoming = manifest.tracking()?;
    incoming.required_sources = tracked.required_sources.clone();
    incoming.invalid = tracked.invalid.clone();
    Ok(incoming)
}

/// Complete-tree changes outside active native files still need adoption:
/// enrollment, inactive variants, and ordinary repository documentation.
pub(super) fn incoming_repository_tree(
    repo: &crate::system::history::shadow::HistoryRepo,
    tracked: &TrackedSet,
) -> Result<Option<String>> {
    let heads = super::graph::Heads::read(repo)?;
    let (Some(local), Some(remote)) = (&heads.local, &heads.remote) else {
        return Ok(None);
    };
    if heads.base.as_ref() == Some(remote) {
        return Ok(None);
    }
    let (merged, conflicts) = repo.merge_tree(local, remote)?;
    let tree = tracked.manifest.write(repo, &merged)?;
    let roots = Roots::current();
    if repo.output_tree_of(local)? == tree && conflicts.is_empty() {
        return Ok(None);
    }
    let unresolved: Vec<_> = conflicts
        .into_iter()
        .filter(|path| {
            path != crate::system::history::manifest::PATH && !eligible(&roots, tracked, path)
        })
        .collect();
    if !unresolved.is_empty() {
        bail!(
            "repository application paused: reconcile repository conflicts first: {}",
            unresolved.join(", ")
        );
    }
    Ok(Some(tree))
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
                // application, not abort fetching.
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
mod capture_tests {
    use super::*;

    #[test]
    fn publication_checks_live_variant_encryption_even_without_a_new_capture() -> Result<()> {
        use crate::system::history::tracked::TrackedEntry;
        let temp = tempfile::tempdir()?;
        let store = Store::open_in(temp.path())?;
        let repo = store.repo().unwrap();
        let mut policy =
            crate::system::files::FilePolicy::for_mode(crate::system::files::FileMode::Track);
        policy.encrypt = true;
        let mut entry = TrackedEntry::new(
            Roots::current().home.join("variant-secret"),
            "track",
            policy,
        );
        entry.variant = Some("macos".into());
        let path = entry.tree_path(&entry.path)?;
        assert_eq!(path, "home@macos/variant-secret");
        let tree = repo.write_tree(&[("100644".into(), repo.hash_blob(b"plaintext")?, path)])?;
        // No encrypted manifest was captured yet: the live policy alone must
        // protect the stream when capture was skipped or failed.
        let head = repo.commit_tree(&tree, vec![], "previous plaintext version")?;
        let tracked = TrackedSet {
            entries: vec![entry],
            ..Default::default()
        };
        assert!(audit_publication(repo, &head, &tracked).is_err());
        Ok(())
    }

    #[test]
    fn unsaved_observation_only_conflicts_when_there_is_an_incoming_write() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open_in(temp.path())?;
        let repo = store.repo().ok_or_else(|| eyre::eyre!("git required"))?;
        let path = format!(
            "home/.mise-test-{}",
            crate::system::history::store::new_uuid()
        );
        let saved = ("100644".into(), repo.hash_blob(b"saved")?);
        let incoming = ("100644".into(), repo.hash_blob(b"incoming")?);
        let shared = BTreeMap::from([(path.clone(), saved.clone())]);
        let upstream = reconcile::Upstream::default();
        let mut status = SyncStatus::default();
        let mut plans = vec![PathPlan {
            branch_path: path,
            publish: Some(Some(saved)),
            ..Default::default()
        }];
        // The missing live file differs from its saved object, just as an
        // autosave edit still waiting in the debounce queue does. Publishing
        // saved commits must not inspect it when no incoming write is planned.
        apply_resolutions(repo, &mut status, &shared, &upstream, &mut plans)?;
        assert!(plans[0].conflict.is_none());
        assert!(plans[0].publish.is_some());

        // Conversely, exempting autosave entries from the check would lose
        // their pending edits when there really is an incoming write.
        plans[0].apply = Some(Some(incoming));
        apply_resolutions(repo, &mut status, &shared, &upstream, &mut plans)?;
        assert_eq!(
            plans[0].conflict.as_ref().map(|c| c.kind),
            Some(reconcile::ConflictKind::UnsavedEdits)
        );
        Ok(())
    }

    #[test]
    fn sync_capture_does_not_observe_an_active_write_operation() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open_in(temp.path())?;
        let repo = store.repo().ok_or_else(|| eyre::eyre!("git required"))?;
        let roots = Roots::current();
        let live = tempfile::Builder::new()
            .prefix(".mise-lock-test-")
            .tempdir_in(&roots.home)?;
        let path = live.path().join("config");
        std::fs::write(&path, "explicitly enrolled")?;
        let tracked = crate::system::history::manifest::Manifest {
            enrollment: vec![crate::system::history::manifest::Enrollment {
                path: roots.branch_path(&path, None).unwrap(),
                autosave: true,
                encrypt: false,
                variants: vec![],
            }],
            ..Default::default()
        }
        .tracking()?;
        let operation = crate::system::history::scope::take_operation_lock(&store, &tracked)?;
        capture_now(&store, &tracked);
        assert_eq!(
            repo.ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?,
            None
        );
        drop(operation);
        capture_now(&store, &tracked);
        assert!(
            repo.ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?
                .is_some()
        );
        Ok(())
    }
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
