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
use super::store::{Checkpoint, Entry, OperationKind, OperationStatus, Summary};
use super::tracked::{TrackedSet, display_to_tree_path, normalize, tree_path_to_display};
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
}

/// One checkpoint and the paths to take from it.
struct Target {
    entry: Entry,
    paths: Vec<PathBuf>,
}

pub(crate) async fn rollback(req: RollbackRequest) -> Result<()> {
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
    let paths: Vec<PathBuf> = req.paths.iter().map(|path| normalize(path)).collect();
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
    execute(
        Execution {
            kind: OperationKind::Rollback,
            command,
            targets,
            message,
            to,
            undoes: None,
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
    let (store, tracked, entries) = crate::cli::dotfiles::history::open().await?;
    let repo = store
        .repo()
        .ok_or_else(|| eyre::eyre!("undoing requires git"))?;
    let live = live_tree(repo, &tracked)?;
    let undone: BTreeSet<String> = entries
        .iter()
        .filter_map(|entry| entry.checkpoint.operation.as_ref()?.undoes.clone())
        .collect();
    let operation = match &req.reference {
        Some(reference) => crate::cli::dotfiles::history::resolve(reference, &entries, None)?,
        None => entries
            .iter()
            .rev()
            .find(|entry| {
                entry.checkpoint.operation.as_ref().is_some_and(|op| {
                    matches!(
                        op.kind,
                        OperationKind::Rollback | OperationKind::Undo | OperationKind::Apply
                    ) && op.status == OperationStatus::Completed
                        && op.before.is_some()
                        && !op.affected.is_empty()
                }) && !undone.contains(&entry.checkpoint.uuid)
            })
            .cloned()
            .ok_or_else(|| eyre::eyre!("nothing to undo: no rollback, undo, or apply is left"))?,
    };
    let Some(op) = operation.checkpoint.operation.clone() else {
        bail!("checkpoint {} is not an operation", operation.id);
    };
    if !matches!(
        op.kind,
        OperationKind::Rollback | OperationKind::Undo | OperationKind::Apply
    ) {
        bail!(
            "checkpoint {} is a {} operation; only rollback, undo, and apply can be undone",
            operation.id,
            op.kind.as_str()
        );
    }
    if op.status != OperationStatus::Completed {
        bail!(
            "checkpoint {} did not finish; nothing to undo",
            operation.id
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
        .map(|path| normalize(Path::new(path)))
        .collect();
    let message = format!(
        "undid {} {} ({})",
        op.kind.as_str(),
        operation.id,
        op.affected.join(", ")
    );
    execute(
        Execution {
            kind: OperationKind::Undo,
            command: format!("bootstrap dotfiles undo {}", operation.id),
            targets: vec![Target {
                entry: before.clone(),
                paths,
            }],
            message,
            to: Some(before.checkpoint.uuid.clone()),
            undoes: Some(operation.checkpoint.uuid.clone()),
            dry_run: req.dry_run,
            yes: req.yes,
            force: false,
        },
        &store,
        &tracked,
        &entries,
        live,
    )
    .await
}

struct Execution {
    kind: OperationKind,
    command: String,
    targets: Vec<Target>,
    message: String,
    to: Option<String>,
    undoes: Option<String>,
    dry_run: bool,
    yes: bool,
    force: bool,
}

async fn execute(
    exec: Execution,
    store: &Store,
    tracked: &TrackedSet,
    entries: &[Entry],
    live: String,
) -> Result<()> {
    let repo = store.repo().expect("checked by the caller");
    let mut steps = plan(repo, &exec.targets, &live, exec.force)?;
    print_plan(&steps, &exec)?;
    if exec.dry_run {
        return Ok(());
    }
    let conflicts: Vec<&Step> = steps
        .iter()
        .filter(|step| matches!(step.action, Action::Conflict(_)))
        .collect();
    if !conflicts.is_empty() {
        bail!(
            "{} path(s) conflict; resolve them or pass --force to replace them",
            conflicts.len()
        );
    }
    if !steps.iter().any(|step| step.action.mutates()) {
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
        op.undoes = exec.undoes.clone();
    });
    let result = apply_steps(&scope, repo, store, tracked, &exec, entries, &mut steps).await;
    let (error, summary) = match &result {
        Ok(touched) => {
            let affected: Vec<String> = touched.iter().map(display_path).collect();
            scope.with_operation(|op| op.affected = affected.clone());
            scope.promote(touched);
            (
                None,
                Summary {
                    parts: vec!["history".into()],
                    message: Some(exec.message.clone()),
                },
            )
        }
        Err(err) => (
            Some(format!("{err:#}")),
            Summary {
                parts: vec!["history".into()],
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
    loop {
        round += 1;
        // editors may have written while the prompt was open: re-plan
        let live = live_tree(repo, tracked)?;
        let fresh = plan(repo, &exec.targets, &live, exec.force)?;
        let changed = actionable(&fresh) != actionable(steps);
        if changed {
            *steps = fresh.clone();
            miseprintln!("history: the working tree changed since the plan was shown:");
            print_plan(steps, exec)?;
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
    for step in steps.iter().filter(|step| step.action.mutates()) {
        let pending =
            journal::begin_changes("history", &display_path(&step.path), [step.path.clone()])?;
        match &step.action {
            Action::Write { mode, oid } => write_object(repo, &step.path, mode, oid)?,
            Action::Delete => remove(&step.path)?,
            _ => unreachable!("filtered to mutating steps"),
        }
        journal::commit_changes(pending);
        touched.push(step.path.clone());
    }
    Ok(touched)
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
fn plan(repo: &HistoryRepo, targets: &[Target], live: &str, force: bool) -> Result<Vec<Step>> {
    let mut steps = vec![];
    for target in targets {
        let checkpoint = &target.entry.checkpoint;
        let Some(snapshot) = &checkpoint.tree.snapshot else {
            bail!("checkpoint {} has no content snapshot", target.entry.id);
        };
        for path in &target.paths {
            let tree_path = display_to_tree_path(&path.to_string_lossy());
            let mut files: BTreeSet<String> = BTreeSet::new();
            for tree in [snapshot.as_str(), live] {
                match repo.object_at(tree, &tree_path)? {
                    Some((mode, _)) if mode == "040000" => {
                        for entry in repo.ls_tree(&format!("{tree}:{tree_path}"))? {
                            files.insert(format!("{tree_path}/{}", entry.path));
                        }
                    }
                    Some(_) => {
                        files.insert(tree_path.clone());
                    }
                    None => {}
                }
            }
            for file in files {
                let abs = PathBuf::from(
                    tree_path_to_display(&file)
                        .replace("~/", &format!("{}/", crate::dirs::HOME.display())),
                );
                let abs = normalize(&abs);
                let saved = repo.object_at(snapshot, &file)?;
                let mut current = repo.object_at(live, &file)?;
                // an empty directory is invisible to the tree; a directory
                // where the checkpoint holds a file is still a type change
                if current.is_none() && abs.is_dir() && !abs.is_symlink() {
                    current = Some(("040000".into(), String::new()));
                }
                let (action, from, to) = decide(checkpoint, &file, saved, current, force);
                steps.push(Step {
                    path: abs,
                    tree_path: file,
                    action,
                    from,
                    to,
                });
            }
        }
    }
    steps.sort_by(|a, b| a.path.cmp(&b.path));
    steps.dedup_by(|a, b| a.path == b.path);
    Ok(steps)
}

fn kind_of(mode: &str) -> &'static str {
    match mode {
        "120000" => "a symlink",
        "160000" => "a nested repository",
        "040000" => "a directory",
        _ => "a file",
    }
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
    for glob in &coverage.exclude {
        let expanded = file::replace_path(Path::new(glob));
        if let Ok(pattern) = globset::Glob::new(&expanded.to_string_lossy()) {
            let expanded_path = file::replace_path(Path::new(display));
            if pattern.compile_matcher().is_match(&expanded_path) {
                return PathState::Uncovered;
            }
        }
    }
    PathState::Absent
}

fn print_plan(steps: &[Step], exec: &Execution) -> Result<()> {
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
    let unchanged = steps
        .iter()
        .filter(|step| matches!(step.action, Action::Unchanged))
        .count();
    table.print()?;
    if unchanged > 0 {
        miseprintln!("  {unchanged} path(s) already match");
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
        if saved.is_some() && saved != current {
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
        .map(|entry| normalize(Path::new(&entry.path)))
        .collect()
}

fn write_object(repo: &HistoryRepo, path: &Path, mode: &str, oid: &str) -> Result<()> {
    let bytes = repo.cat_object(oid)?;
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    if path.is_dir() && !path.is_symlink() {
        std::fs::remove_dir_all(path)?;
    } else if path.exists() || path.is_symlink() {
        std::fs::remove_file(path)?;
    }
    if mode == "120000" {
        let target = String::from_utf8_lossy(&bytes).to_string();
        file::make_symlink(Path::new(&target), path)?;
        return Ok(());
    }
    file::write_atomic(path, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perm = if mode == "100755" { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(perm))?;
    }
    Ok(())
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
        let expanded = file::replace_path(Path::new(glob));
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
