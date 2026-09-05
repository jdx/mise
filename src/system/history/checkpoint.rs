//! Capturing checkpoints: the one entry point every capture goes through.
//!
//! Content is deduplicated, records are not: an automatic capture whose
//! snapshot tree and coverage equal the newest checkpoint's records nothing,
//! while a draft carrying metadata of its own (a description, a label, an
//! operation) always writes a new wrapper commit, reusing the snapshot tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};

use std::collections::BTreeSet;

use super::shadow::{self, HistoryRepo, Overlay};
use super::store::{
    self, Annotation, Changes, Checkpoint, DescriptionSource, Entry, Index, IndexEntry, Machine,
    Operation, SavedRecord, TreeInfo, Trigger,
};
use super::tracked::{TrackedEntry, TrackedSet, display_to_tree_path, tree_path_to_display};
use crate::file::display_path;
use crate::lock_file::LockFile;

/// The most paths a computed description names before `+N more`.
const DESCRIPTION_PATHS: usize = 6;
const DESCRIPTION_MAX: usize = 200;
/// The most paths a `changes` record lists.
const CHANGES_MAX: usize = 2000;

/// What a caller wants captured.
#[derive(Clone, Debug, Default)]
pub(crate) struct Draft {
    pub trigger: Option<Trigger>,
    /// A caller-supplied description; the computed one is kept as `summary`.
    pub description: Option<String>,
    pub description_source: Option<DescriptionSource>,
    pub task: Option<String>,
    pub labels: Vec<String>,
    /// Live contents captured for a `*-before` checkpoint; never promoted.
    pub protective: bool,
    pub operation: Option<Operation>,
    /// Journal blobs already stored in the repository: sha256 -> oid.
    pub blobs: BTreeMap<String, String>,
    /// A uuid reserved when the operation began, so the marker and the
    /// pending record name the outcome before it exists.
    pub uuid: Option<String>,
    /// Paths named explicitly: manual-save entries covering them are read
    /// live and promoted, becoming their new saved version.
    pub explicit_paths: Vec<PathBuf>,
}

impl Draft {
    pub(crate) fn new(trigger: Trigger) -> Self {
        Self {
            trigger: Some(trigger),
            ..Default::default()
        }
    }

    fn trigger(&self) -> Trigger {
        self.trigger.unwrap_or(Trigger::Edit)
    }

    /// Whether the draft carries something a deduplicated capture would lose.
    fn has_metadata(&self) -> bool {
        !self.trigger().is_automatic()
            || self.description.is_some()
            || self.task.is_some()
            || !self.labels.is_empty()
            || self.protective
            || self.operation.is_some()
    }
}

#[derive(Debug)]
pub(crate) enum Outcome {
    Created(Box<Entry>),
    /// The tracked state equals the newest checkpoint's; nothing recorded.
    Unchanged,
    /// No capture could be taken (no usable git); the reason.
    Unavailable(String),
}

/// An open history store.
#[derive(Debug)]
pub(crate) struct Store {
    state_dir: PathBuf,
    repo: Option<HistoryRepo>,
    unavailable: Option<String>,
    machine: Machine,
}

impl Store {
    pub(crate) fn open_in(state_dir: &Path) -> Result<Self> {
        store::ensure_store_dir_in(state_dir)?;
        let machine = store::machine_in(state_dir)?;
        let (repo, unavailable) = match HistoryRepo::open_or_init_in(state_dir) {
            Ok(Some(repo)) => (Some(repo), None),
            Ok(None) => (None, Some(shadow::unavailable_reason())),
            Err(err) => (None, Some(format!("{err:#}"))),
        };
        let store = Self {
            state_dir: state_dir.to_path_buf(),
            repo,
            unavailable,
            machine,
        };
        // the index is a cache of the repository: rebuild it when it is
        // missing but checkpoints exist
        if !store::index_exists_in(state_dir)
            && let Some(repo) = &store.repo
            && !repo.checkpoint_refs()?.is_empty()
        {
            info!("history: rebuilding the checkpoint index from the repository");
            store.rebuild_index()?;
        }
        // the saved index mirrors promotions.json at the head of the
        // promotion chain
        if !store::saved_index_in(state_dir).exists()
            && let Some(repo) = &store.repo
            && let Some(head) = repo.promoted_head()?
        {
            store.rebuild_saved_index(repo, &head)?;
        }
        Ok(store)
    }

