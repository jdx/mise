//! Erasing a path's saved versions from the history.
//!
//! Every commit is rewritten without the path, so the other files'
//! checkpoints survive. A checkpoint's identity is its commit hash, and other
//! checkpoints name it in their commit trailers (an operation's `before`, an
//! annotation's target), so the rewrite carries an old-to-new map through
//! trailers and the numeric-ID index.
//!
//! A commit that changed only the purged path has nothing left and is
//! dropped. Everything else keeps its author, dates, message and numeric ID.
//! History the origin already has is never rewritten: mise does not
//! force-push, so a rewrite would leave the two unrelated. A path whose
//! history is on the origin is refused instead.

use std::collections::{HashMap, HashSet};

use eyre::{Result, WrapErr, bail};

use super::checkpoint::Store;
use super::manifest::{self, Manifest};
use super::shadow::{HistoryRepo, Overlay};
use super::store::{self, CommitRecord, IndexEntry};
use super::sync::network::UPSTREAM_REF;
use super::tracked::display_to_tree_path;

/// Where ref-less backups of the replaced history are kept.
const BACKUP_PREFIX: &str = "refs/mise/purge-backup/";

/// A purged path, as a position in the snapshot tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// What the user typed.
    pub display: String,
    /// The tree root (`home`, `config`, `fs`). A variant's root is
    /// `home@name`, and is purged along with the base.
    root: String,
    relative: Vec<String>,
}

impl Target {
    pub fn parse(display: &str) -> Result<Self> {
        let tree_path = display_to_tree_path(display);
        let mut parts = tree_path.split('/').map(str::to_owned);
        let root = parts.next().unwrap_or_default();
        let relative: Vec<String> = parts.filter(|part| !part.is_empty()).collect();
        if relative.is_empty() {
            bail!("{display}: purging a whole root is not supported; name a file or directory");
        }
        Ok(Self {
            display: display.to_string(),
            root,
            relative,
        })
    }

    fn matches_root(&self, name: &str) -> bool {
        name == self.root
            || name
                .strip_prefix(&self.root)
                .is_some_and(|rest| rest.starts_with('@'))
    }

    /// Whether a portable (variant) path is this target or inside it.
    fn covers_branch_path(&self, path: &str) -> bool {
        let mut parts = path.split('/');
        if !parts.next().is_some_and(|root| self.matches_root(root)) {
            return false;
        }
        let rest: Vec<&str> = parts.collect();
        rest.len() >= self.relative.len() && rest.iter().zip(&self.relative).all(|(a, b)| a == b)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Rewrite history. Without it, only the plan is computed.
    pub apply: bool,
    /// Keep the replaced history reachable under `refs/mise/purge-backup/`.
    /// It still holds the purged content.
    pub backup: bool,
    /// Prune the objects the rewrite left unreachable.
    pub gc: bool,
}

/// One checkpoint the purge removes or rewrites.
#[derive(Clone, Debug)]
pub struct Affected {
    /// Numeric ID, when the checkpoint is in the index.
    pub id: Option<u64>,
    pub commit: String,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub applied: bool,
    pub backup_ref: Option<String>,
    /// Checkpoints that held the path and are removed outright.
    pub dropped: Vec<Affected>,
    /// Checkpoints kept, with the path removed from what they hold.
    pub rewritten: Vec<Affected>,
    /// Checkpoints whose hash changes only because an ancestor's did.
    pub rehashed: usize,
    pub total: usize,
    /// Other refs that still reach the purged content.
    pub other_refs: Vec<String>,
}

impl Outcome {
    pub fn is_noop(&self) -> bool {
        self.dropped.is_empty() && self.rewritten.is_empty()
    }
}

/// What rewriting one commit decided.
enum Rewritten {
    /// The commit stays, under this id.
    Kept(String),
    /// The commit held nothing but the path. Later commits that named it
    /// name `nearest` instead.
    Dropped { nearest: Option<String> },
}

struct Walk<'a> {
    repo: &'a HistoryRepo,
    gix: gix::Repository,
    targets: &'a [Target],
    empty_tree: String,
    tree_cache: HashMap<(usize, usize, String), Option<String>>,
    /// Stripped tree per original tree.
    stripped: HashMap<String, String>,
    map: HashMap<String, Rewritten>,
    /// Commits whose tree held a target.
    held: HashSet<String>,
    /// Commits removed because all they changed was a target (a deletion
    /// included), as opposed to annotations of those.
    removed: HashSet<String>,
}

