//! The on-disk history store under `$MISE_STATE_DIR/history/`.
//!
//! The bare repository `repo.git` is the complete representation: every
//! tracked-file version is an ordinary commit whose tree holds the files.
//! Recovery journals do not belong in publishable commits. Everything else
//! in the directory is a rebuildable index or machine-local bookkeeping. Every
//! function takes the state directory explicitly (`*_in`) so tests can point
//! it at a temporary directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};

use super::journal::JournalEntry;
use crate::file::{self, display_path};

pub(crate) const SCHEMA_VERSION: u32 = 1;

/// The state directory the store lives under.
pub(crate) fn state_dir() -> PathBuf {
    crate::dirs::STATE.to_path_buf()
}

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
pub(crate) fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !dir.is_dir() {
        if let Some(parent) = dir.parent() {
            file::create_dir_all(parent)?;
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists && dir.is_dir() {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .wrap_err_with(|| format!("creating {}", display_path(dir)))?;
    }
    let mode = std::fs::metadata(dir)?.permissions().mode() & 0o777;
    if mode != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .wrap_err_with(|| format!("restricting {}", display_path(dir)))?;
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
        SetFileSecurityW,
    };
    use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

    if let Some(parent) = dir.parent() {
        file::create_dir_all(parent)?;
    }
    // Protected, inheritable owner-only access. Supplying this at creation
    // avoids exposing snapshots through a permissive parent ACL, even briefly.
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)\0".encode_utf16().collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: both pointers are valid for the call; Windows allocates the
    // descriptor, released with LocalFree below on every path.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| -> Result<()> {
        let mut path: Vec<u16> = dir.as_os_str().encode_wide().collect();
        if path.contains(&0) {
            eyre::bail!("history directory contains a NUL character");
        }
        path.push(0);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        if !dir.is_dir()
            // SAFETY: path is NUL-terminated and attributes/descriptor remain live.
            && unsafe { CreateDirectoryW(path.as_ptr(), &attributes) } == 0
            && !dir.is_dir()
        {
            return Err(std::io::Error::last_os_error().into());
        }
        // Also tighten an existing directory instead of trusting inherited ACLs.
        // SAFETY: path and descriptor are valid until the call returns.
        if unsafe {
            SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    })();
    // SAFETY: ConvertStringSecurityDescriptorToSecurityDescriptorW allocated it.
    unsafe {
        LocalFree(descriptor);
    }
    result.wrap_err_with(|| format!("restricting {}", display_path(dir)))
}

/// Display context for an in-progress local operation, never durable history.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Machine {
    pub id: String,
    pub name: String,
}

pub(crate) fn machine() -> Machine {
    Machine {
        id: "local".into(),
        name: hostname(),
    }
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
    CaptureBefore,
    Capture,
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
            Self::CaptureBefore => "capture-before",
            Self::Capture => "capture",
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
            "capture-before" => Self::CaptureBefore,
            "capture" => Self::Capture,
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
    Capture,
    Bootstrap,
    Rollback,
    Undo,
    Apply,
    BootstrapRollback,
}

