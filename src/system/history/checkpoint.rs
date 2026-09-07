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

/// A subtree promotion: the entry's index and the children that were named.
type PartialPromotion = (usize, Vec<PathBuf>);

use super::shadow::{self, HistoryRepo, Overlay};
use super::store::{
    self, Annotation, Changes, Checkpoint, DescriptionSource, Entry, Index, IndexEntry, Machine,
    Operation, TreeInfo, Trigger,
};
use super::tracked::{TrackedEntry, TrackedSet, tree_path_to_display};
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
    /// Explicit enrollment removals; never remove the corresponding live files.
    pub untrack: Vec<PathBuf>,
    /// Paths the watcher is throttling: their files are carried forward
    /// from the newest checkpoint instead of read live, so a capture for
    /// another path never defeats their schedule. Ignored by protective
    /// captures and explicit saves.
    pub held: Vec<PathBuf>,
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
        let machine = store::machine();
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
        Ok(store)
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
        if self.repo.is_none() {
            return Ok(Outcome::Unavailable(
                self.unavailable
                    .clone()
                    .unwrap_or_else(shadow::unavailable_reason),
            ));
        }
        let resolved = match &self.repo {
            Some(repo) => super::enrollment::resolve(
                &self.state_dir,
                repo,
                tracked,
                if draft.trigger() == Trigger::Baseline {
                    &draft.explicit_paths
                } else {
                    &[]
                },
                &draft.untrack,
            )?,
            None => tracked.clone(),
        };
        let tracked = &resolved;
        if tracked.entries.is_empty()
            && !draft.has_metadata()
            && self
                .repo
                .as_ref()
                .map(|repo| repo.ref_oid(HistoryRepo::HISTORY_REF))
                .transpose()?
                .flatten()
                .is_none()
        {
            // A service may watch policy before the first enrollment. Do not
            // manufacture an unrelated empty root before --from-git adoption.
            return Ok(Outcome::Unchanged);
        }
        let mut index = store::load_index_in(&self.state_dir)?;
        if let Some(repo) = &self.repo
            && repo.ref_oid(HistoryRepo::HISTORY_REF)?.as_deref()
                != index.entries.last().map(|entry| entry.commit.as_str())
        {
            index = self.rebuild_index_locked()?;
        }
        // Metadata-only records (for example an operation while git was
        // unavailable) must not hide the last usable content baseline.
        let previous_tree = index.entries.iter().rev().find_map(|entry| {
            store::read_meta_cache_in(&self.state_dir, &entry.uuid)
                .ok()
                .flatten()
                .and_then(|checkpoint| {
                    checkpoint
                        .tree
                        .snapshot
                        .clone()
                        .map(|tree| (checkpoint, tree))
                })
        });
        let uuid = draft.uuid.clone().unwrap_or_else(store::new_uuid);
        let (mut walk, walk_error) = match tracked.walk() {
            Ok(walk) => (walk, None),
            Err(err) if draft.operation.is_some() => {
                // Keep the operation journal even when its outcome cannot be
                // captured. Never substitute a partial or empty snapshot.
                warn!("history: snapshot failed: {err:#}");
                (Default::default(), Some(format!("{err:#}")))
            }
            Err(err) => return Err(err),
        };
        for warning in &walk.warnings {
            warn!("history: {warning}");
        }
        // manual-save entries: carried forward from their promoted version
        // unless named explicitly (promoted) or captured protectively
        let promoted: BTreeSet<String> = previous_tree
            .as_ref()
            .map(|(record, _)| {
                record
                    .tree
                    .coverage
                    .entries
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        let manual = manual_plan(&walk, &draft, &promoted);
        if !manual.carry.is_empty() {
            let carried: BTreeSet<usize> = manual.carry.iter().copied().collect();
            let dropped: BTreeSet<PathBuf> = walk
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
        // Live modes plus the ordinary parent's modes for carried entries.
        // Protective commits use the same history, not a separate saved state.
        let mut modes = file_modes(&walk);
        if !manual.carry.is_empty() {
            let carried: Vec<String> = manual
                .carry
                .iter()
                .map(|index| walk.entries[*index].display())
                .collect();
            for (path, bits) in self.saved_modes(&index, &carried) {
                modes.entry(path).or_insert(bits);
            }
        }
        // a held path keeps the mode the previous checkpoint recorded (or
        // none), like its content
        if !draft.held.is_empty() && !draft.protective && draft.explicit_paths.is_empty() {
            for held in &draft.held {
                let display = display_path(held);
                modes.remove(&display);
                if let Some((previous_checkpoint, _)) = &previous_tree
                    && let Some(bits) = previous_checkpoint.tree.modes.get(&display)
                {
                    modes.insert(display, *bits);
                }
            }
        }
        let mut coverage = tracked.coverage(&walk);
        let parent_commit = match &self.repo {
            Some(repo) => repo.ref_oid(HistoryRepo::HISTORY_REF)?,
            None => None,
        };
        let recipients = if walk.files.values().any(|(_, policy)| policy.encrypt) {
            tracked.manifest.recipients.clone()
        } else {
            vec![]
        };
        let (snapshot, roots, available, reason) = match (&self.repo, walk_error) {
            (_, Some(reason)) => (None, vec![], false, Some(reason)),
            (Some(repo), None) => {
                match repo.capture_tracked(&walk, &recipients, console::user_attended_stderr()) {
                    Ok(result) => {
                        coverage.omitted.extend(result.omitted.iter().cloned());
                        for warning in &result.warnings {
                            warn!("history: {warning}");
                        }
                        let (composed, partial) = self.compose_manual(
                            repo,
                            &result.tree,
                            ManualContext {
                                entries: &walk.entries,
                                manual: &manual,
                                parent_commit: parent_commit.as_deref(),
                                draft: &draft,
                            },
                        )?;
                        // a subtree promotion keeps the saved modes of the
                        // siblings it did not name, like their content
                        for (entry_index, children) in &partial {
                            let entry = walk.entries[*entry_index].display();
                            let named: Vec<String> =
                                children.iter().map(crate::file::display_path).collect();
                            let is_named =
                                |path: &str| named.iter().any(|child| under_entry(path, child));
                            modes.retain(|path, _| !under_entry(path, &entry) || is_named(path));
                            for (path, bits) in
                                self.saved_modes(&index, std::slice::from_ref(&entry))
                            {
                                if !is_named(&path) {
                                    modes.entry(path).or_insert(bits);
                                }
                            }
                        }
                        let composed = self.hold_paths(
                            repo,
                            &composed,
                            &draft,
                            previous_tree.as_ref().map(|(_, tree)| tree.as_str()),
                            &walk.entries,
                        )?;
                        let omissions: Vec<_> = coverage
                            .omitted
                            .iter()
                            .chain(&coverage.incomplete)
                            .collect();
                        let composed = retain_omitted(
                            repo,
                            &composed,
                            previous_tree
                                .as_ref()
                                .map(|(record, tree)| (record, tree.as_str())),
                            tracked,
                            &omissions,
                            &mut modes,
                            &walk,
                        )?;
                        let mut manifest = super::manifest::Manifest::read(repo, &composed)?
                            .ok_or_else(|| {
                                eyre::eyre!("captured tree is missing enrollment metadata")
                            })?;
                        manifest.capture_permissions(&walk.entries, &modes)?;
                        let composed = manifest.write(repo, &composed)?;
                        (Some(composed), result.roots, true, None)
                    }
                    Err(err) => {
                        warn!("history: snapshot failed: {err:#}");
                        (None, vec![], false, Some(format!("{err:#}")))
                    }
                }
            }
            (None, None) => (None, vec![], false, self.unavailable.clone()),
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
                record.promotion = None;
            } else {
                record.state = "saved".into();
                record.promotion = None;
            }
        }
        if !draft.has_metadata()
            && let Some((previous_checkpoint, tree)) = &previous_tree
            && snapshot.as_deref() == Some(tree.as_str())
            && (!cfg!(unix) || previous_checkpoint.tree.modes == modes)
        {
            debug!(
                "history: nothing changed since checkpoint {}",
                previous_checkpoint.uuid
            );
            if let Some(repo) = &self.repo {
                super::enrollment::confirm(&self.state_dir, repo, tracked)?;
            }
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
        // the same bytes under other permissions is a change the tree diff
        // does not show: a path-scoped `latest` and `--path` must see it
        if snapshot.is_some()
            && let Some((previous_checkpoint, _)) = &previous_tree
        {
            let previous_modes = &previous_checkpoint.tree.modes;
            let mode_only: BTreeSet<&String> = modes
                .keys()
                .chain(previous_modes.keys())
                .filter(|path| modes.get(*path) != previous_modes.get(*path))
                .filter(|path| !changes.touches(path))
                .collect();
            changes.modified.extend(mode_only.into_iter().cloned());
        }
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
        let mut checkpoint = Checkpoint {
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
                modes,
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
        if let Some(repo) = &self.repo {
            checkpoint = repo.read_meta(&commit)?;
        }
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
        if available
            && snapshot.is_some()
            && let Some(repo) = &self.repo
        {
            super::enrollment::confirm(&self.state_dir, repo, tracked)?;
        }
        debug!(
            "history: recorded checkpoint {id} ({}): {}",
            checkpoint.trigger.as_str(),
            checkpoint.description
        );
        Ok(Outcome::Created(Box::new(Entry {
            id,
            commit,
            checkpoint,
        })))
    }

    /// Carries the draft's held paths forward from the previous checkpoint:
    /// their live content is not what this capture records. Protective
    /// captures and explicit saves hold nothing.
    /// Held paths (files whose save is not due yet) take their previous
    /// version, or previous absence, from the newest checkpoint: a change,
    /// a creation, and a deletion are all held until the path is due. Held
    /// paths are files, matched exactly.
    fn hold_paths(
        &self,
        repo: &HistoryRepo,
        tree: &str,
        draft: &Draft,
        previous: Option<&str>,
        entries: &[TrackedEntry],
    ) -> Result<String> {
        if draft.held.is_empty() || draft.protective || !draft.explicit_paths.is_empty() {
            return Ok(tree.to_string());
        }
        let mut overlays = vec![];
        for held in &draft.held {
            let Some(entry) = entries
                .iter()
                .filter(|entry| held.starts_with(&entry.path))
                .max_by_key(|entry| entry.path.components().count())
            else {
                continue;
            };
            let tree_path = entry.tree_path(held)?;
            overlays.push(Overlay {
                object: previous
                    .map(|parent| repo.object_at(parent, &tree_path))
                    .transpose()?
                    .flatten(),
                path: tree_path,
            });
        }
        repo.compose(tree, &overlays)
    }

    fn compose_manual(
        &self,
        repo: &HistoryRepo,
        live_tree: &str,
        context: ManualContext<'_>,
    ) -> Result<(String, Vec<PartialPromotion>)> {
        let ManualContext {
            entries,
            manual,
            parent_commit,
            draft,
        } = context;
        let mut overlays = vec![];
        let mut partial_entries = vec![];
        for index in &manual.carry {
            let entry = &entries[*index];
            let path = entry.tree_path(&entry.path)?;
            let object = parent_commit
                .map(|head| repo.object_at(head, &path))
                .transpose()?
                .flatten();
            overlays.push(Overlay { path, object });
        }
        // A selective save replaces only named children of a manual directory.
        // Its siblings come from the ordinary parent, not a promotion branch.
        if let Some(head) = parent_commit {
            for index in &manual.promote {
                let entry = &entries[*index];
                let children: Vec<PathBuf> = draft
                    .explicit_paths
                    .iter()
                    .filter(|path| path.starts_with(&entry.path) && **path != entry.path)
                    .cloned()
                    .collect();
                if children.is_empty()
                    || draft
                        .explicit_paths
                        .iter()
                        .any(|path| entry.path.starts_with(path))
                {
                    continue;
                }
                let path = entry.tree_path(&entry.path)?;
                overlays.push(Overlay {
                    object: repo.object_at(head, &path)?,
                    path,
                });
                for child in &children {
                    let path = entry.tree_path(child)?;
                    overlays.push(Overlay {
                        object: repo.object_at(live_tree, &path)?,
                        path,
                    });
                }
                partial_entries.push((*index, children));
            }
        }
        Ok((repo.compose(live_tree, &overlays)?, partial_entries))
    }

    /// Manual files carry permission metadata from the ordinary parent.
    fn saved_modes(&self, index: &store::Index, entries: &[String]) -> BTreeMap<String, u32> {
        index
            .entries
            .last()
            .and_then(|entry| {
                store::read_meta_cache_in(&self.state_dir, &entry.uuid)
                    .ok()
                    .flatten()
            })
            .map(|record| {
                record
                    .tree
                    .modes
                    .into_iter()
                    .filter(|(path, _)| entries.iter().any(|entry| under_entry(path, entry)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Rebuilds the local index from ordinary commit ancestry.
    pub(crate) fn rebuild_index(&self) -> Result<Index> {
        let _lock = self.lock()?;
        self.rebuild_index_locked()
    }

    fn rebuild_index_locked(&self) -> Result<Index> {
        let Some(repo) = &self.repo else {
            return store::load_index_in(&self.state_dir);
        };
        let existing = store::load_index_in(&self.state_dir)?;
        let mut entries = vec![];
        let commits: Vec<_> = repo.checkpoint_refs()?.into_iter().rev().collect();
        let mut annotations: BTreeMap<String, Vec<Annotation>> = BTreeMap::new();
        for (_, commit) in &commits {
            if let Some((target, annotation)) = repo.read_annotation(commit)? {
                annotations.entry(target).or_default().push(annotation);
            }
        }
        for (uuid, commit) in commits {
            // Annotation commits remain in Git ancestry, but are not new file
            // checkpoints and must not move `latest` in the history browser.
            if repo.read_annotation(&commit)?.is_some() {
                continue;
            }
            let mut checkpoint = repo.read_meta(&commit)?;
            if let Some(annotations) = annotations.get(&commit) {
                for annotation in annotations {
                    annotation.apply_to(&mut checkpoint);
                }
            }
            store::write_meta_cache_in(&self.state_dir, &checkpoint)?;
            let id = existing.by_uuid(&uuid).map(|entry| entry.id);
            entries.push((id, checkpoint, commit));
        }
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
        index.next_id = next_id.max(index.entries.iter().map(|e| e.id + 1).max().unwrap_or(1));
        store::write_index_in(&self.state_dir, &index)?;
        Ok(index)
    }
}

/// Append annotation metadata to ordinary history and rebuild its derived view.
pub(crate) fn annotate(store: &Store, entry: &Entry, annotation: Annotation) -> Result<()> {
    let _lock = store.lock()?;
    if let Some(repo) = store.repo()
        && !entry.commit.is_empty()
    {
        repo.write_annotation(&entry.commit, &annotation)?;
        store.rebuild_index_locked()?;
        return Ok(());
    }
    let mut checkpoint = entry.checkpoint.clone();
    annotation.apply_to(&mut checkpoint);
    store::write_meta_cache_in(store.state_dir(), &checkpoint)
}

/// Everything composing manual-save entries into a snapshot needs.
struct ManualContext<'a> {
    entries: &'a [TrackedEntry],
    manual: &'a ManualPlan,
    parent_commit: Option<&'a str>,
    draft: &'a Draft,
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
        if named || never_promoted {
            plan.promote.push(index);
        } else {
            plan.carry.push(index);
        }
    }
    plan
}

/// Whether `path` is the entry itself or below it.
fn under_entry(path: &str, entry: &str) -> bool {
    path == entry
        || path
            .strip_prefix(entry)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// A failed observation is not evidence of deletion. Carry only saved objects
/// still permitted by explicit enrollment and exclusions; successful reads win.
fn retain_omitted(
    repo: &HistoryRepo,
    tree: &str,
    previous: Option<(&Checkpoint, &str)>,
    tracked: &TrackedSet,
    omissions: &[&store::PathReason],
    modes: &mut BTreeMap<String, u32>,
    walk: &super::tracked::Walk,
) -> Result<String> {
    let Some((record, parent)) = previous.filter(|_| !omissions.is_empty()) else {
        return Ok(tree.into());
    };
    let roots = super::sync::layout::Roots::current();
    let omitted: Vec<_> = omissions
        .iter()
        .map(|failure| crate::file::replace_path(&failure.path))
        .collect();
    let mut overlays = vec![];
    let observed_directories: BTreeSet<_> = walk
        .files
        .keys()
        .flat_map(|path| path.ancestors().skip(1))
        .collect();
    for file in repo.ls_tree(parent)? {
        let located = roots.locate(&file.path);
        let Some(path) = located.path() else { continue };
        if !omitted.iter().any(|omitted| path.starts_with(omitted))
            || !tracked.would_retain(path)?
            || repo.object_at(tree, &file.path)?.is_some()
        {
            continue;
        }
        let Some(entry) = tracked.entry_for(path) else {
            continue;
        };
        if entry.tree_path(path)? != file.path {
            continue;
        }
        if entry.policy.encrypt && !repo.blob_starts_with(&file.oid, b"mise-encrypted-file-v1\n")? {
            eyre::bail!(
                "cannot retain an unreadable plaintext version of newly encrypted {}",
                display_path(path)
            );
        }
        let display = display_path(path);
        modes.remove(&display);
        if let Some(mode) = record.tree.modes.get(&display) {
            modes.insert(display, *mode);
        }
        for parent in path
            .ancestors()
            .skip(1)
            .take_while(|parent| parent.starts_with(&entry.path))
        {
            if !observed_directories.contains(parent) {
                let display = display_path(parent);
                if let Some(mode) = record.tree.modes.get(&display) {
                    modes.insert(display, *mode);
                }
            }
        }
        overlays.push(Overlay {
            path: file.path,
            object: Some((file.mode, file.oid)),
        });
    }
    repo.compose(tree, &overlays)
}

/// The permission bits of captured regular files that git cannot record
/// (anything but `0644` and `0755`), so a restore can put them back.
#[cfg(unix)]
fn file_modes(walk: &super::tracked::Walk) -> BTreeMap<String, u32> {
    use std::os::unix::fs::PermissionsExt;
    let mut modes = BTreeMap::new();
    let mut dirs = BTreeSet::new();
    for (path, (owner, _)) in &walk.files {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            continue;
        };
        if meta.is_file() {
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o644 && mode != 0o755 {
                modes.insert(display_path(path), mode);
            }
        }
        // Directory permissions belong to an explicitly enrolled directory,
        // not to its parents. Enrolling an individual file must not capture
        // unrelated metadata about home or a custom configuration root.
        let enrolled = &walk.entries[*owner].path;
        for ancestor in path.ancestors().skip(1) {
            if !ancestor.starts_with(enrolled) {
                break;
            }
            dirs.insert(ancestor.to_path_buf());
        }
    }
    for dir in dirs {
        let Ok(meta) = std::fs::symlink_metadata(&dir) else {
            continue;
        };
        if meta.is_dir() {
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o755 {
                modes.insert(display_path(&dir), mode);
            }
        }
    }
    modes
}

#[cfg(not(unix))]
fn file_modes(_walk: &super::tracked::Walk) -> BTreeMap<String, u32> {
    BTreeMap::new()
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
        if change.path.starts_with(".mise-history/") {
            continue;
        }
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
        let what = match operation.message.as_deref() {
            Some(message) => format!(": {message}"),
            None if !changes.is_empty() => format!(": {}", describe_changes(changes)),
            None => String::new(),
        };
        return truncate(format!("{}{what}", operation.kind.as_str()));
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

pub(crate) fn describe_changes(changes: &Changes) -> String {
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
            modes: Default::default(),
        },
        changes: Changes::default(),
        operation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_capture_without_enrollment_does_not_create_a_root() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open_in(temp.path())?;
        assert!(matches!(
            store.attempt(&TrackedSet::default(), Draft::new(Trigger::Edit))?,
            Outcome::Unchanged
        ));
        assert!(
            store
                .repo()
                .unwrap()
                .ref_oid(HistoryRepo::HISTORY_REF)?
                .is_none()
        );
        let mut labeled = Draft::new(Trigger::Agent);
        labeled.description = Some("explicit operation boundary".into());
        assert!(matches!(
            store.attempt(&TrackedSet::default(), labeled)?,
            Outcome::Created(_)
        ));
        Ok(())
    }

    #[test]
    fn unavailable_git_does_not_create_a_parallel_metadata_history() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut store = Store::open_in(temp.path())?;
        store.repo = None;
        store.unavailable = Some("Git is unavailable".into());
        let mut draft = Draft::new(Trigger::Agent);
        draft.description = Some("labeled operation".into());
        assert!(matches!(
            store.attempt(&TrackedSet::default(), draft)?,
            Outcome::Unavailable(_)
        ));
        assert!(store::load_index_in(temp.path())?.entries.is_empty());
        Ok(())
    }

    #[test]
    fn numeric_commit_prefixes_cannot_silently_select_another_checkpoint() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open_in(temp.path())?;
        let Outcome::Created(mut first) =
            store.attempt(&TrackedSet::default(), Draft::new(Trigger::Agent))?
        else {
            panic!("expected a checkpoint");
        };
        first.id = 42;
        first.checkpoint.uuid = "123abc".into();
        let mut second = first.clone();
        second.id = 123;
        second.checkpoint.uuid = "abcdef".into();
        let entries = vec![*first, *second];
        assert_eq!(store::resolve_ref("12", &entries)?, 42);
        assert_eq!(store::resolve_ref("42", &entries)?, 42);
        assert!(store::resolve_ref("123", &entries).is_err());
        assert_eq!(store::resolve_ref("123abc", &entries)?, 42);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn permission_capture_stops_at_explicit_enrollment() {
        use super::super::tracked::{TrackedEntry, Walk};
        use crate::system::files::{FileMode, FilePolicy};
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("private-parent");
        let enrolled = parent.join("enrolled");
        std::fs::create_dir_all(&enrolled).unwrap();
        let path = enrolled.join("config");
        std::fs::write(&path, "config").unwrap();
        for directory in [&parent, &enrolled] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let policy = FilePolicy::for_mode(FileMode::Track);
        let mut walk = Walk {
            entries: vec![TrackedEntry::new(enrolled.clone(), "track", policy)],
            files: BTreeMap::from([(path.clone(), (0, policy))]),
            ..Default::default()
        };
        assert_eq!(
            file_modes(&walk),
            BTreeMap::from([
                (display_path(&enrolled), 0o700),
                (display_path(&path), 0o600),
            ])
        );
        walk.entries[0].path = path.clone();
        assert_eq!(
            file_modes(&walk),
            BTreeMap::from([(display_path(&path), 0o600)])
        );
    }

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
            id: "test-operation".into(),
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
            sources: vec![],
            directories: vec![],
            directory_modes: Default::default(),
            message: None,
            journal: vec![],
        });
        let c = changes(&["~/.zshrc"], &[], &[]);
        assert_eq!(describe(&draft, &c, true, 0), "bootstrap: edited ~/.zshrc");
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