/// Previews or performs the purge. Holds the operation and store locks, so
/// nothing saves while the history is rewritten.
pub fn purge(
    store: &Store,
    tracked: &super::tracked::TrackedSet,
    targets: &[Target],
    options: Options,
) -> Result<Outcome> {
    let _operation = super::scope::take_operation_lock(store, tracked)?;
    let _store = store.lock()?;
    let state_dir = store.state_dir();
    if let Some(marker) = store::read_marker_in(state_dir)? {
        bail!(
            "a history operation is recorded as running ({}); finish or recover it first",
            marker.command
        );
    }
    if !store::list_pending_in(state_dir)?.is_empty() {
        bail!("an earlier history operation is unfinished; run `mise dot recover` first");
    }
    let repo = store.repo().ok_or_else(|| {
        eyre::eyre!(
            "history needs git; {}",
            store.unavailable().unwrap_or("git is unavailable")
        )
    })?;
    recover(store, repo)?;
    let Some(head) = repo.ref_oid(HistoryRepo::HISTORY_REF)? else {
        return Ok(Outcome::default());
    };
    let mut walk = Walk {
        repo,
        gix: repo.gix()?,
        targets,
        empty_tree: repo.empty_object("tree")?,
        tree_cache: HashMap::new(),
        stripped: HashMap::new(),
        map: HashMap::new(),
        held: HashSet::new(),
        removed: HashSet::new(),
    };
    let commits: Vec<String> = repo
        .rev_list(&head, usize::MAX)?
        .into_iter()
        .rev()
        .collect();
    let index = store::load_index_in(state_dir)?;
    let ids: HashMap<&str, u64> = index
        .entries
        .iter()
        .map(|entry| (entry.commit.as_str(), entry.id))
        .collect();
    let mut outcome = Outcome {
        total: commits.len(),
        ..Outcome::default()
    };
    for commit in &commits {
        walk.rewrite(commit)?;
    }

    // Published history must not change; refuse rather than diverge.
    if let Some(upstream) = repo.ref_oid(UPSTREAM_REF)? {
        let published: HashSet<String> =
            repo.rev_list(&upstream, usize::MAX)?.into_iter().collect();
        // compare trees, not commit ids: another machine may have saved the
        // same path in commits this history does not have
        let mut on_origin: Vec<String> = vec![];
        for commit in &published {
            // a published commit must keep its hash: the rewrite may also
            // change one that holds no file under the path (a permission
            // entry, a trailer naming a dropped checkpoint)
            let on_it = match walk.map.get(commit) {
                Some(Rewritten::Kept(new)) if new == commit => walk.held.contains(commit),
                Some(_) => true,
                None => {
                    let object = walk
                        .gix
                        .find_commit(gix::ObjectId::from_hex(commit.as_bytes())?)?;
                    walk.holds(&object.tree_id()?.to_string())?
                }
            };
            if on_it {
                on_origin.push(commit.clone());
            }
        }
        if !on_origin.is_empty() {
            on_origin.sort();
            bail!(
                "{} saved version(s) of this path are already on the origin, and mise never rewrites published history.\n\
                 Rotate the secret first, then remove it from the origin with Git and re-adopt the history on every machine; see https://mise.jdx.dev/dotfiles/encryption.html#remove-plaintext-from-history",
                on_origin.len()
            );
        }
    }

    for commit in &commits {
        let Some(new) = walk.map.get(commit) else {
            continue;
        };
        let describe = |commit: &String| -> Result<Affected> {
            Ok(Affected {
                id: ids.get(commit.as_str()).copied(),
                commit: commit.clone(),
                description: walk.repo.read_meta(commit)?.description,
            })
        };
        match new {
            Rewritten::Dropped { .. } if walk.removed.contains(commit) => {
                outcome.dropped.push(describe(commit)?)
            }
            // an annotation of a dropped checkpoint goes with it
            Rewritten::Dropped { .. } => {}
            Rewritten::Kept(new) if new != commit && walk.held.contains(commit) => {
                outcome.rewritten.push(describe(commit)?)
            }
            Rewritten::Kept(new) if new != commit => outcome.rehashed += 1,
            Rewritten::Kept(_) => {}
        }
    }

    // refs other than the history and its upstream keep the old content
    // reachable (an earlier purge's backup, a hand-made ref)
    for (name, oid) in repo.list_refs("refs/")? {
        if name == HistoryRepo::HISTORY_REF || name == UPSTREAM_REF {
            continue;
        }
        if repo.object_type(&oid)?.as_deref() != Some("commit") {
            continue;
        }
        let reachable = repo.rev_list(&oid, usize::MAX)?;
        let mut holds = false;
        for commit in &reachable {
            holds = if walk.map.contains_key(commit) {
                walk.held.contains(commit)
            } else {
                let object = walk
                    .gix
                    .find_commit(gix::ObjectId::from_hex(commit.as_bytes())?)?;
                walk.holds(&object.tree_id()?.to_string())?
            };
            if holds {
                break;
            }
        }
        if holds {
            outcome.other_refs.push(name);
        }
    }

    if outcome.is_noop() || !options.apply {
        return Ok(outcome);
    }

    // the newest checkpoint may itself be dropped; the branch then ends at
    // its nearest surviving ancestor
    let Some(new_head) = walk.remap(&head) else {
        bail!("the path is all this history holds; nothing would remain");
    };

    if options.backup {
        let name = format!("{BACKUP_PREFIX}{}", store::now_rfc3339().replace(':', "-"));
        repo.update_ref(&name, &head, None)?;
        outcome.backup_ref = Some(name);
    }
    let mut entries = vec![];
    for entry in &index.entries {
        if let Some(Rewritten::Kept(new)) = walk.map.get(&entry.commit) {
            entries.push(IndexEntry {
                id: entry.id,
                uuid: new.clone(),
                commit: new.clone(),
                created_at: entry.created_at.clone(),
                trigger: entry.trigger,
            });
        }
    }
    let pending = Pending {
        old_head: head.clone(),
        new_head: new_head.clone(),
        index: store::Index {
            next_id: index.next_id,
            entries,
        },
    };
    // Recorded before the branch moves, so a purge interrupted after it can
    // finish the index the next time one runs.
    store::write_json(&pending_path(state_dir), &pending)?;
    repo.update_history_head(&new_head, Some(&head))?;
    finish(store, &pending)?;
    if options.gc {
        collect_garbage(repo, &outcome)?;
    }
    outcome.applied = true;
    Ok(outcome)
}

