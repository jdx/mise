//! All-or-nothing application of incoming changes: `mise bootstrap dotfiles pull`.
//! The complete setup is preflighted; an incoming configuration file is validated
//! before it is written; a path with unsaved local edits, staged git
//! changes, or a genuine local edit is held for a decision. Application
//! is the same recoverable transaction as a rollback: the write set is
//! planned, its preimages captured in a protective checkpoint, every file
//! written one at a time and journaled, and reload hooks run only once
//! everything succeeded.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eyre::{Result, bail};

use super::layout::Roots;
use super::reconcile::{Conflict, Object};
use super::run::{self, PendingApplication};
use super::state;
use crate::file::display_path;
use crate::system::history::checkpoint::Store;
use crate::system::history::journal;
use crate::system::history::replay;
use crate::system::history::scope::OperationScope;
use crate::system::history::store::{OperationKind, Summary};
use crate::system::history::tracked::{TrackedSet, normalize_target};
use crate::ui::table::MiseTable;

#[derive(Clone, Debug)]
pub(crate) struct ApplyRequest {
    /// Only these local paths (empty: everything pending).
    pub paths: Vec<PathBuf>,
    pub dry_run: bool,
    pub yes: bool,
    /// Resolve these conflicts with the upstream version.
    pub take_remote: Vec<PathBuf>,
    /// Resolve these conflicts by publishing the local version next.
    pub keep_local: Vec<PathBuf>,
    /// The watcher applying in the background: no prompt, no plan on
    /// stdout, and held paths are a count, not a failure.
    pub automatic: bool,
    /// With `dry_run`: the plan is shown before a question, so no
    /// "nothing was changed" note.
    pub plan_only: bool,
}

impl ApplyRequest {
    pub(crate) fn automatic() -> Self {
        Self {
            paths: vec![],
            dry_run: false,
            yes: true,
            take_remote: vec![],
            keep_local: vec![],
            automatic: true,
            plan_only: false,
        }
    }
}

/// What an application did.
#[derive(Debug, Default, Clone)]
pub(crate) struct ApplyOutcome {
    /// Files written or removed.
    pub written: usize,
    /// Paths held for a decision (with their groups).
    pub held: usize,
    /// A configuration file was written: declarations may have changed.
    pub configuration: bool,
}

/// Why a pending application is not written now.
#[derive(Clone, Debug)]
struct Hold {
    path: PathBuf,
    reason: String,
}

struct Step {
    pending: PendingApplication,
    path: PathBuf,
    group: String,
    exists: bool,
    /// The complete live object when planned, verified before each write.
    before: Option<Object>,
    permissions: Option<std::fs::Permissions>,
}

