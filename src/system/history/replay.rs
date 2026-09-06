//! Selective rollback and undo: returning tracked paths to the version a
//! checkpoint holds, with the previous state saved first.
//!
//! One engine serves both spellings. Path-first `rollback <path>…` picks,
//! per path, the newest checkpoint whose captured content differs from the
//! working tree (other files' checkpoints are never candidates);
//! checkpoint-first `rollback --to <ref> --all` selects everything that
//! checkpoint covers. `undo` restores exactly the paths an earlier
//! operation touched from the protective checkpoint it took, never a whole
//! snapshot, so unrelated work done since is preserved.
//!
//! Application is a recoverable transaction, not a filesystem-atomic one:
//! the complete write set is planned and its preimages captured first, the
//! pending record names the operation before anything is written, files are
//! written one at a time, and reload hooks run only after everything
//! succeeded.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use indexmap::IndexMap;

use super::checkpoint::Store;
use super::journal;
use super::scope::OperationScope;
use super::shadow::HistoryRepo;
use super::store::{Checkpoint, Entry, OperationKind, OperationSource, OperationStatus, Summary};
use super::tracked::{
    TrackedSet, display_to_tree_path, normalize, normalize_target, tree_path_to_display,
};
use crate::file::{self, display_path};
use crate::ui::prompt;
use crate::ui::table::MiseTable;

/// How many times the protective capture is retaken when files keep
/// changing while the plan is verified.
const VERIFY_ROUNDS: usize = 3;

pub(crate) struct RollbackRequest {
    /// Paths to roll back; empty with `all` = everything the checkpoint covers.
    pub paths: Vec<PathBuf>,
    /// The checkpoint to roll back to; per path, the newest differing one
    /// when absent.
    pub to: Option<String>,
    pub all: bool,
    pub dry_run: bool,
    pub yes: bool,
    pub force: bool,
}

pub(crate) struct UndoRequest {
    /// The operation to undo; the newest not yet undone when absent.
    pub reference: Option<String>,
    pub dry_run: bool,
    pub yes: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Write { mode: String, oid: String },
    Delete,
    Unchanged,
    Skip(String),
    Conflict(String),
}

impl Action {
    fn label(&self) -> String {
        match self {
            Self::Write { .. } => "write".into(),
            Self::Delete => "delete".into(),
            Self::Unchanged => "unchanged".into(),
            Self::Skip(reason) => format!("skip: {reason}"),
            Self::Conflict(reason) => format!("conflict: {reason}"),
        }
    }

    fn mutates(&self) -> bool {
        matches!(self, Self::Write { .. } | Self::Delete)
    }
}

#[derive(Clone, Debug)]
struct Step {
    path: PathBuf,
    tree_path: String,
    action: Action,
    from: String,
    to: String,
    /// The permission bits the checkpoint recorded for the file (only
    /// modes git cannot express, `0600` say), and for the directories
    /// above it.
    bits: Option<u32>,
    dir_bits: Vec<(PathBuf, u32)>,
}

/// One checkpoint and the paths to take from it.
struct Target {
    entry: Entry,
    paths: Vec<PathBuf>,
}

pub(crate) async fn rollback(req: RollbackRequest) -> Result<()> {
    ensure_enabled()?;
    if req.paths.is_empty() && !(req.to.is_some() && req.all) {
        bail!(
            "name the paths to roll back, or `--to <ref> --all` for everything the checkpoint covers"
        );
    }
    if req.to.is_none() && req.all {
        bail!("`--all` needs `--to <ref>`");
    }
    let (store, tracked, entries) = crate::cli::dotfiles::history::open().await?;
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("rolling back requires git"))?;
    let live = live_tree(repo, &tracked)?;
    let paths: Vec<PathBuf> = req
        .paths
        .iter()
        .map(|path| normalize_target(path))
        .collect();
    for path in &paths {
        if tracked.entry_for(path).is_none() {
            bail!(
                "{} is not tracked; `mise bootstrap dotfiles paths` lists what is",
                display_path(path)
            );
        }
    }
    let targets = match &req.to {
        Some(reference) => {
            let entry = crate::cli::dotfiles::history::resolve(
                reference,
                &entries,
                (paths.len() == 1)
                    .then(|| display_path(&paths[0]))
                    .as_deref(),
            )?;
            refuse_unusable(&entry)?;
            let paths = if req.all {
                covered_paths(&entry.checkpoint)
            } else {
                paths.clone()
            };
            vec![Target { entry, paths }]
        }
        None => {
            let mut by_id: BTreeMap<u64, Target> = BTreeMap::new();
            for path in &paths {
                let Some(entry) = newest_differing(repo, &entries, &live, path)? else {
                    info!(
                        "history: {} has no saved version that differs from the working tree",
                        display_path(path)
                    );
                    continue;
                };
                by_id
                    .entry(entry.id)
                    .or_insert_with(|| Target {
                        entry: entry.clone(),
                        paths: vec![],
                    })
                    .paths
                    .push(path.clone());
            }
            by_id.into_values().collect()
        }
    };
    if targets.is_empty() {
        info!("history: nothing to roll back");
        return Ok(());
    }
    let label = targets
        .iter()
        .map(|target| format!("checkpoint {}", target.entry.id))
        .collect::<Vec<_>>()
        .join(", ");
    let paths_label = targets
        .iter()
        .flat_map(|target| target.paths.iter().map(display_path))
        .collect::<Vec<_>>()
        .join(", ");
    let message = format!("rolled back {paths_label} to {label}");
    let command = format!(
        "bootstrap dotfiles rollback {}",
        shell_words::join(req.paths.iter().map(display_path))
    );
    let to = targets
        .first()
        .map(|target| target.entry.checkpoint.uuid.clone());
    let sources = sources_of(&targets);
    execute(
        Execution {
            kind: OperationKind::Rollback,
            command,
            targets,
            message,
            to,
            sources,
            undoes: None,
            restore_dirs: BTreeSet::new(),
            dry_run: req.dry_run,
            yes: req.yes,
            force: req.force,
        },
        &store,
        &tracked,
        &entries,
        live,
    )
    .await
}