impl OperationKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Capture => "capture",
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
    /// Only tracked-file metadata may travel with the ordinary history.
    /// Recovery material and command invocation details remain local.
    pub(crate) fn for_commit(&self) -> CommitRecord {
        let portable = |path: &String| {
            let path = super::tracked::normalize_target(Path::new(path));
            let entry = self
                .tree
                .coverage
                .entries
                .iter()
                .filter(|entry| {
                    path.starts_with(super::tracked::normalize_target(Path::new(&entry.path)))
                })
                .max_by_key(|entry| entry.path.len())?;
            super::sync::layout::Roots::current().branch_path(&path, entry.variant.as_deref())
        };
        CommitRecord {
            trigger: self.trigger,
            description_source: self.description_source,
            task: self.task.clone(),
            labels: self.labels.clone(),
            pinned: self.pinned,
            omitted: self
                .tree
                .coverage
                .omitted
                .iter()
                .filter_map(|item| portable(&item.path))
                .collect(),
            incomplete: self
                .tree
                .coverage
                .incomplete
                .iter()
                .filter_map(|item| portable(&item.path))
                .collect(),
            operation: self.operation.as_ref().map(|op| CommitOperation {
                id: op.id.clone(),
                kind: op.kind,
                status: op.status,
                before: op.before.clone(),
                to: op.to.clone(),
                undoes: op.undoes.clone(),
                applied: op.applied.clone(),
                affected: op.affected.iter().filter_map(portable).collect(),
                sources: op
                    .sources
                    .iter()
                    .filter_map(|source| {
                        let paths: Vec<_> = source.paths.iter().filter_map(portable).collect();
                        (!paths.is_empty()).then(|| OperationSource {
                            checkpoint: source.checkpoint.clone(),
                            paths,
                        })
                    })
                    .collect(),
                directories: op.directories.iter().filter_map(portable).collect(),
                directory_modes: op
                    .directory_modes
                    .iter()
                    .filter_map(|(path, mode)| portable(path).map(|path| (path, *mode)))
                    .collect(),
            }),
        }
    }

    /// The trigger label plus the operation kind, for tables.
    pub(crate) fn kind_label(&self) -> String {
        self.trigger.as_str().to_string()
    }

    pub(crate) fn status(&self) -> Option<OperationStatus> {
        self.operation.as_ref().map(|operation| operation.status)
    }
}

/// Only information Git cannot derive belongs in a commit trailer. In
/// particular, no recovery data, invocation arguments, or machine paths.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommitRecord {
    pub trigger: Trigger,
    pub description_source: DescriptionSource,
    pub task: Option<String>,
    pub labels: Vec<String>,
    pub pinned: bool,
    /// Portable paths that this commit could not capture, not known absences.
    pub omitted: Vec<String>,
    /// Portable directory prefixes whose inventory was incomplete.
    pub incomplete: Vec<String>,
    pub operation: Option<CommitOperation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommitOperation {
    /// Stable identity reserved before the operation starts, independent of
    /// the outcome commit's object id and this machine's numeric index.
    pub id: String,
    pub kind: OperationKind,
    pub status: OperationStatus,
    pub before: Option<String>,
    pub to: Option<String>,
    pub undoes: Option<String>,
    pub applied: Option<String>,
    pub affected: Vec<String>,
    pub sources: Vec<OperationSource>,
    pub directories: Vec<String>,
    pub directory_modes: BTreeMap<String, u32>,
}