    fn rebuild_saved_index(&self, repo: &HistoryRepo, head: &str) -> Result<()> {
        let Some((_, oid)) = repo.object_at(head, "promotions.json")? else {
            return Ok(());
        };
        let bytes = repo.cat_object(&oid)?;
        let saved: std::collections::BTreeMap<String, SavedRecord> =
            serde_json::from_slice(&bytes).wrap_err("reading promotions.json")?;
        store::write_saved_index_in(&self.state_dir, &saved)
    }

    pub(crate) fn open() -> Result<Self> {
        Self::open_in(&crate::dirs::STATE)
    }

    pub(crate) fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub(crate) fn repo(&self) -> Option<&HistoryRepo> {
        self.repo.as_ref()
    }

    pub(crate) fn machine(&self) -> &Machine {
        &self.machine
    }

    /// Why no content can be captured, if git is unusable.
    pub(crate) fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }

    /// Serializes captures, index writes, and pruning.
    pub(crate) fn lock(&self) -> Result<fslock::LockFile> {
        LockFile::new(&store::store_lock_path_in(&self.state_dir))
            .with_callback(|path| {
                debug!("waiting for the history store lock {}", display_path(path));
            })
            .lock()
    }

    pub(crate) fn list(&self) -> Result<Vec<Entry>> {
        store::list_in(&self.state_dir)
    }

    /// Reserves the next checkpoint id.
    pub(crate) fn reserve_id(&self) -> Result<u64> {
        let mut index = store::load_index_in(&self.state_dir)?;
        let id = index.next_id.max(1);
        index.next_id = id + 1;
        store::write_index_in(&self.state_dir, &index)?;
        Ok(id)
    }

    /// Captures the tracked set into a checkpoint. Takes the store lock.
    pub(crate) fn attempt(&self, tracked: &TrackedSet, draft: Draft) -> Result<Outcome> {
        let _lock = self.lock()?;
        self.attempt_locked(tracked, draft, None)
    }

    /// Like [`attempt`], with a reserved id and the lock already held.
    pub(crate) fn attempt_locked(
        &self,
        tracked: &TrackedSet,
        draft: Draft,
        reserved_id: Option<u64>,
    ) -> Result<Outcome> {
        let mut index = store::load_index_in(&self.state_dir)?;
        let previous = index.newest().cloned();
        let previous_tree = previous
            .as_ref()
            .and_then(|entry| {
                store::read_meta_cache_in(&self.state_dir, &entry.uuid)
                    .ok()
                    .flatten()
            })
            .and_then(|checkpoint| {
                checkpoint
                    .tree
                    .snapshot
                    .clone()
                    .map(|tree| (checkpoint, tree))
            });
        let uuid = draft.uuid.clone().unwrap_or_else(store::new_uuid);
        let mut walk = tracked.walk()?;
        for warning in &walk.warnings {
            warn!("history: {warning}");
        }
        // manual-save entries: carried forward from their promoted version
        // unless named explicitly (promoted) or captured protectively
        let promoted: std::collections::BTreeSet<String> =
            store::read_saved_index_in(&self.state_dir)?
                .into_keys()
                .collect();
        let manual = manual_plan(&walk, &draft, &promoted);
        if !manual.carry.is_empty() {
            let carried: BTreeSet<usize> = manual.carry.iter().copied().collect();
            let dropped: Vec<PathBuf> = walk
                .files
                .iter()
                .filter(|(_, (owner, _))| carried.contains(owner))
                .map(|(path, _)| path.clone())
                .collect();
            for path in &dropped {
                walk.files.remove(path);
            }
            for root in &mut walk.roots {
                root.files
                    .retain(|rel| !dropped.contains(&root.path.join(rel)));
            }
        }
        let mut coverage = tracked.coverage(&walk);
        let promoted_head = match &self.repo {
            Some(repo) => repo.promoted_head()?,
            None => None,
        };
        let mut promotion = None;
        let (snapshot, roots, available, reason) = match &self.repo {
            Some(repo) => match repo.capture(&walk.roots) {
                Ok(result) => {
                    for warning in &result.warnings {
                        warn!("history: {warning}");
                    }
                    let composed = self.compose_manual(
                        repo,
                        &result.tree,
                        ManualContext {
                            entries: &walk.entries,
                            manual: &manual,
                            promoted_head: promoted_head.as_deref(),
                            draft: &draft,
                            uuid: &uuid,
                        },
                        &mut promotion,
                    )?;
                    (Some(composed), result.roots, true, None)
                }
                Err(err) => {
                    warn!("history: snapshot failed: {err:#}");
                    (None, vec![], false, Some(format!("{err:#}")))
                }
            },
            None => (None, vec![], false, self.unavailable.clone()),
        };
        for (index, entry) in walk.entries.iter().enumerate() {
            if entry.policy.autosave {
                continue;
            }
            let Some(record) = coverage
                .entries
                .iter_mut()
                .find(|record| record.path == entry.display())
            else {
                continue;
            };
            if manual.promote.contains(&index) {
                record.state = "live".into();
                record.promotion = promotion.clone().or_else(|| promoted_head.clone());
            } else if draft.protective {
                record.state = "protective".into();
                record.promotion = promoted_head.clone();
            } else {
                record.state = "saved".into();
                record.promotion = promoted_head.clone();
            }
        }
        if !draft.has_metadata()
            && let Some((previous_checkpoint, tree)) = &previous_tree
            && snapshot.as_deref() == Some(tree.as_str())
            && previous_checkpoint.tree.coverage == coverage
        {
            debug!(
                "history: nothing changed since checkpoint {}",
                previous_checkpoint.uuid
            );
            return Ok(Outcome::Unchanged);
        }
        if !draft.has_metadata() && snapshot.is_none() {
            return Ok(Outcome::Unavailable(
                reason.unwrap_or_else(shadow::unavailable_reason),
            ));
        }
        let mut changes = match (&self.repo, &snapshot) {
            (Some(repo), Some(tree)) => {
                let since = previous_tree
                    .as_ref()
                    .map(|(checkpoint, _)| checkpoint.uuid.clone());
                let from = previous_tree.as_ref().map(|(_, tree)| tree.as_str());
                changes_from(repo, from, tree, since)?
            }
            _ => Changes::default(),
        };
        // a manual-save entry carried forward holds its saved version by
        // definition: a difference against a protective capture of its live
        // contents is not a change this checkpoint made
        if !manual.carry.is_empty() {
            let carried: Vec<String> = manual
                .carry
                .iter()
                .map(|index| walk.entries[*index].display())
                .collect();
            let keep = |path: &String| {
                !carried.iter().any(|entry| {
                    path == entry
                        || path
                            .strip_prefix(entry.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))
                })
            };
            changes.added.retain(keep);
            changes.modified.retain(keep);
            changes.removed.retain(keep);
        }
        let total_files: u64 = roots.iter().map(|root| root.files).sum();
        let summary = describe(&draft, &changes, previous_tree.is_some(), total_files);
        let (description, description_source) = match &draft.description {
            Some(text) => (
                text.clone(),
                draft.description_source.unwrap_or(DescriptionSource::User),
            ),
            None => (summary.clone(), DescriptionSource::Computed),
        };
        let checkpoint = Checkpoint {
            schema_version: store::SCHEMA_VERSION,
            uuid,
            machine: self.machine.clone(),
            created_at: store::now_rfc3339(),
            mise_version: crate::cli::version::VERSION_PLAIN.clone(),
            trigger: draft.trigger(),
            description,
            description_source,
            summary,
            task: draft.task.clone(),
            labels: draft.labels.clone(),
            pinned: false,
            tree: TreeInfo {
                snapshot: snapshot.clone(),
                available,
                reason,
                roots,
                coverage,
            },
            changes,
            operation: draft.operation.clone(),
        };
        let id = reserved_id.unwrap_or_else(|| {
            let id = index.next_id.max(1);
            index.next_id = id + 1;
            id
        });
        let commit = match &self.repo {
            Some(repo) => repo
                .write_checkpoint(snapshot.as_deref(), &checkpoint, &draft.blobs)
                .wrap_err("writing the checkpoint")?,
            None => String::new(),
        };
        store::write_meta_cache_in(&self.state_dir, &checkpoint)?;
        index.entries.push(IndexEntry {
            id,
            uuid: checkpoint.uuid.clone(),
            commit: commit.clone(),
            created_at: checkpoint.created_at.clone(),
            trigger: checkpoint.trigger,
        });
        if index.next_id <= id {
            index.next_id = id + 1;
        }
        store::write_index_in(&self.state_dir, &index)?;
        debug!(
            "history: recorded checkpoint {id} ({}): {}",
            checkpoint.trigger.as_str(),
            checkpoint.description
        );
        // cheap retention after every capture; the caller holds the lock
        if let Err(err) = super::retention::prune(self) {
            warn!("history: retention failed: {err:#}");
        }
        Ok(Outcome::Created(Box::new(Entry {
            id,
            commit,
            checkpoint,
        })))
    }

    /// Replaces manual-save entries in a captured tree with their promoted
    /// versions, and promotes the ones the draft names explicitly. A
    /// promotion is durable (a new commit on `refs/promoted`) before the
    /// checkpoint referencing it is written.
    fn compose_manual(
        &self,
        repo: &HistoryRepo,
        live_tree: &str,
        context: ManualContext<'_>,
        promotion: &mut Option<String>,
    ) -> Result<String> {
        let ManualContext {
            entries,
            manual,
            promoted_head,
            draft,
            uuid,
        } = context;
        let mut overlays = vec![];
        for index in &manual.carry {
            let tree_path = display_to_tree_path(&entries[*index].display());
            let object = match promoted_head {
                Some(head) => repo.object_at(head, &format!("promoted/{tree_path}"))?,
                None => None,
            };
            overlays.push(Overlay {
                path: tree_path,
                object,
            });
        }
        if !manual.promote.is_empty() {
            let mut saved = store::read_saved_index_in(&self.state_dir)?;
            let base = match promoted_head {
                Some(head) => repo.output_tree_of(head)?,
                None => repo.empty_object("tree")?,
            };
            let mut promoted_overlays = vec![];
            let now = store::now_rfc3339();
            for index in &manual.promote {
                let entry = &entries[*index];
                let tree_path = display_to_tree_path(&entry.display());
                let object = repo.object_at(live_tree, &tree_path)?;
                promoted_overlays.push(Overlay {
                    path: format!("promoted/{tree_path}"),
                    object: object.clone(),
                });
                match object {
                    Some(_) => {
                        saved.insert(
                            entry.display(),
                            SavedRecord {
                                tree_path,
                                promotion: String::new(),
                                promoted_at: now.clone(),
                                trigger: draft.trigger(),
                                checkpoint: uuid.to_string(),
                            },
                        );
                    }
                    None => {
                        saved.remove(&entry.display());
                    }
                }
            }
            let mut tree = repo.compose(&base, &promoted_overlays)?;
            // promotions.json mirrors the saved index inside the chain
            let listing = serde_json::to_string_pretty(&saved)?;
            let blob = repo.hash_blob(listing.as_bytes())?;
            tree = repo.compose(
                &tree,
                &[Overlay {
                    path: "promotions.json".into(),
                    object: Some(("100644".into(), blob)),
                }],
            )?;
            let names: Vec<String> = manual
                .promote
                .iter()
                .map(|index| entries[*index].display())
                .collect();
            let commit = repo.write_promotion(
                &tree,
                promoted_head,
                &format!("promote {}", names.join(", ")),
            )?;
            for record in saved.values_mut() {
                if record.promotion.is_empty() {
                    record.promotion = commit.clone();
                }
            }
            store::write_saved_index_in(&self.state_dir, &saved)?;
            *promotion = Some(commit);
        }
        repo.compose(live_tree, &overlays)
    }

    /// Removes a checkpoint: its ref, cached record, and index line.
    pub(crate) fn remove(&self, id: u64) -> Result<()> {
        let mut index = store::load_index_in(&self.state_dir)?;
        let Some(position) = index.entries.iter().position(|entry| entry.id == id) else {
            return Ok(());
        };
        let entry = index.entries.remove(position);
        store::write_index_in(&self.state_dir, &index)?;
        store::remove_meta_cache_in(&self.state_dir, &entry.uuid);
        if let Some(repo) = &self.repo {
            repo.delete_checkpoint_ref(&entry.uuid);
        }
        Ok(())
    }

    /// Rebuilds the index and cached records from `refs/checkpoints/*`.
    pub(crate) fn rebuild_index(&self) -> Result<Index> {
        let Some(repo) = &self.repo else {
            return store::load_index_in(&self.state_dir);
        };
        let _lock = self.lock()?;
        let existing = store::load_index_in(&self.state_dir)?;
        let mut entries = vec![];
        for (uuid, commit) in repo.checkpoint_refs()? {
            let mut checkpoint = repo.read_meta(&commit)?;
            if let Some(text) = repo.read_note(&commit)?
                && let Ok(annotation) = serde_json::from_str::<Annotation>(&text)
            {
                annotation.apply_to(&mut checkpoint);
            }
            store::write_meta_cache_in(&self.state_dir, &checkpoint)?;
            let id = existing.by_uuid(&uuid).map(|entry| entry.id);
            entries.push((id, checkpoint, commit));
        }
        entries.sort_by(|a, b| {
            a.1.created_at
                .cmp(&b.1.created_at)
                .then(a.1.uuid.cmp(&b.1.uuid))
        });
        // an outcome always follows the protective checkpoint it links to
        let mut ordered: Vec<(Option<u64>, Checkpoint, String)> = Vec::with_capacity(entries.len());
        let mut deferred: Vec<(Option<u64>, Checkpoint, String)> = vec![];
        for entry in entries {
            let before = entry.1.operation.as_ref().and_then(|op| op.before.clone());
            match before {
                Some(before) if !ordered.iter().any(|(_, c, _)| c.uuid == before) => {
                    deferred.push(entry)
                }
                _ => {
                    ordered.push(entry);
                    let placed: Vec<String> =
                        ordered.iter().map(|(_, c, _)| c.uuid.clone()).collect();
                    let (ready, rest): (Vec<_>, Vec<_>) =
                        deferred.drain(..).partition(|(_, c, _)| {
                            c.operation
                                .as_ref()
                                .and_then(|op| op.before.as_ref())
                                .is_some_and(|before| placed.contains(before))
                        });
                    ordered.extend(ready);
                    deferred = rest;
                }
            }
        }
        ordered.extend(deferred);
        let entries = ordered;
        let mut next_id = existing.next_id.max(1);
        let mut index = Index {
            next_id,
            entries: vec![],
        };
        for (id, checkpoint, commit) in entries {
            let id = id.unwrap_or_else(|| {
                let id = next_id;
                next_id += 1;
                id
            });
            index.entries.push(IndexEntry {
                id,
                uuid: checkpoint.uuid,
                commit,
                created_at: checkpoint.created_at,
                trigger: checkpoint.trigger,
            });
        }
        index.entries.sort_by_key(|entry| entry.id);
        index.next_id = next_id.max(index.entries.iter().map(|e| e.id + 1).max().unwrap_or(1));
        store::write_index_in(&self.state_dir, &index)?;
        Ok(index)
    }
}