/// What a purge needs to finish after the branch has moved.
#[derive(serde::Serialize, serde::Deserialize)]
struct Pending {
    old_head: String,
    new_head: String,
    index: store::Index,
}

fn pending_path(state_dir: &std::path::Path) -> std::path::PathBuf {
    store::store_dir_in(state_dir).join("purge-pending.json")
}

/// Finishes a purge that was interrupted, for whoever opens the store next.
pub(crate) fn recover_interrupted(store: &Store) {
    let Some(repo) = store.repo() else {
        return;
    };
    if !pending_path(store.state_dir()).exists() {
        return;
    }
    let result = store.lock().and_then(|_lock| recover(store, repo));
    if let Err(err) = result {
        warn!("history: could not finish an interrupted purge: {err:#}");
    }
}

/// Finishes (or discards) a purge that was interrupted. Holds the store lock.
fn recover(store: &Store, repo: &HistoryRepo) -> Result<()> {
    let path = pending_path(store.state_dir());
    if !path.exists() {
        return Ok(());
    }
    let text = mise_util::file::read_to_string(&path)?;
    let pending: Pending = serde_json::from_str(&text)
        .wrap_err("reading the record of an interrupted `mise dot history purge`")?;
    match repo.ref_oid(HistoryRepo::HISTORY_REF)?.as_deref() {
        Some(head) if head == pending.new_head => {
            debug!("history: finishing an interrupted purge");
            finish(store, &pending)?;
        }
        _ => {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

fn finish(store: &Store, pending: &Pending) -> Result<()> {
    let state_dir = store.state_dir();
    // The index is rebuilt from ancestry; numeric IDs travel through the
    // new hashes, which are also the checkpoints' uuids.
    store::write_index_in(state_dir, &pending.index)?;
    store::clear_meta_caches_in(state_dir)?;
    store.rebuild_index_locked()?;
    let _ = std::fs::remove_file(pending_path(state_dir));
    Ok(())
}

fn collect_garbage(repo: &HistoryRepo, outcome: &Outcome) -> Result<()> {
    repo.expire_reflog()?;
    // Unreachable content stays on disk until pruned, and a backup keeps it
    // reachable on purpose.
    if outcome.backup_ref.is_none() && outcome.other_refs.is_empty() {
        repo.prune_now()?;
    }
    Ok(())
}

impl Walk<'_> {
    fn rewrite(&mut self, commit: &str) -> Result<()> {
        let gix = self.gix.clone();
        let object = gix
            .find_commit(gix::ObjectId::from_hex(commit.as_bytes())?)
            .wrap_err_with(|| format!("reading {commit}"))?;
        let tree = object.tree_id()?.to_string();
        let parents: Vec<String> = object.parent_ids().map(|id| id.to_string()).collect();
        let raw_message = object.message_raw()?.to_vec();
        let message = String::from_utf8_lossy(&raw_message).into_owned();

        if self.holds(&tree)? {
            self.held.insert(commit.to_string());
        }
        let new_tree = self.strip_tree(&tree)?;
        let new_tree = self.strip_permissions(&new_tree)?;

        let parent_tree = match parents.first() {
            Some(parent) => self.repo.output_tree_of(parent)?,
            None => self.empty_tree.clone(),
        };
        let parent_stripped = self.strip_tree(&parent_tree)?;
        let parent_stripped = self.strip_permissions(&parent_stripped)?;

        let mut new_parents: Vec<String> = vec![];
        let mut nearest = None;
        for (position, parent) in parents.iter().enumerate() {
            let mapped = match self.map.get(parent) {
                Some(Rewritten::Kept(new)) => Some(new.clone()),
                Some(Rewritten::Dropped { nearest }) => nearest.clone(),
                None => Some(parent.clone()),
            };
            if position == 0 {
                nearest = mapped.clone();
            }
            if let Some(mapped) = mapped
                && !new_parents.contains(&mapped)
            {
                new_parents.push(mapped);
            }
        }

        // Dropped: the commit changed something, and all of it was the path.
        let changed = tree != parent_tree;
        let only_the_path = new_tree == parent_stripped;
        let annotates_dropped = annotation_target(&message)
            .is_some_and(|target| matches!(self.map.get(&target), Some(Rewritten::Dropped { .. })));
        if parents.len() <= 1 && ((changed && only_the_path) || annotates_dropped) {
            if changed && only_the_path {
                self.removed.insert(commit.to_string());
            }
            self.map
                .insert(commit.to_string(), Rewritten::Dropped { nearest });
            return Ok(());
        }

        let new_message = self.remap_message(&message)?;
        if new_tree == tree && new_parents == parents && new_message == message {
            self.map
                .insert(commit.to_string(), Rewritten::Kept(commit.to_string()));
            return Ok(());
        }
        let bytes = if new_message == message {
            raw_message
        } else {
            new_message.into_bytes()
        };
        let author = object.author()?;
        let committer = object.committer()?;
        let new = self.repo.commit_tree_as(
            &new_tree,
            new_parents.iter().map(String::as_str).collect(),
            &bytes,
            &signature(&author),
            &signature(&committer),
        )?;
        self.map.insert(commit.to_string(), Rewritten::Kept(new));
        Ok(())
    }

    fn remap(&self, commit: &str) -> Option<String> {
        match self.map.get(commit) {
            Some(Rewritten::Kept(new)) => Some(new.clone()),
            Some(Rewritten::Dropped { nearest }) => nearest.clone(),
            None => Some(commit.to_string()),
        }
    }

    /// Rewrites the checkpoint references in the commit's trailers.
    fn remap_message(&self, message: &str) -> Result<String> {
        let mut lines: Vec<String> = message.split('\n').map(str::to_owned).collect();
        let record = HistoryRepo::RECORD_TRAILER;
        let annotation = HistoryRepo::ANNOTATION_TRAILER;
        if let Some(position) = lines.iter().rposition(|line| line.starts_with(record)) {
            let text = &lines[position][record.len()..];
            let mut parsed: CommitRecord = serde_json::from_str(text)?;
            if let Some(operation) = parsed.operation.as_mut() {
                for field in [
                    &mut operation.before,
                    &mut operation.to,
                    &mut operation.undoes,
                    &mut operation.applied,
                ] {
                    *field = field.as_deref().and_then(|commit| self.remap(commit));
                }
                operation
                    .sources
                    .retain_mut(|source| match self.remap(&source.checkpoint) {
                        Some(new) => {
                            source.checkpoint = new;
                            true
                        }
                        None => false,
                    });
            }
            let rewritten = serde_json::to_string(&parsed)?;
            // keep the original text when nothing changed, so a commit that
            // names no purged checkpoint keeps its hash
            let before: serde_json::Value = serde_json::from_str(text)?;
            let after: serde_json::Value = serde_json::from_str(&rewritten)?;
            if before != after {
                lines[position] = format!("{record}{rewritten}");
            }
        }
        if let Some(position) = lines.iter().rposition(|line| line.starts_with(annotation)) {
            let text = &lines[position][annotation.len()..];
            let mut value: serde_json::Value = serde_json::from_str(text)?;
            if let Some(target) = value.get_mut(0)
                && let Some(old) = target.as_str()
                && let Some(new) = self.remap(old)
                && new != old
            {
                *target = serde_json::Value::String(new);
                lines[position] = format!("{annotation}{}", serde_json::to_string(&value)?);
            }
        }
        Ok(lines.join("\n"))
    }

    /// Whether the tree holds any target.
    fn holds(&self, tree: &str) -> Result<bool> {
        let tree = self
            .gix
            .find_tree(gix::ObjectId::from_hex(tree.as_bytes())?)?;
        for entry in tree.iter() {
            let entry = entry?;
            let name = entry.filename().to_string();
            for target in self.targets {
                if target.matches_root(&name)
                    && self.descends(entry.object_id(), &target.relative)?
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn descends(&self, tree: gix::ObjectId, relative: &[String]) -> Result<bool> {
        let Some((first, rest)) = relative.split_first() else {
            return Ok(true);
        };
        let tree = self.gix.find_tree(tree)?;
        for entry in tree.iter() {
            let entry = entry?;
            if entry.filename() == first.as_str() {
                return if rest.is_empty() {
                    Ok(true)
                } else if entry.mode().is_tree() {
                    self.descends(entry.object_id(), rest)
                } else {
                    Ok(false)
                };
            }
        }
        Ok(false)
    }

    fn strip_tree(&mut self, tree: &str) -> Result<String> {
        if let Some(done) = self.stripped.get(tree) {
            return Ok(done.clone());
        }
        let mut current = tree.to_string();
        for index in 0..self.targets.len() {
            if let Some(new) = self.strip_one(index, &current, 0)? {
                current = new;
            }
        }
        self.stripped.insert(tree.to_string(), current.clone());
        Ok(current)
    }

    /// The tree without the target, or `None` when it holds none.
    fn strip_one(&mut self, target: usize, tree: &str, depth: usize) -> Result<Option<String>> {
        let key = (target, depth, tree.to_string());
        if depth > 0
            && let Some(cached) = self.tree_cache.get(&key)
        {
            return Ok(cached.clone());
        }
        let gix = self.gix.clone();
        let object = gix.find_tree(gix::ObjectId::from_hex(tree.as_bytes())?)?;
        let mut listing: Vec<u8> = vec![];
        let mut changed = false;
        // `depth` 0 is the tree root, whose name is the root or its variant
        let relative_len = self.targets[target].relative.len();
        for entry in object.iter() {
            let entry = entry?;
            let name = entry.filename().to_vec();
            let text = String::from_utf8_lossy(&name).into_owned();
            let hit = if depth == 0 {
                self.targets[target].matches_root(&text)
            } else {
                text == self.targets[target].relative[depth - 1]
            };
            let is_tree = entry.mode().is_tree();
            let mut keep = Some(entry.object_id().to_string());
            if hit {
                if depth > 0 && depth == relative_len {
                    keep = None;
                    changed = true;
                } else if is_tree {
                    let child = entry.object_id().to_string();
                    if let Some(new) = self.strip_one(target, &child, depth + 1)? {
                        changed = true;
                        keep = (new != self.empty_tree).then_some(new);
                    }
                }
            }
            if let Some(oid) = keep {
                listing.extend_from_slice(
                    format!(
                        "{:06o} {} {}\t",
                        entry.mode().value(),
                        kind(entry.mode()),
                        oid
                    )
                    .as_bytes(),
                );
                listing.extend_from_slice(&name);
                listing.push(0);
            }
        }
        let result = if changed {
            Some(self.repo.mktree_z(&listing)?)
        } else {
            None
        };
        if depth > 0 {
            self.tree_cache.insert(key, result.clone());
        }
        Ok(result)
    }

    /// The manifest of a stripped tree, without the purged paths' modes.
    fn strip_permissions(&self, tree: &str) -> Result<String> {
        let object = self
            .gix
            .find_tree(gix::ObjectId::from_hex(tree.as_bytes())?)?;
        let Some(mut manifest) = Manifest::read_gix(&object)? else {
            return Ok(tree.to_string());
        };
        let before = manifest.permissions.len();
        let targets = self.targets;
        manifest
            .permissions
            .retain(|path, _| !targets.iter().any(|target| target.covers_branch_path(path)));
        if manifest.permissions.len() == before {
            return Ok(tree.to_string());
        }
        let blob = self
            .repo
            .hash_blob(&serde_json::to_vec_pretty(&manifest)?)?;
        self.repo.compose(
            tree,
            &[Overlay {
                path: manifest::PATH.into(),
                object: Some(("100644".into(), blob)),
            }],
        )
    }
}

fn kind(mode: gix::object::tree::EntryMode) -> &'static str {
    if mode.is_tree() {
        "tree"
    } else if mode.is_commit() {
        "commit"
    } else {
        "blob"
    }
}

fn signature(signature: &gix::actor::SignatureRef<'_>) -> [String; 3] {
    [
        signature.name.to_string(),
        signature.email.to_string(),
        signature.time.to_string(),
    ]
}

fn annotation_target(message: &str) -> Option<String> {
    let (target, _) = HistoryRepo::annotation_from_message(message).ok()??;
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(root: &str, relative: &[&str]) -> Target {
        Target {
            display: String::new(),
            root: root.into(),
            relative: relative.iter().map(|part| part.to_string()).collect(),
        }
    }

    #[test]
    fn a_whole_root_is_refused() {
        assert!(Target::parse("~").is_err());
    }

    #[test]
    fn a_target_covers_its_variants_and_its_descendants() {
        let target = target("home", &[".aws"]);
        assert!(target.covers_branch_path("home/.aws"));
        assert!(target.covers_branch_path("home/.aws/credentials"));
        assert!(target.covers_branch_path("home@work/.aws/credentials"));
        assert!(!target.covers_branch_path("home/.awsome"));
        assert!(!target.covers_branch_path("home/.config/.aws"));
        assert!(!target.covers_branch_path("config/.aws"));
        assert!(!target.covers_branch_path("homework/.aws"));
    }
}