pub(crate) async fn apply(
    store: &Store,
    tracked: &TrackedSet,
    req: &ApplyRequest,
) -> Result<ApplyOutcome> {
    let _sync_lock = run::lock(store)?;
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("applying requires git"))?;
    let state_dir = store.state_dir();
    let mut status = run::read_status(state_dir)?;
    if req.automatic && status.application_failure.is_some() {
        return Ok(ApplyOutcome {
            held: status.pending_applications.len().max(1),
            ..Default::default()
        });
    }
    run::refresh_with_interaction(
        store,
        tracked,
        &mut status,
        !req.automatic && console::user_attended_stderr(),
    )?;
    let roots = Roots::current();
    let filter: BTreeSet<PathBuf> = req
        .paths
        .iter()
        .map(|path| normalize_target(path))
        .collect();
    let take_remote: BTreeSet<PathBuf> = req
        .take_remote
        .iter()
        .map(|path| normalize_target(path))
        .collect();
    let keep_local: BTreeSet<PathBuf> = req
        .keep_local
        .iter()
        .map(|path| normalize_target(path))
        .collect();

    // Store choices without publishing or applying any part of the setup.
    let mut sync_state = state::load(repo)?;
    let shared = super::share::current(repo, store, tracked)?.objects();
    let encrypted = super::files::encrypted_paths(repo, status.upstream_commit.as_deref())?;
    for conflict in &status.conflicts {
        let Some(local) = roots
            .locate(&conflict.branch_path)
            .path()
            .map(Path::to_path_buf)
        else {
            continue;
        };
        if take_remote.contains(&local) || keep_local.contains(&local) {
            if take_remote.contains(&local) && keep_local.contains(&local) {
                bail!("choose only one resolution for {}", display_path(&local));
            }
            let live = live_object(repo, &local)?;
            let saved = shared.get(&conflict.branch_path).cloned();
            if keep_local.contains(&local) && live != saved {
                bail!("save {} before choosing --keep-local", display_path(&local));
            }
            let remote = match status.upstream_commit.as_deref() {
                Some(head) => repo
                    .object_at(head, &conflict.branch_path)?
                    .map(|object| {
                        if !encrypted.contains(&conflict.branch_path) {
                            return Ok(object);
                        }
                        super::files::decrypt(
                            repo,
                            &conflict.branch_path,
                            &object,
                            !req.automatic && console::user_attended_stderr(),
                        )
                    })
                    .transpose()?,
                None => None,
            };
            status.resolutions.insert(
                conflict.branch_path.clone(),
                run::Resolution {
                    local: saved,
                    remote,
                    live,
                    take_remote: take_remote.contains(&local),
                },
            );
        }
    }
    for path in take_remote.iter().chain(keep_local.iter()) {
        if !status
            .conflicts
            .iter()
            .any(|conflict| roots.locate(&conflict.branch_path).path() == Some(path.as_path()))
        {
            bail!("{} is not a conflict", display_path(path));
        }
    }
    run::refresh(store, tracked, &mut status)?;
    if !req.dry_run {
        run::write_status(state_dir, &status)?;
    }
    if !status.conflicts.is_empty() {
        if req.dry_run {
            // Preview the whole paused setup even when reconciliation
            // cannot yet produce an applicable write set.
            let mut table = MiseTable::new(false, &["Path", "Action", "Group"]);
            for conflict in &status.conflicts {
                if let Some(path) = roots.locate(&conflict.branch_path).path() {
                    table.add_row(vec![
                        display_path(path),
                        "held: unresolved conflict".to_string(),
                        conflict.branch_path.clone(),
                    ]);
                }
            }
            for pending in &status.pending_applications {
                if let Some(path) = roots.locate(&pending.branch_path).path() {
                    table.add_row(vec![
                        display_path(path),
                        "held: sharing paused for the entire setup".to_string(),
                        pending.branch_path.clone(),
                    ]);
                }
            }
            table.print()?;
        }
        if take_remote.is_empty() && keep_local.is_empty() && !req.dry_run && !req.automatic {
            bail!(
                "sync paused: resolve all {} conflict(s) before sharing resumes",
                status.conflicts.len()
            );
        }
        info!(
            "sync paused: {} conflict(s) remain; no files applied or published",
            status.conflicts.len()
        );
        return Ok(ApplyOutcome {
            held: status.conflicts.len(),
            ..Default::default()
        });
    }

    // the write set
    let mut steps = vec![];
    let mut holds: Vec<Hold> = vec![];
    for pending in &status.pending_applications {
        let Some(path) = roots
            .locate(&pending.branch_path)
            .path()
            .map(Path::to_path_buf)
        else {
            continue;
        };
        if !filter.is_empty() && !filter.iter().any(|f| path.starts_with(f)) {
            bail!(
                "partial pulls are not supported: apply the complete setup without PATH arguments"
            );
        }
        let group = if pending.configuration || pending.branch_path.starts_with("sources/") {
            "configuration".to_string()
        } else {
            pending.branch_path.clone()
        };
        steps.push(Step {
            pending: pending.clone(),
            exists: path.exists() || path.is_symlink(),
            before: live_object(repo, &path)?,
            permissions: std::fs::symlink_metadata(&path)
                .ok()
                .map(|meta| meta.permissions()),
            path,
            group,
        });
    }
    if steps.is_empty() {
        if !req.dry_run && !req.automatic {
            status.application_failure = None;
            run::write_status(state_dir, &status)?;
        }
        if !req.automatic {
            info!("history: nothing to apply");
        }
        return Ok(ApplyOutcome::default());
    }

    // validation and holds, per group
    let mut held_groups: BTreeSet<String> = BTreeSet::new();
    for step in &steps {
        let take_remote = status
            .resolutions
            .get(&step.pending.branch_path)
            .is_some_and(|choice| choice.take_remote);
        if let Some(reason) = hold_reason(repo, tracked, &sync_state, step, take_remote)? {
            holds.push(Hold {
                path: step.path.clone(),
                reason,
            });
            held_groups.insert(step.group.clone());
        }
    }
    let (ready, held): (Vec<&Step>, Vec<&Step>) =
        steps.iter().partition(|_| held_groups.is_empty());

    // the plan
    let mut table = MiseTable::new(false, &["Path", "Action", "Group"]);
    for step in &ready {
        let action = match (&step.pending.object, step.exists) {
            (Some(_), true) => "write",
            (Some(_), false) => "create",
            (None, _) => "delete",
        };
        table.add_row(vec![
            display_path(&step.path),
            action.to_string(),
            step.group.clone(),
        ]);
    }
    for step in &held {
        let reason = holds
            .iter()
            .find(|hold| hold.path == step.path)
            .map(|hold| hold.reason.clone())
            .unwrap_or_else(|| "sharing paused for the entire setup".to_string());
        table.add_row(vec![
            display_path(&step.path),
            format!("held: {reason}"),
            step.group.clone(),
        ]);
    }
    if !req.automatic {
        table.print()?;
    }
    if req.dry_run {
        if !req.plan_only {
            miseprintln!("history: dry run; nothing was changed");
        }
        return Ok(ApplyOutcome::default());
    }
    if ready.is_empty() {
        if req.automatic {
            return Ok(ApplyOutcome {
                held: held.len(),
                ..Default::default()
            });
        }
        bail!("nothing can be applied until the held paths are decided");
    }
    if !req.automatic
        && !super::origin::confirmed(req.yes, "history: apply these incoming changes?")?
    {
        info!("history: skipped");
        return Ok(ApplyOutcome::default());
    }

    // the transaction
    let reload = crate::system::history::config::reload_commands()?;
    let scope = if req.automatic {
        OperationScope::begin_automatic_apply().await?
    } else {
        OperationScope::begin_kind(OperationKind::Apply, "dotfiles pull", false).await?
    };
    scope.with_operation(|op| {
        op.applied = status.upstream_commit.clone();
    });
    let mut touched = vec![];
    let result = (|| -> Result<()> {
        // Validate the complete batch again after acquiring the operation
        // lock, before the first write.
        for step in &ready {
            if live_object(repo, &step.path)? != step.before {
                bail!(
                    "{} changed before application; nothing was written",
                    display_path(&step.path)
                );
            }
        }
        for step in &ready {
            // the file may have changed since the plan was made: an edit
            // that landed meanwhile is never overwritten (undo would bring
            // back the planned version, not it)
            if live_object(repo, &step.path)? != step.before {
                bail!(
                    "{} changed while the changes were being applied; nothing more was written. Run `mise bootstrap dotfiles pull` again",
                    display_path(&step.path)
                );
            }
            let pending =
                journal::begin_changes("history", &display_path(&step.path), [step.path.clone()])?;
            touched.push(step.path.clone());
            let affected = display_path(&step.path);
            scope.with_operation(|op| op.affected.push(affected.clone()));
            match &step.pending.object {
                Some((mode, oid)) => replay::write_path(repo, &step.path, mode, oid)?,
                None => replay::remove(&step.path)?,
            }
            journal::commit_changes(pending);
            let mut next = step.pending.next.clone();
            let oid = step.pending.object.clone();
            next.applied = oid.clone();
            next.acknowledged = oid;
            sync_state.insert(step.pending.branch_path.clone(), next);
        }
        Ok(())
    })();
    if let Err(error) = &result {
        let mut recovery_errors = vec![];
        for step in ready
            .iter()
            .rev()
            .filter(|step| touched.contains(&step.path))
        {
            if let Err(err) = recover_step(repo, step) {
                recovery_errors.push(format!("{err:#}"));
            }
        }
        status.application_failure = Some(format!(
            "application failed ({:#}); {}. Sharing is paused. Inspect `mise bootstrap dotfiles history` and retry `mise bootstrap dotfiles pull`",
            error,
            if recovery_errors.is_empty() {
                "previous files restored".into()
            } else {
                recovery_errors.join("; ")
            }
        ));
        run::write_status(state_dir, &status)?;
        scope.finish(
            status.application_failure.clone(),
            Some(Summary {
                message: Some("apply failed; recovery attempted".into()),
            }),
        );
        return result.map(|()| ApplyOutcome::default());
    }
    let (error, summary) = match &result {
        Ok(()) => {
            scope.promote(&touched);
            (
                None,
                Summary {
                    message: Some(format!("applied {} incoming change(s)", touched.len())),
                },
            )
        }
        Err(err) => (
            Some(format!("{err:#}")),
            Summary {
                message: Some("apply failed".into()),
            },
        ),
    };
    state::save(repo, &sync_state, "applied")?;
    let applied: BTreeSet<String> = ready
        .iter()
        .filter(|step| touched.contains(&step.path))
        .map(|step| step.pending.branch_path.clone())
        .collect();
    status
        .pending_applications
        .retain(|pending| !applied.contains(&pending.branch_path));
    status.resolutions.retain(|path, _| !applied.contains(path));
    status.application_failure = None;
    status.conflict_pause_observed = false;
    // the live declarations changed now: said until `mise bootstrap` ran
    let configuration_written = ready
        .iter()
        .any(|step| step.pending.configuration && touched.contains(&step.path));
    status.declarations_changed = status.declarations_changed
        || configuration_written
        || status
            .pending_applications
            .iter()
            .any(|pending| pending.configuration);
    if !touched.is_empty() {
        status.last_apply = Some(crate::system::history::store::now_rfc3339());
    }
    run::write_status(state_dir, &status)?;
    scope.finish(error, Some(summary));
    result?;
    replay::run_reload(&reload, &touched);
    let configuration = ready.iter().any(|step| step.pending.configuration);
    if configuration && !req.automatic {
        info!(
            "history: configuration changed; declarations may differ from the applied setup: run `mise bootstrap --dry-run`"
        );
    }
    if !req.automatic {
        info!("history: applied {} incoming change(s)", touched.len());
    }
    let outcome = ApplyOutcome {
        written: touched.len(),
        held: held.len(),
        configuration,
    };
    Ok(outcome)
}