pub(crate) async fn undo(req: UndoRequest) -> Result<()> {
    ensure_enabled()?;
    let (store, tracked, entries) = crate::cli::dotfiles::history::open().await?;
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("undoing requires git"))?;
    let live = live_tree(repo, &tracked)?;
    // an operation counts as undone only when an undo actually changed
    // something for it: a declined or failed undo that touched nothing
    // leaves the original next in line
    // an operation counts as undone while an undo that changed something
    // points at it; undoing that undo puts its target back in line
    let mut undone: BTreeSet<String> = BTreeSet::new();
    let mut undoes_of: BTreeMap<String, String> = BTreeMap::new();
    for entry in &entries {
        let Some(op) = entry.checkpoint.operation.as_ref() else {
            continue;
        };
        let Some(target) = op.undoes.clone() else {
            continue;
        };
        if op.affected.is_empty() {
            continue;
        }
        undone.insert(target.clone());
        if let Some(reinstated) = undoes_of.get(&target) {
            undone.remove(reinstated);
        }
        undoes_of.insert(entry.checkpoint.uuid.clone(), target);
    }
    let operation = match &req.reference {
        Some(reference) => crate::cli::dotfiles::history::resolve(reference, &entries, None)?,
        None => entries
            .iter()
            .rev()
            .find(|entry| {
                entry.checkpoint.operation.as_ref().is_some_and(|op| {
                    matches!(op.kind, OperationKind::Rollback | OperationKind::Undo)
                        && op.status != OperationStatus::Pending
                        && op.before.is_some()
                        && !op.affected.is_empty()
                }) && !undone.contains(&entry.checkpoint.uuid)
            })
            .cloned()
            .ok_or_else(|| eyre::eyre!("nothing to undo: no rollback or undo is left"))?,
    };
    let Some(op) = operation.checkpoint.operation.clone() else {
        bail!("checkpoint {} is not an operation", operation.id);
    };
    // an operation records the paths it changed (`affected`) only when it
    // writes them itself; a bootstrap or a dotfiles apply journals its
    // changes instead and is not reversed here
    if !matches!(op.kind, OperationKind::Rollback | OperationKind::Undo) {
        bail!(
            "checkpoint {} is a {} operation; only rollback and undo can be undone",
            operation.id,
            op.kind.as_str()
        );
    }
    if op.status == OperationStatus::Pending {
        bail!(
            "checkpoint {} is still running or was interrupted; nothing to undo",
            operation.id
        );
    }
    if op.status == OperationStatus::Failed && !op.affected.is_empty() {
        info!(
            "history: operation {} failed midway; reversing the {} path(s) it changed",
            operation.id,
            op.affected.len()
        );
    }
    let Some(before_uuid) = &op.before else {
        bail!(
            "checkpoint {} has no protective checkpoint to undo from",
            operation.id
        );
    };
    let Some(before) = entries
        .iter()
        .find(|entry| &entry.checkpoint.uuid == before_uuid)
        .cloned()
    else {
        bail!(
            "checkpoint {} was pruned; the state before operation {} is gone",
            before_uuid,
            operation.id
        );
    };
    if op.affected.is_empty() {
        info!("history: operation {} touched nothing", operation.id);
        return Ok(());
    }
    let paths: Vec<PathBuf> = op
        .affected
        .iter()
        .map(|path| normalize_target(Path::new(path)))
        .collect();
    let message = format!(
        "undid {} {} ({})",
        op.kind.as_str(),
        operation.id,
        op.affected.join(", ")
    );
    let restore_dirs: BTreeSet<PathBuf> = op
        .directories
        .iter()
        .map(|path| normalize_target(Path::new(path)))
        .collect();
    let targets = vec![Target {
        entry: before.clone(),
        paths,
    }];
    let sources = sources_of(&targets);
    execute(
        Execution {
            kind: OperationKind::Undo,
            command: format!("bootstrap dotfiles undo {}", operation.id),
            targets,
            message,
            to: Some(before.checkpoint.uuid.clone()),
            sources,
            undoes: Some(operation.checkpoint.uuid.clone()),
            restore_dirs,
            dry_run: req.dry_run,
            yes: req.yes,
            // the protective checkpoint is authoritative: whatever the
            // operation replaced, including a type change it forced, is
            // restored exactly
            force: true,
        },
        &store,
        &tracked,
        &entries,
        live,
    )
    .await
}

/// Restoring needs the store, which `history.enabled = false` closes.
fn ensure_enabled() -> Result<()> {
    if !crate::config::Settings::get().history.enabled {
        bail!("history is disabled (history.enabled = false); nothing can be restored");
    }
    Ok(())
}

struct Execution {
    kind: OperationKind,
    command: String,
    targets: Vec<Target>,
    message: String,
    to: Option<String>,
    sources: Vec<OperationSource>,
    undoes: Option<String>,
    /// Directories to recreate after the plan is applied (an empty directory
    /// an operation replaced leaves no trace in a snapshot).
    restore_dirs: BTreeSet<PathBuf>,
    dry_run: bool,
    yes: bool,
    force: bool,
}

fn sources_of(targets: &[Target]) -> Vec<OperationSource> {
    targets
        .iter()
        .map(|target| OperationSource {
            checkpoint: target.entry.checkpoint.uuid.clone(),
            paths: target.paths.iter().map(display_path).collect(),
        })
        .collect()
}

