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
    before_mode: Option<u32>,
    desired_mode: Option<u32>,
}

pub(crate) async fn apply(
    store: &Store,
    tracked: &TrackedSet,
    req: &ApplyRequest,
) -> Result<ApplyOutcome> {
    if !req.paths.is_empty() {
        bail!("partial pulls are not supported: apply the complete setup without PATH arguments");
    }
    let _sync_lock = run::lock(store)?;
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("applying requires git"))?;
    let state_dir = store.state_dir();
    let planned_head = repo.ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?;
    let incoming = run::incoming_tracking(repo, tracked)?;
    let tracked = &incoming;
    // Metadata changes must advance the same complete Git tree after live
    // application. They cannot be reduced to this machine's selected paths.
    let mut inventory_tree = run::incoming_repository_tree(repo, tracked)?;
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
    let shared = super::share::current(repo, tracked)?.objects();
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
            let saved_mode = permissions_at(
                repo,
                planned_head.as_deref(),
                &conflict.branch_path,
                saved.as_ref(),
            )?;
            if keep_local.contains(&local)
                && (live != saved || live_permissions(&local)? != saved_mode)
            {
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
                    local_mode: saved_mode,
                    remote_mode: permissions_at(
                        repo,
                        status.upstream_commit.as_deref(),
                        &conflict.branch_path,
                        remote.as_ref(),
                    )?,
                    live_mode: live_permissions(&local)?,
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

    // Only after all version-bound choices are valid may they replace Git's
    // conflict objects in the complete tree. Use the committed ciphertext,
    // never the decrypted comparison objects held by the resolution record.
    if let Some(tree) = &inventory_tree {
        let mut overlays = vec![];
        for (path, choice) in &status.resolutions {
            let head = if choice.take_remote {
                status.upstream_commit.as_deref()
            } else {
                planned_head.as_deref()
            };
            overlays.push(crate::system::history::shadow::Overlay {
                path: path.clone(),
                object: head
                    .map(|head| repo.object_at(head, path))
                    .transpose()?
                    .flatten(),
            });
        }
        inventory_tree = Some(repo.compose(tree, &overlays)?);
    }

    // the write set
    let directory_tree = inventory_tree
        .as_deref()
        .or(status.upstream_commit.as_deref())
        .or(planned_head.as_deref());
    let mut directories = directory_tree
        .map(|tree| super::directories::plan(repo, tracked, tree))
        .transpose()?
        .unwrap_or_default();
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
        let group = if pending.configuration || pending.branch_path.starts_with("sources/") {
            "configuration".to_string()
        } else {
            pending.branch_path.clone()
        };
        steps.push(Step {
            before_mode: live_permissions(&path)?,
            desired_mode: tracked
                .manifest
                .file_permissions(&pending.branch_path, pending.object.as_ref()),
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
    let fresh_adoption = planned_head.is_none() && status.upstream_commit.is_some();
    if steps.is_empty() && !fresh_adoption && inventory_tree.is_none() {
        if !req.dry_run && !req.automatic {
            status.application_failure = None;
            run::write_status(state_dir, &status)?;
        }
        if !req.automatic {
            info!("history: nothing to apply");
        }
        return Ok(ApplyOutcome::default());
    }

    let staged = staged_paths(steps.iter().map(|step| step.path.as_path()))?;
    // Validate the complete batch before writing any member.
    for step in &steps {
        let take_remote = status
            .resolutions
            .get(&step.pending.branch_path)
            .is_some_and(|choice| choice.take_remote);
        if let Some(reason) = hold_reason(repo, tracked, step, take_remote, &staged)? {
            holds.push(Hold {
                path: step.path.clone(),
                reason,
            });
        }
    }
    // One unsafe path holds the complete batch; there is no partial apply.
    let (ready, held): (Vec<&Step>, Vec<&Step>) = if holds.is_empty() {
        (steps.iter().collect(), vec![])
    } else {
        (vec![], steps.iter().collect())
    };
    if held.is_empty()
        && let Some(tree) = &inventory_tree
    {
        let parents: Vec<_> = planned_head
            .as_deref()
            .into_iter()
            .chain(status.upstream_commit.as_deref())
            .collect();
        let overlays = ready
            .iter()
            .map(|step| {
                Ok(crate::system::history::shadow::Overlay {
                    path: step.pending.branch_path.clone(),
                    object: step
                        .pending
                        .object
                        .as_ref()
                        .map(|object| {
                            super::files::commit_object(
                                repo,
                                &step.pending.branch_path,
                                object,
                                &tracked.manifest,
                                &parents,
                                !req.automatic && console::user_attended_stderr(),
                            )
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        inventory_tree = Some(repo.compose(tree, &overlays)?);
    }

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
    for directory in &directories {
        table.add_row(vec![
            display_path(&directory.path),
            "permissions".into(),
            "setup".into(),
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
    if ready.is_empty() && !((fresh_adoption || inventory_tree.is_some()) && held.is_empty()) {
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
    let application_head =
        repo.ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?;
    let mut touched = vec![];
    let result = (|| -> Result<()> {
        scope.validate_starting_head(planned_head.as_deref())?;
        if let (Some(tree), Some(local)) = (&inventory_tree, &planned_head) {
            let heads = super::graph::Heads::read(repo)?;
            let current = heads
                .local
                .as_deref()
                .ok_or_else(|| eyre::eyre!("local history disappeared"))?;
            // The operation's own protective commit may save the unsaved
            // versions that an explicit --take-remote decision will replace.
            // Permit only changes at those preflighted paths; any other saved
            // change requires a fresh complete plan.
            let restore_planned = ready
                .iter()
                .map(|step| {
                    Ok(crate::system::history::shadow::Overlay {
                        path: step.pending.branch_path.clone(),
                        object: repo.object_at(local, &step.pending.branch_path)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            if repo.compose(&repo.output_tree_of(current)?, &restore_planned)?
                != repo.output_tree_of(local)?
            {
                bail!("local files were saved after enrollment planning; retry pull");
            }
            // Do not let a divergent text merge or a stale resolution differ
            // from the complete tree that will be adopted.
            for step in &ready {
                if repo.restored_object_at(tree, &step.pending.branch_path)? != step.pending.object
                {
                    bail!(
                        "{} changed in the complete repository merge; reconcile again",
                        display_path(&step.path)
                    );
                }
            }
            let candidate = heads
                .candidate(repo, tree)?
                .ok_or_else(|| eyre::eyre!("setup branch disappeared"))?;
            super::files::audit_history(repo, &candidate.commit, &Default::default())?;
        }
        // Validate the complete batch again after acquiring the operation
        // lock, before the first write.
        for directory in &directories {
            directory.validate()?;
        }
        for step in &ready {
            if live_object(repo, &step.path)? != step.before
                || live_permissions(&step.path)? != step.before_mode
            {
                bail!(
                    "{} changed before application; nothing was written",
                    display_path(&step.path)
                );
            }
        }
        for directory in &mut directories {
            directory.apply()?;
        }
        for step in &ready {
            // the file may have changed since the plan was made: an edit
            // that landed meanwhile is never overwritten (undo would bring
            // back the planned version, not it)
            if live_object(repo, &step.path)? != step.before
                || live_permissions(&step.path)? != step.before_mode
            {
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
                Some((mode, oid)) => {
                    replay::write_path_with_mode(repo, &step.path, mode, oid, step.desired_mode)?
                }
                None => replay::remove(&step.path)?,
            }
            journal::commit_changes(pending);
            let mut next = step.pending.next.clone();
            let oid = step.pending.object.clone();
            next.applied = oid.clone();
            next.acknowledged = oid;
            sync_state.insert(step.pending.branch_path.clone(), next);
        }
        if fresh_adoption || inventory_tree.is_some() {
            // Verify the whole written batch again before advancing Git. A
            // concurrent edit must not become an apparently applied setup.
            for step in &ready {
                if live_object(repo, &step.path)? != step.pending.object
                    || live_permissions(&step.path)? != step.desired_mode
                {
                    bail!(
                        "{} changed during setup adoption; retry pull",
                        display_path(&step.path)
                    );
                }
            }
            for directory in &directories {
                directory.verify_written()?;
            }
            let heads = super::graph::Heads::read(repo)?;
            if heads.local != application_head || heads.remote != status.upstream_commit {
                bail!("setup history changed during adoption; retry pull");
            }
            let remote = heads.remote.as_deref().unwrap();
            super::files::audit_history(repo, remote, &Default::default())?;
            let tree = inventory_tree
                .clone()
                .unwrap_or(repo.output_tree_of(remote)?);
            let candidate = heads
                .candidate(repo, &tree)?
                .ok_or_else(|| eyre::eyre!("setup branch disappeared during adoption"))?;
            candidate.adopt(repo)?;
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
        for directory in directories.iter().rev() {
            if let Err(err) = directory.recover() {
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
        let summary = Some(Summary {
            message: Some("apply failed; recovery attempted".into()),
        });
        if recovery_errors.is_empty() {
            scope.finish(status.application_failure.clone(), summary);
        } else {
            scope.finish_incomplete(status.application_failure.clone(), summary);
        }
        return result.map(|()| ApplyOutcome::default());
    }
    let written = touched.len() + directories.len();
    scope.promote(&touched);
    let summary = Summary {
        message: Some(format!("applied {written} incoming change(s)")),
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
    status.pending_repository = false;
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
    if !touched.is_empty() || inventory_tree.is_some() || fresh_adoption {
        status.last_apply = Some(crate::system::history::store::now_rfc3339());
    }
    run::write_status(state_dir, &status)?;
    scope.finish(None, Some(summary));
    replay::run_reload(&reload, &touched);
    let configuration = ready.iter().any(|step| step.pending.configuration);
    if configuration && !req.automatic {
        info!(
            "history: configuration changed; declarations may differ from the applied setup: run `mise bootstrap --dry-run`"
        );
    }
    if !req.automatic {
        info!("history: applied {written} incoming change(s)");
    }
    let outcome = ApplyOutcome {
        written,
        held: held.len(),
        configuration,
    };
    Ok(outcome)
}

fn recover_step(repo: &crate::system::history::shadow::HistoryRepo, step: &Step) -> Result<()> {
    let current = live_object(repo, &step.path)?;
    if current == step.before && live_permissions(&step.path)? == step.before_mode {
        return Ok(());
    }
    if current != step.pending.object || live_permissions(&step.path)? != step.desired_mode {
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
    step: &Step,
    take_remote: bool,
    staged: &BTreeSet<PathBuf>,
) -> Result<Option<String>> {
    let expected = step.pending.local.clone();
    if saved_object(repo, tracked, &step.path)? != expected {
        return Ok(Some(
            "local saved version changed since planning; run sync again".into(),
        ));
    }
    let live = live_object(repo, &step.path)?;
    let identical_adoption = repo
        .ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?
        .is_none()
        && live == step.pending.object;
    let local_head = repo.ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)?;
    let expected_mode = permissions_at(
        repo,
        local_head.as_deref(),
        &step.pending.branch_path,
        expected.as_ref(),
    )?;
    if !take_remote
        && (live != expected || live_permissions(&step.path)? != expected_mode)
        && !identical_adoption
    {
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
    // Saved/live comparison above is authoritative; preserve staged changes
    // independently, without consulting a machine's old application cache.
    if has_staged_changes(staged, &step.path) {
        return Ok(Some("needs decision: staged git changes".into()));
    }
    Ok(None)
}

fn permissions_at(
    repo: &crate::system::history::shadow::HistoryRepo,
    commit: Option<&str>,
    path: &str,
    object: Option<&Object>,
) -> Result<Option<u32>> {
    Ok(commit
        .map(|head| crate::system::history::manifest::Manifest::read(repo, head))
        .transpose()?
        .flatten()
        .unwrap_or_default()
        .file_permissions(path, object))
}

pub(super) fn live_permissions(path: &Path) -> Result<Option<u32>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_file() => Ok(Some(meta.permissions().mode() & 0o777)),
            Ok(_) => Ok(None),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(None)
    }
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
            repo.transient_blob_id(std::fs::read_link(path)?.to_string_lossy().as_bytes())?,
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
        repo.transient_blob_id(&std::fs::read(path)?)?,
    )))
}

fn saved_object(
    repo: &crate::system::history::shadow::HistoryRepo,
    tracked: &TrackedSet,
    path: &Path,
) -> Result<Option<Object>> {
    let Some(head) = repo.ref_oid(crate::system::history::shadow::HistoryRepo::HISTORY_REF)? else {
        return Ok(None);
    };
    let Some(entry) = tracked.entry_for(path) else {
        return Ok(None);
    };
    repo.restored_object_at(&head, &entry.tree_path(path)?)
}

/// Read each containing checkout's index once per preflight, not once per
/// incoming file. Never retain this observation across subsequent validation.
pub(super) fn staged_paths<'a>(paths: impl Iterator<Item = &'a Path>) -> Result<BTreeSet<PathBuf>> {
    let roots: BTreeSet<_> = paths
        .filter_map(|path| {
            path.ancestors()
                .find(|dir| dir.join(".git").exists())
                .map(Path::to_path_buf)
        })
        .collect();
    let mut staged = BTreeSet::new();
    if roots.is_empty() {
        return Ok(staged);
    }
    let Some(git) = crate::git::plumbing_binary() else {
        bail!("cannot check staged changes: git is unavailable");
    };
    for root in roots {
        let mut command = std::process::Command::new(git);
        crate::git::sanitize_git_command(&mut command);
        let output = command
            .arg("-C")
            .arg(&root)
            .args([
                "-c",
                "core.fsmonitor=false",
                "diff",
                "--cached",
                "--name-only",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--",
            ])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(std::process::Stdio::null())
            .output()?;
        if !output.status.success() {
            bail!(
                "cannot check staged changes in {}: {}",
                display_path(&root),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        for name in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
        {
            #[cfg(unix)]
            let relative = {
                use std::os::unix::ffi::OsStrExt;
                PathBuf::from(std::ffi::OsStr::from_bytes(name))
            };
            #[cfg(not(unix))]
            let relative = PathBuf::from(std::str::from_utf8(name)?);
            staged.insert(normalize_target(&root.join(relative)));
        }
    }
    Ok(staged)
}

pub(super) fn has_staged_changes(staged: &BTreeSet<PathBuf>, path: &Path) -> bool {
    staged
        .range(path.to_path_buf()..)
        .next()
        .is_some_and(|entry| entry.starts_with(path))
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

pub(crate) fn resolution_advice(path: &str, reason: &str) -> String {
    if reason == super::reconcile::ConflictKind::Repository.describe() {
        "inspect the validation error, reconcile the repository with Git, then run `mise bootstrap dotfiles sync`".into()
    } else {
        format!("mise bootstrap dotfiles pull --take-remote|--keep-local {path}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn staged_lookup_batches_checkouts_and_preserves_filename_boundaries() -> eyre::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = crate::system::history::tracked::normalize_target(temp.path());
        let git = crate::git::plumbing_binary().expect("git is required for history tests");
        let run = |args: &[&str]| -> eyre::Result<()> {
            let mut command = std::process::Command::new(git);
            crate::git::sanitize_git_command(&mut command);
            let output = command.arg("-C").arg(&root).args(args).output()?;
            eyre::ensure!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            Ok(())
        };
        run(&["init", "--quiet"])?;
        std::fs::create_dir(root.join("configs"))?;
        let staged_file = root.join("configs/with spaces");
        let untracked = root.join("configs/untracked");
        std::fs::write(&staged_file, "staged")?;
        std::fs::write(&untracked, "not staged")?;
        run(&["add", "--", "configs/with spaces"])?;
        #[cfg(unix)]
        {
            std::fs::write(root.join("configs/with\nnewline"), "staged")?;
            run(&["add", "--", "configs/with\nnewline"])?;
        }
        let paths = [&staged_file, &untracked];
        let staged = super::staged_paths(paths.into_iter().map(|path| path.as_path()))?;
        assert!(super::has_staged_changes(&staged, &staged_file));
        assert!(super::has_staged_changes(&staged, &root.join("configs")));
        assert!(!super::has_staged_changes(&staged, &untracked));
        #[cfg(unix)]
        assert!(staged.contains(&root.join("configs/with\nnewline")));
        // A fresh validation must observe changes to the index.
        run(&["rm", "--cached", "--", "configs/with spaces"])?;
        let staged = super::staged_paths(std::iter::once(staged_file.as_path()))?;
        assert!(!super::has_staged_changes(&staged, &staged_file));
        Ok(())
    }

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
            before_mode: live_permissions(&path)?,
            desired_mode: cfg!(unix).then_some(0o644),
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
        let (mode, oid) = step.pending.object.as_ref().unwrap();
        replay::write_path_with_mode(&repo, &path, mode, oid, step.desired_mode)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400))?;
            assert!(recover_step(&repo, &step).is_err());
            assert_eq!(std::fs::read_to_string(&path)?, "incoming");
            assert_eq!(live_permissions(&path)?, Some(0o400));
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
        }
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