fn recover_step(repo: &crate::system::history::shadow::HistoryRepo, step: &Step) -> Result<()> {
    let current = live_object(repo, &step.path)?;
    if current == step.before {
        return Ok(());
    }
    if current != step.pending.object {
        bail!(
            "{} changed during recovery; left untouched",
            display_path(&step.path)
        );
    }
    match &step.before {
        Some((mode, oid)) => {
            #[cfg(unix)]
            let bits = {
                use std::os::unix::fs::PermissionsExt;
                step.permissions.as_ref().map(|p| p.mode() & 0o777)
            };
            #[cfg(not(unix))]
            let bits = None;
            replay::write_path_with_mode(repo, &step.path, mode, oid, bits)?;
        }
        None => replay::remove(&step.path)?,
    }
    if !step.path.is_symlink()
        && let Some(permissions) = &step.permissions
    {
        std::fs::set_permissions(&step.path, permissions.clone())?;
    }
    Ok(())
}

/// Why a step must wait: unsaved local edits, git changes in a user
/// checkout, an invalid incoming configuration file.
fn hold_reason(
    repo: &crate::system::history::shadow::HistoryRepo,
    tracked: &TrackedSet,
    sync_state: &state::SyncState,
    step: &Step,
    take_remote: bool,
) -> Result<Option<String>> {
    let expected = step.pending.local.clone();
    if saved_object(repo, tracked, &step.path)? != expected {
        return Ok(Some(
            "local saved version changed since planning; run sync again".into(),
        ));
    }
    if !take_remote && live_object(repo, &step.path)? != expected {
        return Ok(Some(
            "local file changed since planning; save or resolve it first".into(),
        ));
    }
    // an incoming configuration file must at least parse
    if step.pending.configuration
        && step.path.extension().is_some_and(|ext| ext == "toml")
        && let Some((_, oid)) = &step.pending.object
    {
        let bytes = repo.cat_object(oid)?;
        if let Err(err) = toml::from_str::<toml::Value>(&String::from_utf8_lossy(&bytes)) {
            return Ok(Some(format!("invalid merge: {err}")));
        }
    }
    if !step.exists {
        return Ok(None);
    }
    // a directory is never replaced by a file from the repository
    if step.path.is_dir() && !step.path.is_symlink() {
        return Ok(Some(
            "needs decision: a directory stands where the repository has a file; move it away first"
                .into(),
        ));
    }
    // inside a user checkout: staged changes always hold; an unstaged
    // difference holds unless it is mise's own previous application
    let applied = sync_state
        .get(&step.pending.branch_path)
        .and_then(|record| record.applied.clone());
    // an untracked file is not the checkout's: the ordinary check applies
    if let Some(status) = git_status(&step.path)?
        && status != "??"
        && status != "!!"
    {
        let staged = status.chars().next().is_some_and(|c| c != ' ' && c != '?');
        if staged {
            return Ok(Some("needs decision: staged git changes".into()));
        }
        let unstaged = status.chars().nth(1).is_some_and(|c| c != ' ');
        if !take_remote && unstaged && live_object(repo, &step.path)? != applied {
            return Ok(Some("needs decision: local git changes".into()));
        }
        return Ok(None);
    }
    if take_remote {
        return Ok(None);
    }
    // a genuine local edit: the live file is neither what mise last wrote
    // nor the saved version
    let live = live_object(repo, &step.path)?;
    if live == applied {
        return Ok(None);
    }
    let saved = saved_object(repo, tracked, &step.path)?;
    if live == saved {
        return Ok(None);
    }
    Ok(Some(
        if tracked
            .entry_for(&step.path)
            .is_some_and(|entry| !entry.policy.autosave)
        {
            "unsaved edits: `mise bootstrap dotfiles save` or discard them first".into()
        } else {
            "local edit not saved yet; save it (or wait for the watcher) and apply again".into()
        },
    ))
}