async fn execute(
    exec: Execution,
    store: &Store,
    tracked: &TrackedSet,
    entries: &[Entry],
    live: String,
) -> Result<()> {
    let repo = store.repo().expect("checked by the caller");
    let mut steps = plan(repo, &exec, &live)?;
    print_plan(&steps, &exec, tracked)?;
    if exec.dry_run {
        return Ok(());
    }
    let conflicts: Vec<&Step> = steps
        .iter()
        .filter(|step| matches!(step.action, Action::Conflict(_)))
        .collect();
    if !conflicts.is_empty() {
        // `--force` answers a type change only (and undo already forces);
        // an occupant history never captured stays a conflict whatever is
        // passed, so it is not offered where it would not help
        let type_changes = conflicts.iter().any(
            |step| matches!(&step.action, Action::Conflict(reason) if reason.contains(" became ")),
        );
        let hint = if !exec.force && type_changes {
            "; pass --force to replace a path whose type changed"
        } else {
            ""
        };
        bail!(
            "{} path(s) conflict (see the plan above); resolve them first{hint}",
            conflicts.len()
        );
    }
    // a link to a directory is not the directory: it counts as work left
    let restores = exec
        .restore_dirs
        .iter()
        .filter(|dir| std::fs::symlink_metadata(dir).is_err())
        .count();
    if !steps.iter().any(|step| step.action.mutates()) && restores == 0 {
        info!("history: nothing to do");
        return Ok(());
    }
    if !exec.yes && !prompt::confirm("history: apply this plan?")?.is_yes() {
        info!("history: skipped");
        return Ok(());
    }
    // resolved from the trusted layers now, so nothing this operation writes
    // can change which commands run afterwards
    let reload = super::config::reload_commands()?;
    let scope = OperationScope::begin_kind(exec.kind, &exec.command, false).await?;
    scope.with_operation(|op| {
        op.to = exec.to.clone();
        op.sources = exec.sources.clone();
        op.undoes = exec.undoes.clone();
    });
    let result = apply_steps(&scope, repo, store, tracked, &exec, entries, &mut steps).await;
    let (error, summary) = match &result {
        Ok(touched) => {
            scope.promote(touched);
            (
                None,
                Summary {
                    message: Some(exec.message.clone()),
                },
            )
        }
        Err(err) => (
            Some(format!("{err:#}")),
            Summary {
                message: Some(format!("{} failed", exec.message)),
            },
        ),
    };
    scope.finish(error, Some(summary));
    let touched = result?;
    run_reload(&reload, &touched);
    config_hint(&touched);
    info!("history: {}", exec.message);
    Ok(())
}

