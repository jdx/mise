//! On-disk generation records under `$MISE_STATE_DIR/bootstrap/generations/`.
//!
//! Every function takes the state directory explicitly (`*_in`) so tests can
//! point it at a temporary directory; the wrappers without a suffix use
//! [`crate::dirs::STATE`].

use std::path::{Path, PathBuf};

use eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize};

use super::journal::JournalEntry;
use super::shadow::ShadowRepo;
use crate::file::{self, display_path};

pub(crate) const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GenerationStatus {
    /// The run that owns this generation has not finished (or died).
    Pending,
    Completed,
    Failed,
}

impl GenerationStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Generation {
    pub schema_version: u32,
    pub id: u64,
    pub status: GenerationStatus,
    /// RFC 3339, UTC.
    pub created_at: String,
    pub finished_at: Option<String>,
    /// The mise command line without `argv[0]`, e.g. `bootstrap --yes`.
    pub command: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub user: Option<String>,
    pub mise_version: String,
    pub pid: u32,
    /// The newest generation that existed when this one began.
    pub parent: Option<u64>,
    /// Set on generations recorded by `mise bootstrap rollback`.
    pub rollback_of: Option<u64>,
    pub snapshot: SnapshotInfo,
    pub lockfile: Option<LockfileSnapshot>,
    #[serde(default)]
    pub journal: Vec<JournalEntry>,
    pub summary: Option<Summary>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SnapshotInfo {
    /// False when no content snapshot could be taken (no usable `git`).
    pub available: bool,
    pub reason: Option<String>,
    pub repo: PathBuf,
    pub before: Option<Snapshot>,
    pub after: Option<Snapshot>,
    /// Whether `after` captured the same trees as `before`.
    pub unchanged: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub commit: String,
    pub tree: String,
    pub taken_at: String,
    pub roots: Vec<RootRecord>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct RootRecord {
    pub label: String,
    pub path: PathBuf,
    /// Tree id of this root inside the snapshot commit; None when skipped
    /// or represented by another root.
    pub tree: Option<String>,
    pub files: u64,
    pub bytes: u64,
    /// `missing`, `refused`, or `too-large`.
    pub skipped: Option<String>,
    /// The same directory as this earlier root.
    pub alias_of: Option<String>,
    /// Inside this earlier root, at `subpath`.
    pub contained_in: Option<String>,
    pub subpath: Option<PathBuf>,
    /// The user's own checkout holding this root, if any. Never modified.
    pub vcs: Option<VcsInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct VcsInfo {
    pub root: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct LockfileSnapshot {
    pub path: PathBuf,
    pub sha256: String,
    /// Blob id in the shadow repository.
    pub blob: Option<String>,
    /// Sidecar copy used when the shadow repository is unavailable.
    pub file: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Summary {
    /// Bootstrap parts the run executed.
    pub parts: Vec<String>,
    pub message: Option<String>,
}

pub(crate) fn store_dir_in(state_dir: &Path) -> PathBuf {
    state_dir.join("bootstrap")
}

pub(crate) fn records_dir_in(state_dir: &Path) -> PathBuf {
    store_dir_in(state_dir).join("generations")
}

pub(crate) fn record_path_in(state_dir: &Path, id: u64) -> PathBuf {
    records_dir_in(state_dir).join(format!("{id:06}.json"))
}

pub(crate) fn sidecar_lockfile_path_in(state_dir: &Path, id: u64) -> PathBuf {
    records_dir_in(state_dir).join(format!("{id:06}.mise.lock"))
}

/// Creates the store directories. The `bootstrap` directory is private to
/// the user because snapshots can hold secrets kept in the config directory.
pub(crate) fn ensure_store_dir_in(state_dir: &Path) -> Result<()> {
    let store = store_dir_in(state_dir);
    if !store.is_dir() {
        file::create_dir_all(state_dir)?;
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&store) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err).map_err(|e| eyre!("{}: {e}", display_path(&store))),
        }
    }
    file::create_dir_all(records_dir_in(state_dir))
}

pub(crate) fn write_in(state_dir: &Path, generation: &Generation) -> Result<()> {
    let path = record_path_in(state_dir, generation.id);
    let body = serde_json::to_vec_pretty(generation)?;
    file::write_atomic(&path, body)
}

fn id_from_path(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".json")?;
    stem.parse().ok()
}

/// Ids of every record on disk, ascending, whether or not it parses.
fn ids_in(state_dir: &Path) -> Result<Vec<u64>> {
    let dir = records_dir_in(state_dir);
    let mut ids = vec![];
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
        Err(err) => return Err(err).map_err(|e| eyre!("{}: {e}", display_path(&dir))),
    };
    for entry in entries {
        if let Some(id) = id_from_path(&entry?.path()) {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

pub(crate) fn next_id_in(state_dir: &Path) -> Result<u64> {
    Ok(ids_in(state_dir)?.last().map_or(1, |id| id + 1))
}

pub(crate) fn try_load_in(state_dir: &Path, id: u64) -> Result<Option<Generation>> {
    let path = record_path_in(state_dir, id);
    let body = match std::fs::read(&path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).map_err(|e| eyre!("{}: {e}", display_path(&path))),
    };
    let generation: Generation = serde_json::from_slice(&body)
        .map_err(|e| eyre!("{}: invalid generation record: {e}", display_path(&path)))?;
    Ok(Some(generation))
}

pub(crate) fn load_in(state_dir: &Path, id: u64) -> Result<Generation> {
    try_load_in(state_dir, id)?.ok_or_else(|| eyre!("no bootstrap generation {id}"))
}

/// Every readable generation, ascending by id. Unreadable records are
/// skipped with a warning so one corrupt file does not hide the rest.
pub(crate) fn list_in(state_dir: &Path) -> Result<Vec<Generation>> {
    let mut generations = vec![];
    for id in ids_in(state_dir)? {
        match try_load_in(state_dir, id) {
            Ok(Some(generation)) => generations.push(generation),
            Ok(None) => {}
            Err(err) => warn!("bootstrap generations: {err}"),
        }
    }
    Ok(generations)
}

pub(crate) fn remove_in(state_dir: &Path, id: u64) -> Result<()> {
    for path in [
        record_path_in(state_dir, id),
        sidecar_lockfile_path_in(state_dir, id),
    ] {
        if path.exists() {
            file::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Resolves `42`, `latest`, or `latest~N` against the recorded generations.
pub(crate) fn resolve_id(spec: &str, generations: &[Generation]) -> Result<u64> {
    if let Ok(id) = spec.parse::<u64>() {
        return Ok(id);
    }
    let back = match spec {
        "latest" => 0,
        _ => match spec.strip_prefix("latest~") {
            Some(n) => n
                .parse::<usize>()
                .map_err(|_| eyre!("invalid generation: {spec}"))?,
            None => bail!("invalid generation: {spec} (expected an id, `latest`, or `latest~N`)"),
        },
    };
    generations
        .iter()
        .rev()
        .nth(back)
        .map(|generation| generation.id)
        .ok_or_else(|| eyre!("no bootstrap generation matches {spec}"))
}

/// Deletes the oldest finished generations beyond `keep`, never touching
/// pending ones, the newest completed one, or anything in `protect`.
/// Returns the pruned ids. `keep == 0` disables pruning.
pub(crate) fn prune_in(
    state_dir: &Path,
    shadow: Option<&ShadowRepo>,
    keep: usize,
    protect: &[u64],
) -> Result<Vec<u64>> {
    if keep == 0 {
        return Ok(vec![]);
    }
    let generations = list_in(state_dir)?;
    let newest_completed = generations
        .iter()
        .rev()
        .find(|generation| generation.status == GenerationStatus::Completed)
        .map(|generation| generation.id);
    let mut protected: Vec<u64> = protect.to_vec();
    protected.extend(newest_completed);
    protected.extend(generations.iter().filter_map(|g| g.rollback_of));
    let candidates: Vec<u64> = generations
        .iter()
        .filter(|generation| generation.status != GenerationStatus::Pending)
        .map(|generation| generation.id)
        .filter(|id| !protected.contains(id))
        .collect();
    let retained = generations.len();
    let excess = retained.saturating_sub(keep);
    let mut pruned = vec![];
    for id in candidates.into_iter().take(excess) {
        remove_in(state_dir, id)?;
        if let Some(shadow) = shadow {
            shadow.delete_refs(id);
        }
        pruned.push(id);
    }
    Ok(pruned)
}

pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(id: u64, status: GenerationStatus) -> Generation {
        Generation {
            schema_version: SCHEMA_VERSION,
            id,
            status,
            created_at: now_rfc3339(),
            finished_at: None,
            command: "bootstrap".into(),
            argv: vec!["bootstrap".into()],
            cwd: PathBuf::from("/"),
            user: None,
            mise_version: "test".into(),
            pid: 1,
            parent: None,
            rollback_of: None,
            snapshot: SnapshotInfo {
                available: false,
                reason: None,
                repo: PathBuf::from("/nonexistent"),
                before: None,
                after: None,
                unchanged: None,
            },
            lockfile: None,
            journal: vec![],
            summary: None,
            error: None,
        }
    }

    #[test]
    fn records_round_trip_and_list_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path();
        ensure_store_dir_in(state).unwrap();
        assert_eq!(next_id_in(state).unwrap(), 1);
        for id in [3, 1, 2] {
            write_in(state, &generation(id, GenerationStatus::Completed)).unwrap();
        }
        assert_eq!(next_id_in(state).unwrap(), 4);
        let listed = list_in(state)
            .unwrap()
            .into_iter()
            .map(|g| g.id)
            .collect::<Vec<_>>();
        assert_eq!(listed, vec![1, 2, 3]);
        assert!(try_load_in(state, 9).unwrap().is_none());
        assert_eq!(load_in(state, 2).unwrap().id, 2);
    }

    #[test]
    fn corrupt_records_are_skipped_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path();
        ensure_store_dir_in(state).unwrap();
        write_in(state, &generation(1, GenerationStatus::Completed)).unwrap();
        std::fs::write(record_path_in(state, 2), b"{ not json").unwrap();
        let listed = list_in(state).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(next_id_in(state).unwrap(), 3);
    }

    #[test]
    fn unknown_journal_kinds_deserialize() {
        let body = r#"{"kind":"from_the_future","anything":1}"#;
        let entry: JournalEntry = serde_json::from_str(body).unwrap();
        assert!(matches!(entry, JournalEntry::Unknown));
    }

    #[test]
    fn resolve_id_handles_latest_and_offsets() {
        let generations = [1, 2, 5]
            .into_iter()
            .map(|id| generation(id, GenerationStatus::Completed))
            .collect::<Vec<_>>();
        assert_eq!(resolve_id("5", &generations).unwrap(), 5);
        assert_eq!(resolve_id("latest", &generations).unwrap(), 5);
        assert_eq!(resolve_id("latest~1", &generations).unwrap(), 2);
        assert_eq!(resolve_id("latest~2", &generations).unwrap(), 1);
        assert!(resolve_id("latest~3", &generations).is_err());
        assert!(resolve_id("nope", &generations).is_err());
    }

    #[test]
    fn prune_keeps_pending_newest_and_protected() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path();
        ensure_store_dir_in(state).unwrap();
        write_in(state, &generation(1, GenerationStatus::Completed)).unwrap();
        write_in(state, &generation(2, GenerationStatus::Failed)).unwrap();
        write_in(state, &generation(3, GenerationStatus::Pending)).unwrap();
        write_in(state, &generation(4, GenerationStatus::Completed)).unwrap();
        write_in(state, &generation(5, GenerationStatus::Completed)).unwrap();
        // keep 2 of 5: candidates are 1, 2, 4 (3 pending, 5 newest completed);
        // 4 is protected, so 1 and 2 go and the excess of 3 is only partly met.
        let pruned = prune_in(state, None, 2, &[4]).unwrap();
        assert_eq!(pruned, vec![1, 2]);
        let remaining = list_in(state)
            .unwrap()
            .into_iter()
            .map(|g| g.id)
            .collect::<Vec<_>>();
        assert_eq!(remaining, vec![3, 4, 5]);
        assert!(prune_in(state, None, 0, &[]).unwrap().is_empty());
    }
}