pub(super) fn live_object(
    repo: &crate::system::history::shadow::HistoryRepo,
    path: &Path,
) -> Result<Option<Object>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    if meta.file_type().is_symlink() {
        return Ok(Some((
            "120000".into(),
            repo.hash_blob(std::fs::read_link(path)?.to_string_lossy().as_bytes())?,
        )));
    }
    if !meta.is_file() {
        bail!("{} is not a regular file or symlink", display_path(path));
    }
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    Ok(Some((
        if executable { "100755" } else { "100644" }.into(),
        repo.hash_blob(&std::fs::read(path)?)?,
    )))
}

fn saved_object(
    repo: &crate::system::history::shadow::HistoryRepo,
    _tracked: &TrackedSet,
    path: &Path,
) -> Result<Option<Object>> {
    let store = Store::open()?;
    let Some(latest) = store.list()?.into_iter().last() else {
        return Ok(None);
    };
    let Some(snapshot) = latest.checkpoint.tree.snapshot else {
        return Ok(None);
    };
    let tree_path = crate::system::history::tracked::display_to_tree_path(&display_path(path));
    repo.object_at(&snapshot, &tree_path)
}

/// The porcelain status of `path` in the user's checkout that contains
/// it, if any (`None` outside a checkout).
pub(super) fn git_status(path: &Path) -> Result<Option<String>> {
    let Some(root) = path
        .ancestors()
        .skip(1)
        .find(|dir| dir.join(".git").exists())
    else {
        return Ok(None);
    };
    let Some(git) = crate::git::plumbing_binary() else {
        return Ok(None);
    };
    let output = std::process::Command::new(git)
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=all", "--"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .next()
        .map(|line| line.chars().take(2).collect()))
}