/// Writes the plan after the protective checkpoint exists and the plan and
/// its preimages are verified against the working tree.
async fn apply_steps(
    scope: &OperationScope,
    repo: &HistoryRepo,
    store: &Store,
    tracked: &TrackedSet,
    exec: &Execution,
    entries: &[Entry],
    steps: &mut Vec<Step>,
) -> Result<Vec<PathBuf>> {
    let _ = (store, entries);
    let mut round = 0;
    let mut live;
    loop {
        round += 1;
        // editors may have written while the prompt was open: re-plan
        live = live_tree(repo, tracked)?;
        let fresh = plan(repo, exec, &live)?;
        let changed = actionable(&fresh) != actionable(steps);
        if changed {
            *steps = fresh.clone();
            miseprintln!("history: the working tree changed since the plan was shown:");
            print_plan(steps, exec, tracked)?;
            if steps
                .iter()
                .any(|step| matches!(step.action, Action::Conflict(_)))
            {
                bail!("the refreshed plan has conflicts; nothing was changed");
            }
            if !exec.yes && !prompt::confirm("history: apply the refreshed plan?")?.is_yes() {
                bail!("declined; nothing was changed");
            }
        }
        // completeness: every path about to be written or deleted that exists
        // now must be captured, as it is now, in the protective checkpoint
        let Some((before_id, _)) = scope.before() else {
            bail!("no protective checkpoint could be taken; nothing was changed");
        };
        let before = store::entry(store, before_id)?;
        let before_tree = before
            .checkpoint
            .tree
            .snapshot
            .clone()
            .ok_or_else(|| eyre::eyre!("the protective checkpoint has no content"))?;
        let mut missing = vec![];
        for step in steps.iter().filter(|step| step.action.mutates()) {
            let Some(current) = repo.object_at(&live, &step.tree_path)? else {
                continue;
            };
            if repo.object_at(&before_tree, &step.tree_path)? != Some(current) {
                missing.push(step.path.clone());
            }
        }
        if missing.is_empty() {
            break;
        }
        if round >= VERIFY_ROUNDS {
            bail!(
                "these paths keep changing while being protected; nothing was changed: {}",
                missing
                    .iter()
                    .map(display_path)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        warn!(
            "history: {} path(s) changed after the protective checkpoint; capturing again",
            missing.len()
        );
        scope.recapture_before()?;
    }
    let mut touched = vec![];
    // deletions deepest first, then writes shallowest first: a directory is
    // emptied before the file that replaces it is written, and a directory
    // that replaces a file exists before its files are written
    let mut ordered: Vec<&Step> = steps.iter().filter(|step| step.action.mutates()).collect();
    ordered.sort_by(|a, b| {
        let rank = |step: &Step| matches!(step.action, Action::Write { .. });
        rank(a).cmp(&rank(b)).then_with(|| {
            if rank(a) {
                a.path.cmp(&b.path)
            } else {
                b.path
                    .components()
                    .count()
                    .cmp(&a.path.components().count())
                    .then_with(|| b.path.cmp(&a.path))
            }
        })
    });
    for step in ordered {
        // a file created or changed since the verified sample was never
        // protected: stop here; what was already written is recorded below
        // and undo can reverse it
        let expected = repo.object_at(&live, &step.tree_path)?;
        if !same_object(&expected, &current_object(repo, &step.path)?) {
            bail!(
                "{} changed after it was protected; nothing more was changed",
                display_path(&step.path)
            );
        }
        let was_directory = step.path.is_dir() && !step.path.is_symlink();
        let mut empty_dirs = vec![];
        if was_directory && !matches!(&step.action, Action::Write { mode, .. } if mode == "040000")
        {
            // replacing or removing a directory removes everything inside
            // it: a file history would capture must be a step of this plan
            // (else it appeared after protection); a file history never
            // covers goes with it, said out loud; empty subdirectories
            // leave no trace in a snapshot and are recorded for undo
            let inside = directory_contents(&step.path, steps, tracked)?;
            if let Some(stray) = inside.appeared.first() {
                bail!(
                    "{} appeared after {} was protected; nothing more was changed",
                    display_path(stray),
                    display_path(&step.path)
                );
            }
            if !inside.uncovered.is_empty() {
                // only reachable with --force (a type change): say what
                // goes with the directory that no checkpoint holds
                warn!(
                    "history: {} contains files history does not cover; they are removed with it and cannot be undone: {}",
                    display_path(&step.path),
                    inside
                        .uncovered
                        .iter()
                        .map(display_path)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            empty_dirs = inside.empty_dirs;
        }
        // recorded before anything is touched: a write that fails halfway
        // (the old file removed, the new one not written) is still undone
        let affected = display_path(&step.path);
        scope.with_operation(|op| {
            op.affected.push(affected.clone());
            if was_directory {
                op.directories.push(affected.clone());
                op.directories.extend(empty_dirs.iter().map(display_path));
            }
        });
        let pending =
            journal::begin_changes("history", &display_path(&step.path), [step.path.clone()])?;
        match &step.action {
            Action::Write { mode, oid } => write_object(repo, step, mode, oid)?,
            Action::Delete => remove(&step.path)?,
            _ => unreachable!("filtered to mutating steps"),
        }
        journal::commit_changes(pending);
        touched.push(step.path.clone());
    }
    for dir in &exec.restore_dirs {
        if dir.is_dir() && !dir.is_symlink() {
            continue;
        }
        if dir.exists() || dir.is_symlink() {
            bail!(
                "{} cannot be recreated as a directory: something else is there now; nothing more was changed",
                display_path(dir)
            );
        }
        file::create_dir_all(dir)?;
        if !touched.contains(dir) {
            touched.push(dir.clone());
            let affected = display_path(dir);
            scope.with_operation(|op| op.affected.push(affected.clone()));
        }
    }
    Ok(touched)
}

/// What replacing or removing `dir` would take with it beyond the plan.
#[derive(Default)]
struct DirectoryContents {
    /// Files history would capture that no step accounts for: they
    /// appeared after protection.
    appeared: Vec<PathBuf>,
    /// Files history never covers (excluded, omitted, special, `.git`).
    uncovered: Vec<PathBuf>,
    /// Empty subdirectories, invisible to snapshots.
    empty_dirs: Vec<PathBuf>,
}

fn directory_contents(
    dir: &Path,
    steps: &[Step],
    tracked: &TrackedSet,
) -> Result<DirectoryContents> {
    // only a step that writes or deletes the file accounts for it; a file
    // the plan skips (omitted, not covered) goes with the directory and is
    // said out loud rather than silently removed
    let known: BTreeSet<&Path> = steps
        .iter()
        .filter(|step| step.action.mutates())
        .map(|step| step.path.as_path())
        .collect();
    let skipped: BTreeSet<&Path> = steps
        .iter()
        .filter(|step| !step.action.mutates())
        .map(|step| step.path.as_path())
        .collect();
    let mut out = DirectoryContents::default();
    for entry in walkdir::WalkDir::new(dir).min_depth(1).follow_links(false) {
        let entry = entry?;
        // the walked path itself: a link is the link, never its target
        let path = entry.path().to_path_buf();
        if entry.file_type().is_dir() {
            // an empty directory history would look at is recorded for undo;
            // one under an exclusion or a nested repository is not
            if std::fs::read_dir(entry.path())?.next().is_none() && tracked.would_capture(&path)? {
                out.empty_dirs.push(path);
            }
            continue;
        }
        if known.contains(path.as_path()) {
            continue;
        }
        let regular = entry.file_type().is_file() || entry.file_type().is_symlink();
        if regular && !skipped.contains(path.as_path()) && tracked.would_capture(&path)? {
            out.appeared.push(path);
        } else {
            out.uncovered.push(path);
        }
    }
    Ok(out)
}

/// What is on disk at `path` right now, as the (mode, object id) the
/// working tree sample would hold for it.
/// The permission bits git implies for a file mode; `None` for anything
/// that is not a regular file.
fn default_bits(git_mode: &str) -> Option<u32> {
    match git_mode {
        "100644" => Some(0o644),
        "100755" => Some(0o755),
        _ => None,
    }
}

/// The permission bits of a regular file when they differ from `default`
/// (the bits a checkpoint records), `None` when they are the default or
/// the path is not a regular file.
#[cfg(unix)]
fn live_bits(path: &Path, default: u32) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let bits = meta.permissions().mode() & 0o777;
    (bits != default).then_some(bits)
}

#[cfg(not(unix))]
fn live_bits(_path: &Path, _default: u32) -> Option<u32> {
    None
}

fn current_object(repo: &HistoryRepo, path: &Path) -> Result<Option<(String, String)>> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(None);
    };
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        let oid = repo.hash_blob(target.to_string_lossy().as_bytes())?;
        return Ok(Some(("120000".into(), oid)));
    }
    if meta.is_dir() {
        // an empty directory is invisible to the sample; one with files is
        // marked so a directory that appeared after protection is noticed
        let empty = std::fs::read_dir(path)?.next().is_none();
        let marker = if empty { "" } else { "nonempty" };
        return Ok(Some(("040000".into(), marker.into())));
    }
    let oid = repo.hash_blob(&std::fs::read(path)?)?;
    Ok(Some(("100644".into(), oid)))
}

/// Same kind and content; a directory's tree id is not compared because its
/// files are verified as their own steps (and checked again before the
/// directory is replaced).
fn same_object(expected: &Option<(String, String)>, current: &Option<(String, String)>) -> bool {
    match (expected, current) {
        (None, None) => true,
        // an empty directory is invisible to the sample, as it is to the plan
        (None, Some((cmode, coid))) if cmode == "040000" && coid.is_empty() => true,
        (Some((emode, eoid)), Some((cmode, coid))) => {
            kind_of(emode) == kind_of(cmode) && (emode == "040000" || eoid == coid)
        }
        _ => false,
    }
}