/// Records an annotation durably (a git note on the wrapper commit) and
/// mirrors it into the cached record.
pub(crate) fn annotate(store: &Store, entry: &Entry, annotation: Annotation) -> Result<()> {
    let _lock = store.lock()?;
    if let Some(repo) = store.repo()
        && !entry.commit.is_empty()
    {
        let mut merged = match repo.read_note(&entry.commit)? {
            Some(text) => serde_json::from_str::<Annotation>(&text).unwrap_or_default(),
            None => Annotation::default(),
        };
        if annotation.description.is_some() {
            merged.description = annotation.description.clone();
            merged.description_source = annotation.description_source;
        }
        if annotation.pinned.is_some() {
            merged.pinned = annotation.pinned;
        }
        if annotation.labels.is_some() {
            merged.labels = annotation.labels.clone();
        }
        merged.updated_at = annotation.updated_at.clone();
        repo.write_note(&entry.commit, &serde_json::to_string_pretty(&merged)?)?;
    }
    let mut checkpoint = entry.checkpoint.clone();
    annotation.apply_to(&mut checkpoint);
    store::write_meta_cache_in(store.state_dir(), &checkpoint)
}

/// Everything composing manual-save entries into a snapshot needs.
struct ManualContext<'a> {
    entries: &'a [TrackedEntry],
    manual: &'a ManualPlan,
    promoted_head: Option<&'a str>,
    draft: &'a Draft,
    /// The uuid of the checkpoint being written.
    uuid: &'a str,
}