/// The conflicts as rows for `mise bootstrap dotfiles status`.
pub(crate) fn describe_conflicts(conflicts: &[Conflict]) -> Vec<(String, String)> {
    let roots = Roots::current();
    conflicts
        .iter()
        .map(|conflict| {
            let path = roots
                .locate(&conflict.branch_path)
                .path()
                .map(display_path)
                .unwrap_or_else(|| conflict.branch_path.clone());
            (path, conflict.kind.describe().to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::history::shadow::HistoryRepo;

    #[test]
    fn recovery_restores_preimage_but_preserves_concurrent_edits() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let repo = HistoryRepo::open_or_init_in(dir.path())?.expect("git available");
        let path = dir.path().join("config");
        std::fs::write(&path, "before")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        let step = Step {
            before: live_object(&repo, &path)?,
            permissions: Some(std::fs::metadata(&path)?.permissions()),
            path: path.clone(),
            group: "setup".into(),
            exists: true,
            pending: PendingApplication {
                branch_path: "home/config".into(),
                object: Some(("100644".into(), repo.hash_blob(b"incoming")?)),
                configuration: false,
                next: Default::default(),
                local: None,
            },
        };
        std::fs::write(&path, "incoming")?;
        recover_step(&repo, &step)?;
        assert_eq!(std::fs::read_to_string(&path)?, "before");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path)?.permissions().mode() & 0o777,
                0o600
            );
        }
        // Recovery is idempotent after an earlier successful attempt.
        recover_step(&repo, &step)?;
        std::fs::write(&path, "concurrent edit")?;
        assert!(recover_step(&repo, &step).is_err());
        assert_eq!(std::fs::read_to_string(&path)?, "concurrent edit");
        Ok(())
    }
}
