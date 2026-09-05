//! The on-disk history store under `$MISE_STATE_DIR/history/`.
//!
//! The bare repository `repo.git` is the complete representation: every
//! checkpoint is a wrapper commit holding the snapshot tree, its `meta.json`
//! record, and the journal blobs it references. Everything else in the
//! directory is a rebuildable index or machine-local bookkeeping. Every
//! function takes the state directory explicitly (`*_in`) so tests can point
//! it at a temporary directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};

use super::journal::JournalEntry;
use crate::file::{self, display_path};

pub(crate) const SCHEMA_VERSION: u32 = 1;

pub(crate) fn store_dir_in(state_dir: &Path) -> PathBuf {
    state_dir.join("history")
}

pub(crate) fn repo_dir_in(state_dir: &Path) -> PathBuf {
    store_dir_in(state_dir).join("repo.git")
}

pub(crate) fn index_dir_in(state_dir: &Path) -> PathBuf {
    store_dir_in(state_dir).join("index")
}

fn index_file_in(state_dir: &Path) -> PathBuf {
    index_dir_in(state_dir).join("checkpoints.json")
}

fn meta_cache_dir_in(state_dir: &Path) -> PathBuf {
    index_dir_in(state_dir).join("meta")
}

pub(crate) fn pending_dir_in(state_dir: &Path) -> PathBuf {
    index_dir_in(state_dir).join("pending")
}

pub(crate) fn machine_file_in(state_dir: &Path) -> PathBuf {
    store_dir_in(state_dir).join("machine.json")
}

pub(crate) fn operation_marker_in(state_dir: &Path) -> PathBuf {
    store_dir_in(state_dir).join("operation.json")
}

pub(crate) fn operation_lock_in(state_dir: &Path) -> PathBuf {
    store_dir_in(state_dir).join("operation")
}

/// The lock serializing captures, index writes, and pruning.
pub(crate) fn store_lock_path_in(state_dir: &Path) -> PathBuf {
    store_dir_in(state_dir).join("store")
}