/// Which manual-save entries a capture carries forward and which it
/// promotes.
#[derive(Debug, Default)]
struct ManualPlan {
    carry: Vec<usize>,
    promote: Vec<usize>,
}

fn manual_plan(
    walk: &super::tracked::Walk,
    draft: &Draft,
    promoted: &std::collections::BTreeSet<String>,
) -> ManualPlan {
    let mut plan = ManualPlan::default();
    for (index, entry) in walk.entries.iter().enumerate() {
        if entry.policy.autosave {
            continue;
        }
        let named = draft
            .explicit_paths
            .iter()
            .any(|path| path.starts_with(&entry.path) || entry.path.starts_with(path));
        // an entry that was never promoted has no saved version to carry
        // forward: its first capture is its baseline
        let never_promoted = !promoted.contains(&entry.display());
        if named || (never_promoted && !draft.protective) {
            plan.promote.push(index);
        } else if !draft.protective {
            plan.carry.push(index);
        }
    }
    plan
}

fn changes_from(
    repo: &HistoryRepo,
    from: Option<&str>,
    to: &str,
    since: Option<String>,
) -> Result<Changes> {
    let mut changes = Changes {
        since,
        ..Default::default()
    };
    let mut count = 0usize;
    for change in repo.changes(from, to)? {
        count += 1;
        if count > CHANGES_MAX {
            changes.truncated = true;
            break;
        }
        let path = tree_path_to_display(&change.path);
        match change.status {
            'A' => changes.added.push(path),
            'D' => changes.removed.push(path),
            _ => changes.modified.push(path),
        }
    }
    Ok(changes)
}