fn actionable(steps: &[Step]) -> Vec<(PathBuf, Action)> {
    steps
        .iter()
        .filter(|step| step.action.mutates() || matches!(step.action, Action::Conflict(_)))
        .map(|step| (step.path.clone(), step.action.clone()))
        .collect()
}

mod store {
    use super::*;

    pub(super) fn entry(store: &Store, id: u64) -> Result<Entry> {
        store
            .list()?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| eyre::eyre!("no history checkpoint {id}"))
    }
}

/// The plan for every selected path of every target.
fn plan(repo: &HistoryRepo, exec: &Execution, live: &str) -> Result<Vec<Step>> {
    let force = exec.force;
    let mut steps = vec![];
    for target in &exec.targets {
        let checkpoint = &target.entry.checkpoint;
        let Some(snapshot) = &checkpoint.tree.snapshot else {
            bail!("checkpoint {} has no content snapshot", target.entry.id);
        };
        for path in &target.paths {
            let tree_path = display_to_tree_path(&path.to_string_lossy());
            let mut files: BTreeSet<String> = BTreeSet::new();
            let mut live_files: BTreeSet<String> = BTreeSet::new();
            let mut snapshot_files: BTreeSet<String> = BTreeSet::new();
            // nested repositories in either tree: what is inside one is its
            // own, never written into or removed by a rollback
            let mut gitlinks: BTreeSet<String> = BTreeSet::new();
            for tree in [snapshot.as_str(), live] {
                match repo.object_at(tree, &tree_path)? {
                    Some((mode, _)) if mode == "040000" => {
                        for entry in repo.ls_tree(&format!("{tree}:{tree_path}"))? {
                            let file = format!("{tree_path}/{}", entry.path);
                            if entry.mode == "160000" {
                                gitlinks.insert(file.clone());
                            }
                            if tree == live {
                                live_files.insert(file.clone());
                            } else {
                                snapshot_files.insert(file.clone());
                            }
                            files.insert(file);
                        }
                    }
                    Some((mode, _)) => {
                        if mode == "160000" {
                            gitlinks.insert(tree_path.clone());
                        }
                        files.insert(tree_path.clone());
                    }
                    None => {}
                }
            }
            let inside_gitlink = |file: &str| {
                gitlinks.iter().any(|link| {
                    file.strip_prefix(link.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                })
            };
            let first_step = steps.len();
            for file in files {
                if inside_gitlink(&file) {
                    continue;
                }
                // a repository on disk the trees may not show (no commit
                // yet, say) is one too
                let abs_now = PathBuf::from(
                    tree_path_to_display(&file)
                        .replace("~/", &format!("{}/", crate::dirs::HOME.display())),
                );
                // ancestors compared in the tracked set's form: with a
                // symlinked home the raw path and the normalized named path
                // differ in their prefix
                if abs_now
                    .ancestors()
                    .skip(1)
                    .filter(|dir| super::tracked::normalize(dir).starts_with(path))
                    .any(|dir| dir.join(".git").exists())
                {
                    continue;
                }
                let abs = PathBuf::from(
                    tree_path_to_display(&file)
                        .replace("~/", &format!("{}/", crate::dirs::HOME.display())),
                );
                // the link itself, never its destination
                let abs = normalize_target(&abs);
                // An explicit child selection owns its subtree, even when
                // its differing checkpoint is older than its parent's.
                if exec
                    .targets
                    .iter()
                    .flat_map(|target| &target.paths)
                    .any(|selected| {
                        selected != path && selected.starts_with(path) && abs.starts_with(selected)
                    })
                {
                    continue;
                }
                let saved = repo.object_at(snapshot, &file)?;
                let mut current = repo.object_at(live, &file)?;
                // an empty directory is invisible to the tree; a directory
                // where the checkpoint holds a file is still a type change
                if current.is_none() && abs.is_dir() && !abs.is_symlink() {
                    current = Some(("040000".into(), String::new()));
                }
                // a file that exists but the live walk omitted (excluded,
                // over the size limit, an incomplete scan) is not missing:
                // it is not touched
                let present_uncaptured = current.is_none()
                    && std::fs::symlink_metadata(&abs).is_ok_and(|meta| !meta.is_dir());
                let (mut action, mut from, mut to) = if present_uncaptured {
                    (
                        Action::Skip("present but not captured (excluded or omitted)".into()),
                        "present".into(),
                        "?".into(),
                    )
                } else {
                    decide(checkpoint, &file, saved.clone(), current, force)
                };
                let mut bits = recorded_bits(checkpoint, &file, &abs);
                // the same bytes under other permissions: a change too
                if matches!(action, Action::Unchanged)
                    && let Some((smode, soid)) = &saved
                    && let Some(default) = default_bits(smode)
                    && live_bits(&abs, default) != bits
                {
                    let recorded = bits.unwrap_or(default);
                    from = format!("mode {:o}", live_bits(&abs, default).unwrap_or(default));
                    to = format!("mode {recorded:o}");
                    action = Action::Write {
                        mode: smode.clone(),
                        oid: soid.clone(),
                    };
                    bits = Some(recorded);
                }
                let dir_bits = abs
                    .ancestors()
                    .skip(1)
                    .filter_map(|dir| {
                        let bits = checkpoint.tree.modes.get(&display_path(dir))?;
                        Some((dir.to_path_buf(), *bits))
                    })
                    .collect();
                steps.push(Step {
                    path: abs,
                    tree_path: file,
                    action,
                    from,
                    to,
                    bits,
                    dir_bits,
                });
            }
            // a directory the checkpoint knows nothing of (the named path
            // itself, or one below it) whose every file the plan deletes
            // goes with them: returning to a known absence leaves no empty
            // folder behind
            let deleted: BTreeSet<String> = steps[first_step..]
                .iter()
                .filter(|step| matches!(step.action, Action::Delete))
                .map(|step| step.tree_path.clone())
                .collect();
            let under = |file: &str, dir: &str| {
                file.strip_prefix(dir)
                    .is_some_and(|rest| rest.starts_with('/'))
            };
            let mut emptied: BTreeSet<String> = BTreeSet::new();
            if path.is_dir()
                && !path.is_symlink()
                && std::fs::read_dir(path)?.next().is_none()
                && matches!(
                    classify(checkpoint, &tree_path_to_display(&tree_path)),
                    PathState::Absent
                )
            {
                emptied.insert(tree_path.clone());
            }
            for file in &deleted {
                let mut dir = file.as_str();
                while let Some((parent, _)) = dir.rsplit_once('/') {
                    if parent.len() < tree_path.len() {
                        break;
                    }
                    emptied.insert(parent.to_string());
                    dir = parent;
                }
            }
            for dir in emptied {
                if snapshot_files.iter().any(|file| under(file, &dir))
                    || repo.object_at(snapshot, &dir)?.is_some()
                    || live_files
                        .iter()
                        .any(|file| under(file, &dir) && !deleted.contains(file))
                    || gitlinks
                        .iter()
                        .any(|link| under(link, &dir) || *link == dir)
                {
                    continue;
                }
                let abs = normalize_target(&PathBuf::from(
                    tree_path_to_display(&dir)
                        .replace("~/", &format!("{}/", crate::dirs::HOME.display())),
                ));
                if !abs.is_dir() || abs.is_symlink() {
                    continue;
                }
                steps.push(Step {
                    path: abs,
                    tree_path: dir,
                    action: Action::Delete,
                    from: "a directory".into(),
                    to: "missing".into(),
                    bits: None,
                    dir_bits: vec![],
                });
            }
        }
    }
    steps.sort_by(|a, b| a.path.cmp(&b.path));
    steps.dedup_by(|a, b| a.path == b.path);
    occupied_restore_dirs(repo, exec, live, &mut steps)?;
    Ok(steps)
}

/// A recorded empty directory whose path now holds a symlink or a file is
/// part of the plan, so it is decided before anything is written instead
/// of failing after other paths changed: a captured occupant is removed
/// like any other step (protected, journaled); one history does not
/// capture (excluded, say) cannot be protected, so it is a conflict that
/// `--force` does not override. An occupant inside a directory the plan
/// removes goes with it.
fn occupied_restore_dirs(
    repo: &HistoryRepo,
    exec: &Execution,
    live: &str,
    steps: &mut Vec<Step>,
) -> Result<()> {
    for dir in &exec.restore_dirs {
        let Ok(meta) = std::fs::symlink_metadata(dir) else {
            continue;
        };
        if meta.is_dir() {
            continue;
        }
        if steps
            .iter()
            .any(|step| &step.path == dir || (step.action.mutates() && dir.starts_with(&step.path)))
        {
            continue;
        }
        let kind = if meta.file_type().is_symlink() {
            "a symlink"
        } else {
            "a file"
        };
        let tree_path = display_to_tree_path(&dir.to_string_lossy());
        let action = match repo.object_at(live, &tree_path)? {
            Some(_) => Action::Delete,
            None => Action::Conflict(format!(
                "{kind} history does not capture stands where a directory was; remove it first"
            )),
        };
        steps.push(Step {
            path: dir.clone(),
            tree_path,
            action,
            from: kind.into(),
            to: "an empty directory".into(),
            bits: None,
            dir_bits: vec![],
        });
    }
    steps.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(())
}

fn kind_of(mode: &str) -> &'static str {
    match mode {
        "120000" => "a symlink",
        "160000" => "a nested repository",
        "040000" => "a directory",
        _ => "a file",
    }
}

