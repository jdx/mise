//! Explicit application of incoming changes: `mise bootstrap dotfiles pull`. Related
//! files apply as groups (configuration together with the sources and
//! task files it references); an incoming configuration file is validated
//! before it is written; a path with unsaved local edits, staged git
//! changes, or a genuine local edit is held for a decision. Application
//! is the same recoverable transaction as a rollback: the write set is
//! planned, its preimages captured in a protective checkpoint, every file
//! written one at a time and journaled, and reload hooks run only once
//! everything succeeded.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eyre::{Result, bail};

use super::layout::{Roots, is_configuration};
use super::reconcile::Conflict;
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

pub(crate) struct ApplyRequest {
    /// Only these local paths (empty: everything pending).
    pub paths: Vec<PathBuf>,
    pub dry_run: bool,
    pub yes: bool,
    /// Resolve these conflicts with the upstream version.
    pub take_remote: Vec<PathBuf>,
    /// Resolve these conflicts by publishing the local version next.
    pub keep_local: Vec<PathBuf>,
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
    /// The live object when the plan was made, verified again right
    /// before the write.
    live: Option<String>,
}

pub(crate) async fn apply(store: &Store, tracked: &TrackedSet, req: &ApplyRequest) -> Result<()> {
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("applying requires git"))?;
    let state_dir = store.state_dir();
    let mut status = run::read_status(state_dir);
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

    // conflict decisions
    let mut decided = vec![];
    let mut decided_conflicts = vec![];
    let mut sync_state = state::load(repo)?;
    let mut state_changed = false;
    let mut remaining_conflicts = vec![];
    for conflict in &status.conflicts {
        let Some(local) = roots
            .locate(&conflict.branch_path)
            .path()
            .map(Path::to_path_buf)
        else {
            remaining_conflicts.push(conflict.clone());
            continue;
        };
        if take_remote.contains(&local) {
            decided_conflicts.push(conflict.clone());
            decided.push(PendingApplication {
                branch_path: conflict.branch_path.clone(),
                object: conflict
                    .remote
                    .clone()
                    .map(|oid| ("100644".to_string(), oid)),
                configuration: is_configuration(&conflict.branch_path),
                next: state::SyncRecord {
                    acknowledged: conflict.remote.clone(),
                    reconciled: conflict.remote.clone(),
                    applied: conflict.remote.clone(),
                    upstream_commit: status.upstream_commit.clone(),
                },
            });
        } else if keep_local.contains(&local) {
            // the local version is published by the next sync: upstream is
            // reconciled at its current version, local stays unacknowledged
            let record = sync_state.entry(conflict.branch_path.clone()).or_default();
            record.reconciled = conflict.remote.clone();
            record.upstream_commit = status.upstream_commit.clone();
            state_changed = true;
            info!(
                "history: keeping the local version of {}; the next `mise bootstrap dotfiles sync` publishes it",
                display_path(&local)
            );
        } else {
            remaining_conflicts.push(conflict.clone());
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
    if state_changed && !req.dry_run {
        state::save(repo, &sync_state, "kept local versions")?;
        status.conflicts = remaining_conflicts.clone();
        run::write_status(state_dir, &status)?;
    }

    // the write set
    let mut steps = vec![];
    let mut holds: Vec<Hold> = vec![];
    for pending in status.pending_applications.iter().chain(decided.iter()) {
        let Some(path) = roots
            .locate(&pending.branch_path)
            .path()
            .map(Path::to_path_buf)
        else {
            continue;
        };
        if !filter.is_empty() && !filter.iter().any(|f| path.starts_with(f)) {
            continue;
        }
        let group = if pending.configuration || pending.branch_path.starts_with("sources/") {
            "configuration".to_string()
        } else {
            pending.branch_path.clone()
        };
        steps.push(Step {
            pending: pending.clone(),
            exists: path.exists() || path.is_symlink(),
            live: live_oid(repo, &path)?,
            path,
            group,
        });
    }
    if steps.is_empty() {
        info!("history: nothing to apply");
        return Ok(());
    }

    // validation and holds, per group
    let mut held_groups: BTreeSet<String> = BTreeSet::new();
    for step in &steps {
        if let Some(reason) = hold_reason(repo, tracked, &sync_state, step)? {
            holds.push(Hold {
                path: step.path.clone(),
                reason,
            });
            held_groups.insert(step.group.clone());
        }
    }
    let (ready, held): (Vec<&Step>, Vec<&Step>) = steps
        .iter()
        .partition(|step| !held_groups.contains(&step.group));

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
            .unwrap_or_else(|| "held with its group".to_string());
        table.add_row(vec![
            display_path(&step.path),
            format!("held: {reason}"),
            step.group.clone(),
        ]);
    }
    table.print()?;
    if req.dry_run {
        miseprintln!("history: dry run; nothing was changed");
        return Ok(());
    }
    if ready.is_empty() {
        bail!("nothing can be applied until the held paths are decided");
    }
    if !super::origin::confirmed(req.yes, "history: apply these incoming changes?")? {
        info!("history: skipped");
        return Ok(());
    }

    // the transaction
    let reload = crate::system::history::config::reload_commands()?;
    let scope = OperationScope::begin_kind(OperationKind::Apply, "dotfiles pull", false).await?;
    scope.with_operation(|op| {
        op.applied = status.upstream_commit.clone();
    });
    let mut touched = vec![];
    let result = (|| -> Result<()> {
        for step in &ready {
            // the file may have changed since the plan was made: an edit
            // that landed meanwhile is never overwritten (undo would bring
            // back the planned version, not it)
            if live_oid(repo, &step.path)? != step.live {
                bail!(
                    "{} changed while the changes were being applied; nothing more was written. Run `mise bootstrap dotfiles pull` again",
                    display_path(&step.path)
                );
            }
            let pending =
                journal::begin_changes("history", &display_path(&step.path), [step.path.clone()])?;
            match &step.pending.object {
                Some((mode, oid)) => replay::write_path(repo, &step.path, mode, oid)?,
                None => replay::remove(&step.path)?,
            }
            journal::commit_changes(pending);
            touched.push(step.path.clone());
            let affected = display_path(&step.path);
            scope.with_operation(|op| op.affected.push(affected.clone()));
            let mut next = step.pending.next.clone();
            let oid = step.pending.object.as_ref().map(|(_, oid)| oid.clone());
            next.applied = oid.clone();
            next.acknowledged = oid;
            sync_state.insert(step.pending.branch_path.clone(), next);
        }
        Ok(())
    })();
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
    // a decided conflict whose repository version was not written after all
    // (filtered out, held with its group, a failed step) is still a conflict
    for conflict in decided_conflicts {
        if !applied.contains(&conflict.branch_path)
            && !remaining_conflicts
                .iter()
                .any(|remaining| remaining.branch_path == conflict.branch_path)
        {
            remaining_conflicts.push(conflict);
        }
    }
    status.conflicts = remaining_conflicts;
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
    if ready.iter().any(|step| step.pending.configuration) {
        info!(
            "history: configuration changed; declarations may differ from the applied setup: run `mise bootstrap --dry-run`"
        );
    }
    info!("history: applied {} incoming change(s)", touched.len());
    Ok(())
}