/// The computed one-line description of a checkpoint.
pub(crate) fn describe(
    draft: &Draft,
    changes: &Changes,
    has_previous: bool,
    total_files: u64,
) -> String {
    if let Some(operation) = &draft.operation {
        let parts = if operation.parts.is_empty() {
            String::new()
        } else {
            format!(" {}", operation.parts.join(", "))
        };
        let what = match operation.message.as_deref() {
            Some(message) => format!(": {message}"),
            None if !changes.is_empty() => format!(": {}", describe_changes(changes)),
            None => String::new(),
        };
        return truncate(format!("{}{parts}{what}", operation.kind.as_str()));
    }
    if draft.protective {
        return truncate(format!(
            "before {}",
            draft.trigger().as_str().trim_end_matches("-before")
        ));
    }
    if !has_previous {
        return format!("initial checkpoint ({total_files} files)");
    }
    if changes.is_empty() {
        return "no file changes".to_string();
    }
    truncate(describe_changes(changes))
}

fn describe_changes(changes: &Changes) -> String {
    let mut budget = DESCRIPTION_PATHS;
    let mut groups = vec![];
    let mut more = 0usize;
    for (verb, paths) in [
        ("edited", &changes.modified),
        ("added", &changes.added),
        ("removed", &changes.removed),
    ] {
        if paths.is_empty() {
            continue;
        }
        let mut sorted: Vec<&String> = paths.iter().collect();
        sorted.sort();
        let take = budget.min(sorted.len());
        more += sorted.len() - take;
        if take == 0 {
            continue;
        }
        budget -= take;
        let names: Vec<String> = sorted[..take].iter().map(|path| short_path(path)).collect();
        groups.push(format!("{verb} {}", names.join(", ")));
    }
    let mut text = groups.join("; ");
    if more > 0 {
        text.push_str(&format!(" +{more} more"));
    }
    text
}