/// Creates the store directory, private to the user: snapshots hold
/// whatever the tracked paths hold, secrets included.
pub(crate) fn ensure_store_dir_in(state_dir: &Path) -> Result<()> {
    let dir = store_dir_in(state_dir);
    create_private_dir(&dir)?;
    create_private_dir(&index_dir_in(state_dir))?;
    create_private_dir(&meta_cache_dir_in(state_dir))?;
    create_private_dir(&pending_dir_in(state_dir))?;
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !dir.is_dir() {
        if let Some(parent) = dir.parent() {
            file::create_dir_all(parent)?;
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .wrap_err_with(|| format!("creating {}", display_path(dir)))?;
    }
    let mode = std::fs::metadata(dir)?.permissions().mode() & 0o777;
    if mode != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .wrap_err_with(|| format!("restricting {}", display_path(dir)))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<()> {
    file::create_dir_all(dir)
}

/// This machine's stable identity, created on first use.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Machine {
    pub id: String,
    pub name: String,
}

pub(crate) fn machine_in(state_dir: &Path) -> Result<Machine> {
    let path = machine_file_in(state_dir);
    if path.exists() {
        let text = file::read_to_string(&path)?;
        return serde_json::from_str(&text)
            .wrap_err_with(|| format!("reading {}", display_path(&path)));
    }
    let machine = Machine {
        id: uuid::Uuid::new_v4().to_string(),
        name: hostname(),
    };
    write_json(&path, &machine)?;
    Ok(machine)
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|name| !name.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "machine".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Trigger {
    Edit,
    Save,
    Agent,
    Baseline,
    BootstrapBefore,
    Bootstrap,
    RollbackBefore,
    Rollback,
    UndoBefore,
    Undo,
    ApplyBefore,
    Apply,
    Update,
}

impl Trigger {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Save => "save",
            Self::Agent => "agent",
            Self::Baseline => "baseline",
            Self::BootstrapBefore => "bootstrap-before",
            Self::Bootstrap => "bootstrap",
            Self::RollbackBefore => "rollback-before",
            Self::Rollback => "rollback",
            Self::UndoBefore => "undo-before",
            Self::Undo => "undo",
            Self::ApplyBefore => "apply-before",
            Self::Apply => "apply",
            Self::Update => "update",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "edit" => Self::Edit,
            "save" => Self::Save,
            "agent" => Self::Agent,
            "baseline" => Self::Baseline,
            "bootstrap-before" => Self::BootstrapBefore,
            "bootstrap" => Self::Bootstrap,
            "rollback-before" => Self::RollbackBefore,
            "rollback" => Self::Rollback,
            "undo-before" => Self::UndoBefore,
            "undo" => Self::Undo,
            "apply-before" => Self::ApplyBefore,
            "apply" => Self::Apply,
            "update" => Self::Update,
            _ => return None,
        })
    }

    /// A capture with no metadata of its own: recorded only when something
    /// changed. A bare `mise bootstrap dotfiles save` counts; one with a description,
    /// a label, or a task always records.
    pub(crate) fn is_automatic(self) -> bool {
        matches!(self, Self::Edit | Self::Save)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DescriptionSource {
    Computed,
    User,
    Agent,
    Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OperationKind {
    Bootstrap,
    Rollback,
    Undo,
    Apply,
    BootstrapRollback,
}

impl OperationKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Rollback => "rollback",
            Self::Undo => "undo",
            Self::Apply => "apply",
            Self::BootstrapRollback => "bootstrap-rollback",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationStatus {
    /// The command that owns this operation has not finished (or died).
    Pending,
    Completed,
    Failed,
}

impl OperationStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

/// The `meta.json` record of one checkpoint. Immutable once its wrapper
/// commit exists; descriptions, labels, and pins change through annotations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Checkpoint {
    pub schema_version: u32,
    pub uuid: String,
    pub machine: Machine,
    /// RFC 3339, UTC.
    pub created_at: String,
    pub mise_version: String,
    pub trigger: Trigger,
    pub description: String,
    pub description_source: DescriptionSource,
    /// The computed description, kept even when a caller supplied one.
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
    pub tree: TreeInfo,
    pub changes: Changes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
}

impl Checkpoint {
    /// The trigger label plus the operation kind, for tables.
    pub(crate) fn kind_label(&self) -> String {
        self.trigger.as_str().to_string()
    }

    pub(crate) fn status(&self) -> Option<OperationStatus> {
        self.operation.as_ref().map(|operation| operation.status)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TreeInfo {
    /// The snapshot tree inside the wrapper commit (`snapshot/`).
    pub snapshot: Option<String>,
    /// False when no content snapshot could be taken (no usable `git`).
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub roots: Vec<RootRecord>,
    pub coverage: Coverage,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct RootRecord {
    /// `home` for `$HOME`, `fs` for everything outside it.
    pub label: String,
    pub path: PathBuf,
    pub files: u64,
    pub bytes: u64,
}

/// The effective rules a capture ran under, persisted so a checkpoint can
/// say for any path whether it was captured, known absent, uncovered, or
/// omitted.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Coverage {
    pub entries: Vec<CoverageEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived: Vec<DerivedRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incomplete: Vec<PathReason>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<PathReason>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CoverageEntry {
    /// `~`-relative when under `$HOME`, absolute otherwise.
    pub path: String,
    /// `track`, `implicit`, `source`, `template`, `copy`, `content`, …
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub autosave: bool,
    pub share: bool,
    pub backup: bool,
    /// `live`, `saved`, or `protective`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_in: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DerivedRecord {
    pub path: String,
    pub from: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PathReason {
    pub path: String,
    pub reason: String,
}

/// What changed since the previous checkpoint's snapshot, as `~`-relative
/// (or absolute) paths.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Changes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    #[serde(default)]
    pub truncated: bool,
}

impl Changes {
    pub(crate) fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.added.len() + self.modified.len() + self.removed.len()
    }

    /// Whether `path` (or anything under it) changed.
    pub(crate) fn touches(&self, path: &str) -> bool {
        let under = |candidate: &String| {
            candidate == path
                || candidate
                    .strip_prefix(path)
                    .is_some_and(|rest| rest.starts_with('/'))
        };
        self.added.iter().any(under)
            || self.modified.iter().any(under)
            || self.removed.iter().any(under)
    }
}

/// The outcome half of an operation pair.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Operation {
    pub kind: OperationKind,
    pub status: OperationStatus,
    /// The mise command line without `argv[0]`, e.g. `bootstrap --yes`.
    pub command: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub user: Option<String>,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    /// The protective checkpoint taken before the operation ran.
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The file changes the operation made, with their preimages.
    #[serde(default)]
    pub journal: Vec<JournalEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Summary {
    pub message: Option<String>,
}

/// One line of the local index: enough to list checkpoints without
/// reading the repository.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct IndexEntry {
    pub id: u64,
    pub uuid: String,
    pub commit: String,
    pub created_at: String,
    pub trigger: Trigger,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Index {
    pub next_id: u64,
    pub entries: Vec<IndexEntry>,
}

impl Index {
    pub(crate) fn by_uuid(&self, uuid: &str) -> Option<&IndexEntry> {
        self.entries.iter().find(|entry| entry.uuid == uuid)
    }

    pub(crate) fn newest(&self) -> Option<&IndexEntry> {
        self.entries.last()
    }
}

pub(crate) fn index_exists_in(state_dir: &Path) -> bool {
    index_file_in(state_dir).exists()
}

pub(crate) fn load_index_in(state_dir: &Path) -> Result<Index> {
    let path = index_file_in(state_dir);
    if !path.exists() {
        return Ok(Index {
            next_id: 1,
            entries: vec![],
        });
    }
    let text = file::read_to_string(&path)?;
    serde_json::from_str(&text).wrap_err_with(|| format!("reading {}", display_path(&path)))
}

pub(crate) fn write_index_in(state_dir: &Path, index: &Index) -> Result<()> {
    write_json(&index_file_in(state_dir), index)
}

/// A checkpoint together with its local handle.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Entry {
    pub id: u64,
    pub commit: String,
    #[serde(flatten)]
    pub checkpoint: Checkpoint,
}

/// Mutable data kept out of the immutable record: a git note on the
/// wrapper commit, mirrored into the cached record.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Annotation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_source: Option<DescriptionSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    pub updated_at: String,
}

impl Annotation {
    pub(crate) fn apply_to(&self, checkpoint: &mut Checkpoint) {
        if let Some(description) = &self.description {
            checkpoint.description = description.clone();
            checkpoint.description_source =
                self.description_source.unwrap_or(DescriptionSource::User);
        }
        if let Some(pinned) = self.pinned {
            checkpoint.pinned = pinned;
        }
        if let Some(labels) = &self.labels {
            checkpoint.labels = labels.clone();
        }
    }
}

pub(crate) fn meta_cache_path_in(state_dir: &Path, uuid: &str) -> PathBuf {
    meta_cache_dir_in(state_dir).join(format!("{uuid}.json"))
}

pub(crate) fn write_meta_cache_in(state_dir: &Path, checkpoint: &Checkpoint) -> Result<()> {
    write_json(&meta_cache_path_in(state_dir, &checkpoint.uuid), checkpoint)
}

pub(crate) fn read_meta_cache_in(state_dir: &Path, uuid: &str) -> Result<Option<Checkpoint>> {
    let path = meta_cache_path_in(state_dir, uuid);
    if !path.exists() {
        return Ok(None);
    }
    let text = file::read_to_string(&path)?;
    let checkpoint =
        serde_json::from_str(&text).wrap_err_with(|| format!("reading {}", display_path(&path)))?;
    Ok(Some(checkpoint))
}

pub(crate) fn remove_meta_cache_in(state_dir: &Path, uuid: &str) {
    let _ = std::fs::remove_file(meta_cache_path_in(state_dir, uuid));
}

pub(crate) fn pending_path_in(state_dir: &Path, uuid: &str) -> PathBuf {
    pending_dir_in(state_dir).join(format!("{uuid}.json"))
}

/// The pending outcome of an operation in progress: the record as written so
/// far plus the git objects its journal already stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub id: u64,
    pub checkpoint: Checkpoint,
    /// sha256 -> blob oid for journal content already written to the
    /// repository (unreferenced until the wrapper commit exists).
    #[serde(default)]
    pub blobs: BTreeMap<String, String>,
}