/// The permission bits a checkpoint recorded for a path. Captures key the
/// record by the display form of the walked (canonical) path, which with a
/// symlinked home is not the `~/…` the tree path renders to: both forms
/// are tried.
fn recorded_bits(checkpoint: &Checkpoint, tree_path: &str, abs: &Path) -> Option<u32> {
    checkpoint
        .tree
        .modes
        .get(&tree_path_to_display(tree_path))
        .or_else(|| checkpoint.tree.modes.get(&display_path(abs)))
        .copied()
}

fn decide(
    checkpoint: &Checkpoint,
    tree_path: &str,
    saved: Option<(String, String)>,
    current: Option<(String, String)>,
    force: bool,
) -> (Action, String, String) {
    let display = tree_path_to_display(tree_path);
    match (saved, current) {
        (Some((smode, soid)), Some((cmode, coid))) => {
            let from = if coid.is_empty() {
                kind_of(&cmode).to_string()
            } else {
                format!("{} {}", kind_of(&cmode), &coid[..7])
            };
            let to = format!("{} {}", kind_of(&smode), &soid[..7]);
            if smode == cmode && soid == coid {
                return (Action::Unchanged, from, to);
            }
            if smode == "160000" || cmode == "160000" {
                return (Action::Skip("nested repository".into()), from, to);
            }
            if kind_of(&smode) != kind_of(&cmode) && !force {
                return (
                    Action::Conflict(format!("{} became {}", kind_of(&smode), kind_of(&cmode))),
                    from,
                    to,
                );
            }
            (
                Action::Write {
                    mode: smode,
                    oid: soid,
                },
                from,
                to,
            )
        }
        (Some((smode, soid)), None) => {
            if smode == "160000" {
                return (
                    Action::Skip("nested repository".into()),
                    "missing".into(),
                    kind_of(&smode).into(),
                );
            }
            (
                Action::Write {
                    mode: smode.clone(),
                    oid: soid.clone(),
                },
                "missing".into(),
                format!("{} {}", kind_of(&smode), &soid[..7]),
            )
        }
        (None, Some((cmode, coid))) => {
            if coid.is_empty() {
                // an empty directory the checkpoint knows nothing about
                return (Action::Unchanged, "a directory".into(), "?".into());
            }
            if cmode == "160000" {
                // a nested repository is never removed by a rollback
                return (
                    Action::Skip("nested repository".into()),
                    "a nested repository".into(),
                    "missing".into(),
                );
            }
            let from = format!("{} {}", kind_of(&cmode), &coid[..7]);
            match classify(checkpoint, &display) {
                PathState::Absent => (Action::Delete, from, "missing".into()),
                PathState::Uncovered => (
                    Action::Skip(format!(
                        "not covered by checkpoint {}",
                        &checkpoint.uuid[..8]
                    )),
                    from,
                    "?".into(),
                ),
                PathState::Omitted(reason) => (Action::Skip(reason), from, "?".into()),
            }
        }
        (None, None) => (Action::Unchanged, "missing".into(), "missing".into()),
    }
}