/// `~/.config/hypr/bindings.lua` -> `hypr/bindings.lua`; other paths as is.
fn short_path(path: &str) -> String {
    path.strip_prefix("~/.config/")
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn truncate(text: String) -> String {
    if text.chars().count() <= DESCRIPTION_MAX {
        return text;
    }
    let mut cut: String = text.chars().take(DESCRIPTION_MAX - 1).collect();
    if let Some(boundary) = cut.rfind(", ") {
        cut.truncate(boundary);
    }
    cut.push('…');
    cut
}

/// A minimal record for repository tests.
#[cfg(test)]
pub(crate) fn test_checkpoint(uuid: &str, snapshot: Option<&str>) -> Checkpoint {
    Checkpoint {
        schema_version: store::SCHEMA_VERSION,
        uuid: uuid.to_string(),
        machine: Machine {
            id: "machine".into(),
            name: "test".into(),
        },
        created_at: store::now_rfc3339(),
        mise_version: "0".into(),
        trigger: Trigger::Save,
        description: "test".into(),
        description_source: DescriptionSource::Computed,
        summary: "test".into(),
        task: None,
        labels: vec![],
        pinned: false,
        tree: TreeInfo {
            snapshot: snapshot.map(str::to_string),
            available: snapshot.is_some(),
            reason: None,
            roots: vec![],
            coverage: Default::default(),
        },
        changes: Changes::default(),
        operation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(modified: &[&str], added: &[&str], removed: &[&str]) -> Changes {
        Changes {
            since: None,
            added: added.iter().map(|s| s.to_string()).collect(),
            modified: modified.iter().map(|s| s.to_string()).collect(),
            removed: removed.iter().map(|s| s.to_string()).collect(),
            truncated: false,
        }
    }

    #[test]
    fn descriptions_group_sort_and_cap() {
        let draft = Draft::new(Trigger::Edit);
        assert_eq!(
            describe(&draft, &Changes::default(), false, 12),
            "initial checkpoint (12 files)"
        );
        assert_eq!(
            describe(&draft, &Changes::default(), true, 12),
            "no file changes"
        );
        let c = changes(
            &["~/.config/hypr/monitors.lua", "~/.config/hypr/bindings.lua"],
            &["~/.config/omarchy/hooks/post-theme"],
            &["~/.XCompose"],
        );
        assert_eq!(
            describe(&draft, &c, true, 0),
            "edited hypr/bindings.lua, hypr/monitors.lua; added omarchy/hooks/post-theme; removed ~/.XCompose"
        );
        let many: Vec<String> = (0..10).map(|i| format!("~/.f{i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let text = describe(&draft, &changes(&refs, &[], &[]), true, 0);
        assert!(text.ends_with("+4 more"), "{text}");
        assert!(text.chars().count() <= DESCRIPTION_MAX);
    }

    #[test]
    fn operation_descriptions_name_the_kind() {
        let mut draft = Draft::new(Trigger::Bootstrap);
        draft.operation = Some(Operation {
            kind: store::OperationKind::Bootstrap,
            status: store::OperationStatus::Completed,
            command: "bootstrap".into(),
            argv: vec![],
            cwd: PathBuf::new(),
            user: None,
            finished_at: None,
            error: None,
            before: None,
            to: None,
            undoes: None,
            applied: None,
            affected: vec![],
            parts: vec!["dotfiles".into()],
            message: None,
            journal: vec![],
            lockfile: None,
        });
        let c = changes(&["~/.zshrc"], &[], &[]);
        assert_eq!(
            describe(&draft, &c, true, 0),
            "bootstrap dotfiles: edited ~/.zshrc"
        );
        let mut protective = Draft::new(Trigger::BootstrapBefore);
        protective.protective = true;
        assert_eq!(describe(&protective, &c, true, 0), "before bootstrap");
    }

    #[test]
    fn metadata_decides_deduplication() {
        assert!(!Draft::new(Trigger::Edit).has_metadata());
        assert!(!Draft::new(Trigger::Save).has_metadata());
        assert!(Draft::new(Trigger::Agent).has_metadata());
        let mut described = Draft::new(Trigger::Edit);
        described.description = Some("x".into());
        assert!(described.has_metadata());
    }
}