/// Why a step must wait: unsaved local edits, git changes in a user
/// checkout, an invalid incoming configuration file.
fn hold_reason(
    repo: &crate::system::history::shadow::HistoryRepo,
    tracked: &TrackedSet,
    sync_state: &state::SyncState,
    step: &Step,
) -> Result<Option<String>> {
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
        if unstaged && live_oid(repo, &step.path)? != applied {
            return Ok(Some("needs decision: local git changes".into()));
        }
        return Ok(None);
    }
    // a genuine local edit: the live file is neither what mise last wrote
    // nor the saved version
    let live = live_oid(repo, &step.path)?;
    if live == applied {
        return Ok(None);
    }
    let saved = saved_oid(repo, tracked, &step.path)?;
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

fn live_oid(
    repo: &crate::system::history::shadow::HistoryRepo,
    path: &Path,
) -> Result<Option<String>> {
    Ok(match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Some(repo.hash_blob(std::fs::read_link(path)?.to_string_lossy().as_bytes())?)
        }
        Ok(meta) if meta.is_file() => Some(repo.hash_blob(&std::fs::read(path)?)?),
        _ => None,
    })
}

fn saved_oid(
    repo: &crate::system::history::shadow::HistoryRepo,
    _tracked: &TrackedSet,
    path: &Path,
) -> Result<Option<String>> {
    let store = Store::open()?;
    let Some(latest) = store.list()?.into_iter().last() else {
        return Ok(None);
    };
    let Some(snapshot) = latest.checkpoint.tree.snapshot else {
        return Ok(None);
    };
    let tree_path = crate::system::history::tracked::display_to_tree_path(&display_path(path));
    Ok(repo.object_at(&snapshot, &tree_path)?.map(|(_, oid)| oid))
}

/// The porcelain status of `path` in the user's checkout that contains
/// it, if any (`None` outside a checkout).
fn git_status(path: &Path) -> Result<Option<String>> {
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