enum PathState {
    Absent,
    Uncovered,
    Omitted(String),
}

/// What a checkpoint says about a path it does not hold.
fn classify(checkpoint: &Checkpoint, display: &str) -> PathState {
    let coverage = &checkpoint.tree.coverage;
    let under = |prefix: &str| {
        display == prefix
            || display
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    for omitted in &coverage.omitted {
        if under(&omitted.path) {
            return PathState::Omitted(omitted.reason.clone());
        }
    }
    for incomplete in &coverage.incomplete {
        if under(&incomplete.path) {
            return PathState::Omitted(format!("scan incomplete: {}", incomplete.reason));
        }
    }
    let covered = coverage.entries.iter().any(|entry| under(&entry.path))
        || coverage.derived.iter().any(|derived| under(&derived.path));
    if !covered {
        return PathState::Uncovered;
    }
    if let Ok(exclude) = super::tracked::ExcludeSet::new(&coverage.exclude)
        && exclude.is_match(&file::replace_path(Path::new(display)))
    {
        return PathState::Uncovered;
    }
    PathState::Absent
}

fn print_plan(steps: &[Step], exec: &Execution, tracked: &TrackedSet) -> Result<()> {
    let mut table = MiseTable::new(false, &["Path", "Action", "From", "To"]);
    for step in steps {
        if matches!(step.action, Action::Unchanged) {
            continue;
        }
        table.add_row(vec![
            display_path(&step.path),
            step.action.label(),
            step.from.clone(),
            step.to.clone(),
        ]);
    }
    // an occupied one is a step of the plan
    for dir in &exec.restore_dirs {
        if std::fs::symlink_metadata(dir).is_err() {
            table.add_row(vec![
                display_path(dir),
                "recreate".into(),
                "missing".into(),
                "an empty directory".into(),
            ]);
        }
    }
    let unchanged = steps
        .iter()
        .filter(|step| matches!(step.action, Action::Unchanged))
        .count();
    table.print()?;
    if unchanged > 0 {
        miseprintln!("  {unchanged} path(s) already match");
    }
    // a directory being replaced or removed takes files no checkpoint holds
    // with it: said before the plan is accepted, not after
    for step in steps.iter().filter(|step| step.action.mutates()) {
        let replaces_dir = step.path.is_dir()
            && !step.path.is_symlink()
            && !matches!(&step.action, Action::Write { mode, .. } if mode == "040000");
        if !replaces_dir {
            continue;
        }
        let inside = directory_contents(&step.path, steps, tracked)?;
        if !inside.uncovered.is_empty() {
            miseprintln!(
                "  {} contains files history does not cover; they are removed with it and cannot be undone: {}",
                display_path(&step.path),
                inside
                    .uncovered
                    .iter()
                    .map(display_path)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    if exec.dry_run {
        miseprintln!("history: dry run; nothing was changed");
    }
    Ok(())
}

/// The tracked set as it is on disk right now, as a tree in the repository
/// (objects only; nothing is recorded).
pub(crate) fn live_tree(repo: &HistoryRepo, tracked: &TrackedSet) -> Result<String> {
    let walk = tracked.walk()?;
    Ok(repo.capture(&walk.roots)?.tree)
}

/// The newest checkpoint whose captured content for `path` differs from the
/// working tree, if any.
fn newest_differing(
    repo: &HistoryRepo,
    entries: &[Entry],
    live: &str,
    path: &Path,
) -> Result<Option<Entry>> {
    let tree_path = display_to_tree_path(&path.to_string_lossy());
    let current = repo.object_at(live, &tree_path)?;
    for entry in entries.iter().rev() {
        let Some(snapshot) = &entry.checkpoint.tree.snapshot else {
            continue;
        };
        if entry.checkpoint.status() == Some(OperationStatus::Pending) {
            continue;
        }
        let saved = repo.object_at(snapshot, &tree_path)?;
        if saved == current {
            if saved.is_none()
                && normalize_target(path).is_dir()
                && matches!(
                    classify(&entry.checkpoint, &tree_path_to_display(&tree_path)),
                    PathState::Absent
                )
            {
                return Ok(Some(entry.clone()));
            }
            // the same bytes under other permissions differ too
            let recorded = recorded_bits(&entry.checkpoint, &tree_path, &normalize_target(path));
            let differs = saved
                .as_ref()
                .and_then(|(mode, _)| default_bits(mode))
                .is_some_and(|default| live_bits(&normalize_target(path), default) != recorded);
            if !differs {
                continue;
            }
            return Ok(Some(entry.clone()));
        }
        // held content that differs, or a known absence while the path now
        // exists, are both versions to return to
        let differs = saved.is_some()
            || matches!(
                classify(&entry.checkpoint, &tree_path_to_display(&tree_path)),
                PathState::Absent
            );
        if differs {
            return Ok(Some(entry.clone()));
        }
    }
    Ok(None)
}

fn refuse_unusable(entry: &Entry) -> Result<()> {
    if entry.checkpoint.status() == Some(OperationStatus::Pending) {
        bail!(
            "checkpoint {} did not finish; there is no state to roll back to",
            entry.id
        );
    }
    if entry.checkpoint.tree.snapshot.is_none() {
        bail!("checkpoint {} has no content snapshot", entry.id);
    }
    if entry.checkpoint.schema_version > super::store::SCHEMA_VERSION {
        bail!(
            "checkpoint {} was written by a newer mise (schema {})",
            entry.id,
            entry.checkpoint.schema_version
        );
    }
    Ok(())
}

/// Every path a checkpoint's coverage includes, as absolute paths.
fn covered_paths(checkpoint: &Checkpoint) -> Vec<PathBuf> {
    checkpoint
        .tree
        .coverage
        .entries
        .iter()
        .filter(|entry| entry.mode != "private")
        .map(|entry| normalize_target(Path::new(&entry.path)))
        .collect()
}

/// Puts the object a step names at its path. Permissions: a regular file
/// that is replaced keeps the bits it has (only the executable bit follows
/// the checkpoint), a file the checkpoint recorded a mode for gets exactly
/// that mode, and the bytes never exist under a wider mode in between.
fn write_object(repo: &HistoryRepo, step: &Step, mode: &str, oid: &str) -> Result<()> {
    let path = &step.path;
    // only a directory this restore creates gets its recorded mode; one
    // that already exists keeps the mode it has
    let created: Vec<(PathBuf, u32)> = step
        .dir_bits
        .iter()
        .filter(|(dir, _)| std::fs::symlink_metadata(dir).is_err())
        .cloned()
        .collect();
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    restore_dir_modes(step, &created);
    if mode == "040000" {
        // a directory replaces a file or link; its files are their own steps
        if path.is_symlink() || path.is_file() {
            std::fs::remove_file(path)?;
        }
        file::create_dir_all(path)?;
        // its own recorded mode, now, not once a child is written
        #[cfg(unix)]
        if let Some(bits) = step.bits {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))?;
        }
        return Ok(());
    }
    let bytes = repo.cat_object(oid)?;
    if path.is_dir() && !path.is_symlink() {
        std::fs::remove_dir_all(path)?;
    } else if path.is_symlink() || (mode == "120000" && path.exists()) {
        std::fs::remove_file(path)?;
    }
    if mode == "120000" {
        let target = String::from_utf8_lossy(&bytes).to_string();
        file::make_symlink(Path::new(&target), path)?;
        return Ok(());
    }
    #[cfg(unix)]
    let bits = {
        use std::os::unix::fs::PermissionsExt;
        let existing = std::fs::symlink_metadata(path)
            .ok()
            .filter(|meta| meta.is_file())
            .map(|meta| meta.permissions().mode() & 0o777);
        let bits = match (step.bits, existing) {
            (Some(bits), _) => bits,
            (None, Some(bits)) if mode == "100755" => bits | ((bits & 0o444) >> 2),
            (None, Some(bits)) => bits & !0o111,
            (None, None) if mode == "100755" => 0o755,
            (None, None) => 0o644,
        };
        // the bytes never exist under a wider mode than the one they get:
        // a private file restored from deletion is created private, and an
        // existing wider file is restricted before it is rewritten (the
        // atomic write keeps an existing file's mode)
        match existing {
            None if bits & 0o077 != 0o044 => {
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(bits & 0o666)
                    .open(path)?;
            }
            Some(current) if current & !bits != 0 => {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))?;
            }
            _ => {}
        }
        bits
    };
    // a regular file is replaced in place: the atomic write keeps its mode
    file::write_atomic(path, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))?;
    }
    Ok(())
}