pub(crate) fn write_pending_in(state_dir: &Path, pending: &Pending) -> Result<()> {
    write_json(
        &pending_path_in(state_dir, &pending.checkpoint.uuid),
        pending,
    )
}

/// Every pending record with the file holding it.
pub(crate) fn list_pending_in(state_dir: &Path) -> Result<Vec<(PathBuf, Pending)>> {
    let mut pending = vec![];
    for path in file::ls(&pending_dir_in(state_dir)).unwrap_or_default() {
        if path.extension().is_some_and(|ext| ext == "json") {
            let text = file::read_to_string(&path)?;
            match serde_json::from_str::<Pending>(&text) {
                Ok(record) => pending.push((path, record)),
                Err(err) => {
                    warn!(
                        "history: removing unreadable {}: {err}",
                        display_path(&path)
                    );
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    pending.sort_by_key(|(_, record)| record.id);
    Ok(pending)
}

pub(crate) fn remove_pending_in(state_dir: &Path, uuid: &str) {
    let _ = std::fs::remove_file(pending_path_in(state_dir, uuid));
}

/// One promoted (explicitly saved) version of a manual-save path, mirrored
/// from `promotions.json` in the promotion chain.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SavedRecord {
    /// Path inside the snapshot tree (`home/.config/app/state.json`).
    pub tree_path: String,
    /// The promotion commit that made this version durable.
    pub promotion: String,
    pub promoted_at: String,
    pub trigger: Trigger,
    /// The checkpoint recorded together with the promotion.
    pub checkpoint: String,
}

pub(crate) fn saved_index_in(state_dir: &Path) -> PathBuf {
    index_dir_in(state_dir).join("saved.json")
}

pub(crate) fn read_saved_index_in(state_dir: &Path) -> Result<BTreeMap<String, SavedRecord>> {
    let path = saved_index_in(state_dir);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = file::read_to_string(&path)?;
    serde_json::from_str(&text).wrap_err_with(|| format!("reading {}", display_path(&path)))
}

pub(crate) fn write_saved_index_in(
    state_dir: &Path,
    saved: &BTreeMap<String, SavedRecord>,
) -> Result<()> {
    write_json(&saved_index_in(state_dir), saved)
}

/// The marker of an operation in progress, next to the lock that owns it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct OperationMarker {
    pub uuid: String,
    pub kind: OperationKind,
    pub started_at: String,
    pub command: String,
}

pub(crate) fn write_marker_in(state_dir: &Path, marker: &OperationMarker) -> Result<()> {
    write_json(&operation_marker_in(state_dir), marker)
}

pub(crate) fn read_marker_in(state_dir: &Path) -> Result<Option<OperationMarker>> {
    let path = operation_marker_in(state_dir);
    if !path.exists() {
        return Ok(None);
    }
    let text = file::read_to_string(&path)?;
    let marker =
        serde_json::from_str(&text).wrap_err_with(|| format!("reading {}", display_path(&path)))?;
    Ok(Some(marker))
}

pub(crate) fn remove_marker_in(state_dir: &Path) {
    let _ = std::fs::remove_file(operation_marker_in(state_dir));
}

/// Loads every indexed checkpoint, oldest first.
pub(crate) fn list_in(state_dir: &Path) -> Result<Vec<Entry>> {
    let index = load_index_in(state_dir)?;
    let mut entries = Vec::with_capacity(index.entries.len());
    for line in &index.entries {
        match read_meta_cache_in(state_dir, &line.uuid)? {
            Some(checkpoint) => entries.push(Entry {
                id: line.id,
                commit: line.commit.clone(),
                checkpoint,
            }),
            None => warn!(
                "history: checkpoint {} ({}) has no cached record; run any history command with a usable git to rebuild the index",
                line.id, line.uuid
            ),
        }
    }
    Ok(entries)
}

/// Turns `ID`, `latest`, `latest~N`, or a uuid prefix into a checkpoint id,
/// resolved against `entries` (oldest first).
pub(crate) fn resolve_ref(spec: &str, entries: &[Entry]) -> Result<u64> {
    if let Some(rest) = spec.strip_prefix("latest") {
        let back: usize = match rest {
            "" => 0,
            _ => rest
                .strip_prefix('~')
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| eyre!("invalid checkpoint reference {spec:?}"))?,
        };
        return entries
            .iter()
            .rev()
            .nth(back)
            .map(|entry| entry.id)
            .ok_or_else(|| {
                eyre!(
                    "no history checkpoint {spec} (only {} recorded)",
                    entries.len()
                )
            });
    }
    if let Ok(id) = spec.parse::<u64>() {
        if entries.iter().any(|entry| entry.id == id) {
            return Ok(id);
        }
        bail!("no history checkpoint {id}");
    }
    let matches: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.checkpoint.uuid.starts_with(spec))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.id),
        [] => bail!("no history checkpoint matches {spec:?}"),
        _ => bail!("{spec:?} matches more than one checkpoint; use a longer prefix"),
    }
}

pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn new_uuid() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    file::write_atomic(path, text).wrap_err_with(|| format!("writing {}", display_path(path)))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