impl CommitOperation {
    pub(crate) fn localize(self) -> Result<Operation> {
        let local = |path: String| -> Result<String> {
            super::sync::layout::Roots::current()
                .locate(&path)
                .path()
                .map(display_path)
                .ok_or_else(|| eyre!("invalid operation path: {path}"))
        };
        Ok(Operation {
            id: self.id,
            kind: self.kind,
            status: self.status,
            command: self.kind.as_str().into(),
            argv: vec![],
            cwd: PathBuf::new(),
            user: None,
            finished_at: None,
            error: None,
            before: self.before,
            to: self.to,
            undoes: self.undoes,
            applied: self.applied,
            affected: self
                .affected
                .into_iter()
                .map(local)
                .collect::<Result<_>>()?,
            sources: self
                .sources
                .into_iter()
                .map(|source| {
                    Ok(OperationSource {
                        checkpoint: source.checkpoint,
                        paths: source.paths.into_iter().map(local).collect::<Result<_>>()?,
                    })
                })
                .collect::<Result<_>>()?,
            directories: self
                .directories
                .into_iter()
                .map(local)
                .collect::<Result<_>>()?,
            directory_modes: self
                .directory_modes
                .into_iter()
                .map(|(path, bits)| Ok((local(path)?, bits)))
                .collect::<Result<_>>()?,
            message: None,
            journal: vec![],
        })
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
    /// Unix permission bits (`0o600`, `0o700`, …) of captured regular files
    /// whose mode is not one git records (`0644`, `0755`), by display path,
    /// so a restore puts a private file back private.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modes: BTreeMap<String, u32>,
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
    #[serde(default)]
    pub encrypt: bool,
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
    pub id: String,
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
    /// Every checkpoint a rollback took content from, with the paths taken
    /// from each (a path-first rollback may resolve paths to different
    /// checkpoints; `to` names only the first).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<OperationSource>,
    /// Paths that were directories before this operation replaced or removed
    /// them; an empty directory leaves no trace in a snapshot, so undo
    /// recreates these explicitly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directories: Vec<String>,
    /// Modes of directories removed by this operation, including empty ones
    /// which cannot be represented by a Git snapshot tree.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub directory_modes: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The file changes the operation made, with their preimages.
    #[serde(default)]
    pub journal: Vec<JournalEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OperationSource {
    pub checkpoint: String,
    pub paths: Vec<String>,
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

/// A description or label edit appended as metadata in ordinary Git history.
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

pub(crate) fn pending_path_in(state_dir: &Path, uuid: &str) -> PathBuf {
    pending_dir_in(state_dir).join(format!("{uuid}.json"))
}

/// The pending outcome of an operation in progress: the record as written so
/// far plus the git objects its journal already stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub id: u64,
    pub checkpoint: Checkpoint,
    pub recovery: RecoveryState,
    /// sha256 -> blob oid for journal content already written to the
    /// repository (unreferenced until the wrapper commit exists).
    #[serde(default)]
    pub blobs: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryState {
    /// An interrupted batch or an explicitly incomplete all-or-nothing apply.
    Pending,
    /// A normally returned command; retain its completed phases.
    UnfinishedWrites,
    /// All writes completed or were safely recovered; finalize history only.
    Finished,
}

pub(crate) fn write_pending_in(state_dir: &Path, pending: &Pending) -> Result<()> {
    write_json(
        &pending_path_in(state_dir, &pending.checkpoint.uuid),
        pending,
    )
}

/// Every pending record with the file holding it. Unreadable records stop
/// recovery; never hide them or discard possibly referenced preimages.
pub(crate) fn list_pending_in(state_dir: &Path) -> Result<Vec<(PathBuf, Pending)>> {
    let mut pending = vec![];
    let directory = match std::fs::read_dir(pending_dir_in(state_dir)) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(pending),
        Err(error) => return Err(error.into()),
    };
    for entry in directory {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(err).wrap_err_with(|| format!("reading {}", display_path(&path)));
                }
            };
            let record = serde_json::from_str::<Pending>(&text).wrap_err_with(|| format!(
                "cannot read recovery record {}; repair it before retrying; recovery data was retained", display_path(&path)
            ))?;
            pending.push((path, record));
        }
    }
    pending.sort_by_key(|(_, record)| record.id);
    Ok(pending)
}

/// Read-only status lookup. A record removed concurrently is skipped, but
/// corruption must remain visible rather than reporting healthy empty state.
pub(crate) fn peek_pending_in(state_dir: &Path) -> Result<Vec<(PathBuf, Pending)>> {
    list_pending_in(state_dir)
}

pub(crate) fn remove_pending_in(state_dir: &Path, uuid: &str) {
    let _ = std::fs::remove_file(pending_path_in(state_dir, uuid));
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
    let numeric_id = spec.parse::<u64>().ok();
    let matches: Vec<&Entry> = entries
        .iter()
        .filter(|entry| Some(entry.id) == numeric_id || entry.checkpoint.uuid.starts_with(spec))
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

#[cfg(all(test, unix))]
mod private_directory_tests {
    #[test]
    fn concurrent_creation_preserves_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("history");
        let barrier = std::sync::Barrier::new(16);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    barrier.wait();
                    super::create_private_dir(&path).unwrap();
                });
            }
        });
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