/// Puts back the recorded mode of the directories above a restored file
/// (a `0700` directory recreated by `create_dir_all` would be `0755`).
#[cfg(unix)]
fn restore_dir_modes(_step: &Step, created: &[(PathBuf, u32)]) {
    use std::os::unix::fs::PermissionsExt;
    for (dir, bits) in created {
        if dir.is_dir() && !dir.is_symlink() {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(*bits));
        }
    }
}

#[cfg(not(unix))]
fn restore_dir_modes(step: &Step, _created: &[(PathBuf, u32)]) {
    if step.bits.is_some() || !step.dir_bits.is_empty() {
        debug!(
            "history: {}: recorded permission bits are not restored on this platform",
            display_path(&step.path)
        );
    }
}

fn remove(path: &Path) -> Result<()> {
    if path.is_symlink() || path.is_file() {
        std::fs::remove_file(path)?;
    } else if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

/// Runs the reload commands whose glob matches a touched path, each once,
/// best-effort.
fn run_reload(reload: &IndexMap<String, String>, touched: &[PathBuf]) {
    let mut commands: Vec<&String> = vec![];
    for (glob, command) in reload {
        // touched paths are normalized (a symlinked `$HOME` resolved), so
        // the glob's home is too
        let expanded = match glob.strip_prefix("~/") {
            Some(rest) => super::tracked::normalize(&crate::dirs::HOME).join(rest),
            None => file::replace_path(Path::new(glob)),
        };
        let Ok(pattern) = globset::Glob::new(&expanded.to_string_lossy()) else {
            warn!("history: invalid [history.reload] glob {glob:?}");
            continue;
        };
        let matcher = pattern.compile_matcher();
        if touched.iter().any(|path| matcher.is_match(path)) && !commands.contains(&command) {
            commands.push(command);
        }
    }
    for command in commands {
        info!("history: reload: {command}");
        let status = if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/C", command])
                .status()
        } else {
            std::process::Command::new("sh")
                .args(["-c", command])
                .status()
        };
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => warn!("history: reload command failed ({status}): {command}"),
            Err(err) => warn!("history: could not run reload command {command:?}: {err}"),
        }
    }
}

/// Restoring configuration never runs bootstrap: say when declarations may
/// now differ from the applied setup.
fn config_hint(touched: &[PathBuf]) {
    let config_dir = normalize(&super::tracked::global_config_dir());
    let config_files: Vec<String> = touched
        .iter()
        .filter(|path| {
            path.starts_with(&config_dir) && path.extension().is_some_and(|ext| ext == "toml")
        })
        .map(display_path)
        .collect();
    if !config_files.is_empty() {
        info!(
            "history: {} changed; declarations may differ from the applied setup — run `mise bootstrap --dry-run` to see",
            config_files.join(", ")
        );
    }
}
