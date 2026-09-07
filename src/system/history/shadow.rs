//! The bare repository mise owns under `$MISE_STATE_DIR/history/repo.git`.
//!
//! Tracked-file versions form ordinary parented commits on the local branch.
//! The files are the commit tree itself, not a snapshot wrapper. Boundary
//! metadata is recorded in a commit-message trailer, never a recovery tree.
//!
//! Files are captured as raw blobs under a scratch index from literal
//! pathspecs mise's own walker produced: the root's `.gitignore` is bypassed
//! on purpose (an ignored file is often exactly the secret a rollback must
//! restore), git never indexes a `.git` component so the user's own
//! repositories are untouched, and nested repositories become gitlinks
//! rather than content. The user's checkouts and indexes are never touched.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use super::store::{Checkpoint, RootRecord, repo_dir_in};
use crate::file::display_path;
use crate::git::{GitPlumbing, PlumbingCall};

/// Regular files above this size are left out of a snapshot.
pub(crate) const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// An entry with more files than this is cut short.
pub(crate) const MAX_FILES: u64 = 100_000;
/// An entry with more bytes than this is cut short.
pub(crate) const MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// One top-level root of a snapshot tree and the files to add under it,
/// relative to `path`.
#[derive(Clone, Debug)]
pub(crate) struct CaptureRoot {
    pub label: String,
    pub path: PathBuf,
    pub files: Vec<PathBuf>,
    pub bytes: u64,
}

#[derive(Debug)]
pub(crate) struct CaptureResult {
    /// The snapshot tree (empty tree when nothing was captured).
    pub tree: String,
    pub roots: Vec<RootRecord>,
    pub warnings: Vec<String>,
    /// Files which were discovered but could not be read. They are not deletions.
    pub omitted: Vec<super::store::PathReason>,
}

#[derive(Debug)]
pub(crate) struct TreeEntry {
    pub mode: String,
    pub oid: String,
    pub size: Option<u64>,
    pub path: String,
}

#[derive(Debug, Default)]
pub(crate) struct DiffOpts {
    /// Full patch instead of a per-file summary.
    pub patch: bool,
    /// Write the diff to the terminal as git produces it instead of
    /// returning it: `output` comes back empty.
    pub stream: bool,
    pub color: bool,
    /// Restrict the comparison to a path inside each tree.
    pub paths: Option<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct DiffResult {
    pub output: Vec<u8>,
    pub changed: bool,
}

/// One path replaced inside a composed tree.
#[derive(Clone, Debug)]
pub(crate) struct Overlay {
    /// Path inside the tree (`home/.config/app/state.json`).
    pub path: String,
    /// `(mode, oid)` to put there, or `None` to leave the path absent.
    pub object: Option<(String, String)>,
}

/// One changed path between two snapshot trees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Change {
    /// `A`, `M`, `D`, or `T`.
    pub status: char,
    /// Path inside the snapshot tree (`home/.zshrc`).
    pub path: String,
}

#[derive(Debug)]
pub(crate) struct HistoryRepo {
    git: GitPlumbing,
    /// Comparison and decrypted bytes live only for this process. They must
    /// never become objects merely because a preflight or diff read them.
    transient: std::sync::Mutex<TransientObjects>,
}

#[derive(Default)]
struct TransientObjects {
    blobs: BTreeMap<String, Vec<u8>>,
    decrypted: BTreeMap<String, (String, String)>,
}

impl std::fmt::Debug for TransientObjects {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransientObjects")
            .field("blob_count", &self.blobs.len())
            .field("decrypted_count", &self.decrypted.len())
            .finish_non_exhaustive()
    }
}

/// A replaceable index, not history: enough to reuse randomized ciphertext
/// when a live file and its recipients have not changed. Never published.
#[derive(Deserialize, Serialize)]
struct EncryptionCacheEntry {
    fingerprint: String,
    mode: String,
    scheme: String,
    oid: String,
}

/// Keep content fingerprints useful for cache lookup without publishing a
/// dictionary-testable hash of a secret in the disposable cache.
fn encryption_cache_key(dir: &Path) -> Result<[u8; 32]> {
    use std::io::Write;

    super::store::create_private_dir(dir)?;
    let path = dir.join("encryption-cache.key");
    match std::fs::read(&path) {
        Ok(bytes) => bytes
            .try_into()
            .map_err(|_| eyre::eyre!("invalid encryption cache key: {}", display_path(&path))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let key: [u8; 32] = rand::random();
            let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
            temporary.write_all(&key)?;
            temporary.as_file().sync_all()?;
            match temporary.persist_noclobber(&path) {
                Ok(_) => Ok(key),
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    encryption_cache_key(dir)
                }
                Err(error) => Err(error.error.into()),
            }
        }
        Err(error) => Err(error.into()),
    }
}

impl HistoryRepo {
    pub(crate) const HISTORY_REF: &'static str = "refs/heads/main";
    const RECORD_TRAILER: &'static str = "Mise-History: ";
    pub(crate) fn path_in(state_dir: &Path) -> PathBuf {
        repo_dir_in(state_dir)
    }

    /// Opens the repository, creating it on first use. `Ok(None)` means no
    /// git binary mise is willing to run is available.
    pub(crate) fn open_or_init_in(state_dir: &Path) -> Result<Option<Self>> {
        if crate::git::plumbing_binary().is_none() {
            return Ok(None);
        }
        let git = GitPlumbing::new(Self::path_in(state_dir));
        git.init_bare()
            .wrap_err_with(|| format!("initializing {}", display_path(git.git_dir())))?;
        // the binary may exist and still be unusable (a stub, a broken
        // install): probe it once so callers can say capture is unavailable
        git.run(PlumbingCall::new(["rev-parse", "--is-bare-repository"]))
            .wrap_err_with(|| format!("opening {}", display_path(git.git_dir())))?;
        let repo = Self {
            git,
            transient: Default::default(),
        };
        if !repo.list_refs("refs/checkpoints/")?.is_empty() {
            bail!(
                "this store uses the old unreleased checkpoint format; use a separate MISE_STATE_DIR for the new history format; existing data has not been changed"
            );
        }
        Ok(Some(repo))
    }

    pub(crate) fn dir(&self) -> &Path {
        self.git.git_dir()
    }

    pub(crate) fn capture_tracked(
        &self,
        walk: &super::tracked::Walk,
        recipient_strings: &[String],
        interactive: bool,
    ) -> Result<CaptureResult> {
        let mut manifest = walk.manifest.clone();
        manifest.recipients = recipient_strings.to_vec();
        if manifest.recipients.is_empty()
            && manifest.enrollment.iter().any(|entry| entry.encrypt)
            && let Some(head) = self.ref_oid(Self::HISTORY_REF)?
            && let Some(previous) = super::manifest::Manifest::read(self, &head)?
        {
            manifest.recipients = previous.recipients;
        }
        manifest.recipients.sort();
        manifest.recipients.dedup();
        let mut result = self.capture_tracked_files(walk, &manifest.recipients, interactive)?;
        result.tree = manifest.preserve_other_files(self, &result.tree)?;
        result.tree = manifest.write(self, &result.tree)?;
        Ok(result)
    }

    fn capture_tracked_files(
        &self,
        walk: &super::tracked::Walk,
        recipient_strings: &[String],
        interactive: bool,
    ) -> Result<CaptureResult> {
        if !walk.files.values().any(|(_, policy)| policy.encrypt) {
            return self.capture(&walk.roots);
        }
        if recipient_strings.is_empty() {
            bail!(
                "encrypted tracked files require [history.encryption].recipients; nothing was committed"
            );
        }
        let mut normalized = recipient_strings.to_vec();
        normalized.sort();
        normalized.dedup();
        let scheme = crate::hash::hash_sha256_to_str(&normalized.join("\n"));
        let cache_path = self.dir().parent().unwrap().join("index/encryption.json");
        let cache_key = encryption_cache_key(self.dir().parent().unwrap())?;
        let mut cache: BTreeMap<String, EncryptionCacheEntry> = std::fs::read(&cache_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut roots = walk.roots.clone();
        let mut overlays = vec![];
        let mut recipients = None;
        for root in &mut roots {
            let encrypted: Vec<_> = root
                .files
                .iter()
                .filter(|rel| {
                    walk.files
                        .get(&root.path.join(rel))
                        .is_some_and(|(_, policy)| policy.encrypt)
                })
                .cloned()
                .collect();
            root.files.retain(|rel| !encrypted.contains(rel));
            for rel in encrypted {
                let live = root.path.join(&rel);
                let metadata = std::fs::symlink_metadata(&live)?;
                let (mode, bytes) = if metadata.file_type().is_symlink() {
                    (
                        "120000",
                        path_bytes(&std::fs::read_link(&live)?).into_owned(),
                    )
                } else if metadata.is_file() {
                    #[cfg(unix)]
                    let executable = {
                        use std::os::unix::fs::PermissionsExt;
                        metadata.permissions().mode() & 0o100 != 0
                    };
                    #[cfg(not(unix))]
                    let executable = false;
                    let bytes = crate::agecrypt::read_bounded(
                        std::fs::File::open(&live)?,
                        crate::agecrypt::MAX_PLAINTEXT_BYTES,
                    )?;
                    (if executable { "100755" } else { "100644" }, bytes)
                } else {
                    bail!("cannot encrypt non-file {}", display_path(&live));
                };
                let path = format!(
                    "{}/{}",
                    root.label,
                    rel.to_str()
                        .ok_or_else(|| eyre::eyre!("non-UTF-8 tracked path"))?
                        .replace('\\', "/")
                );
                let fingerprint = blake3::keyed_hash(&cache_key, &bytes).to_hex().to_string();
                let cached = cache.get(&path).filter(|entry| {
                    entry.fingerprint == fingerprint && entry.mode == mode && entry.scheme == scheme
                });
                let oid = match cached {
                    Some(entry)
                        if self
                            .blob_starts_with(&entry.oid, b"mise-encrypted-file-v1\n")
                            .unwrap_or(false) =>
                    {
                        entry.oid.clone()
                    }
                    _ => {
                        let recipients = match &recipients {
                            Some(recipients) => recipients,
                            None => recipients.insert(
                                normalized
                                    .iter()
                                    .map(|recipient| {
                                        crate::agecrypt::parse_recipient_mode(
                                            recipient,
                                            interactive,
                                        )?
                                        .ok_or_else(
                                            || eyre::eyre!("invalid encrypted-file recipient"),
                                        )
                                    })
                                    .collect::<Result<Vec<_>>>()?,
                            ),
                        };
                        let encoded =
                            super::sync::files::encode(&path, mode, &bytes, &scheme, recipients)?;
                        let oid = self.hash_blob(&encoded)?;
                        cache.insert(
                            path.clone(),
                            EncryptionCacheEntry {
                                fingerprint,
                                mode: mode.into(),
                                scheme: scheme.clone(),
                                oid: oid.clone(),
                            },
                        );
                        oid
                    }
                };
                overlays.push(Overlay {
                    path,
                    object: Some(("100644".into(), oid)),
                });
            }
        }
        // No encrypted input is ever handed to git add or hash-object.
        let mut captured = self.capture(&roots)?;
        captured.tree = self.compose(&captured.tree, &overlays)?;
        captured.roots = walk
            .roots
            .iter()
            .map(|root| RootRecord {
                label: root.label.clone(),
                path: root.path.clone(),
                files: root.files.len() as u64,
                bytes: root.bytes,
            })
            .collect();
        super::store::create_private_dir(cache_path.parent().unwrap())?;
        // NamedTempFile starts private, including when replacing an older cache
        // with broader permissions. Never expose fingerprints during the write.
        let mut temporary = tempfile::NamedTempFile::new_in(cache_path.parent().unwrap())?;
        serde_json::to_writer(temporary.as_file_mut(), &cache)?;
        temporary.as_file().sync_all()?;
        crate::file::persist_atomic(temporary, &cache_path)?;
        Ok(captured)
    }

    /// Builds the snapshot tree for `roots`: one subtree per root holding
    /// exactly the listed files.
    pub(crate) fn capture(&self, roots: &[CaptureRoot]) -> Result<CaptureResult> {
        let mut omitted = vec![];
        let mut entries: Vec<(String, String)> = vec![]; // (oid, name)
        let mut records = vec![];
        for root in roots {
            let record = RootRecord {
                label: root.label.clone(),
                path: root.path.clone(),
                files: root.files.len() as u64,
                bytes: root.bytes,
            };
            if root.files.is_empty() {
                records.push(record);
                continue;
            }
            let tree = self
                .tree_for_root(root, &mut omitted)
                .wrap_err_with(|| format!("snapshotting {}", display_path(&root.path)))?;
            entries.push((tree, root.label.clone()));
            records.push(record);
        }
        entries.sort_by(|a, b| a.1.cmp(&b.1));
        let listing = entries
            .iter()
            .map(|(oid, name)| format!("040000 tree {oid}\t{name}\n"))
            .collect::<String>();
        let tree = self.mktree(&listing)?;
        Ok(CaptureResult {
            tree,
            roots: records,
            warnings: omitted
                .iter()
                .map(|failure| format!("{} was not captured: {}", failure.path, failure.reason))
                .collect(),
            omitted,
        })
    }

    /// Adds a root's files under a scratch index and returns the tree id.
    fn tree_for_root(
        &self,
        root: &CaptureRoot,
        omitted: &mut Vec<super::store::PathReason>,
    ) -> Result<String> {
        let mut overlays = vec![];
        for rel in &root.files {
            let live = root.path.join(rel);
            let captured = (|| -> Result<Overlay> {
                let meta = std::fs::symlink_metadata(&live)?;
                let (mode, oid) = if meta.file_type().is_symlink() {
                    (
                        "120000",
                        self.hash_blob(path_bytes(&std::fs::read_link(&live)?).as_ref())?,
                    )
                } else if meta.is_dir() {
                    ("160000", crate::git::Git::new(&live).current_sha()?)
                } else if meta.is_file() {
                    #[cfg(unix)]
                    let executable = {
                        use std::os::unix::fs::PermissionsExt;
                        meta.permissions().mode() & 0o100 != 0
                    };
                    #[cfg(not(unix))]
                    let executable = false;
                    let bytes = crate::agecrypt::read_bounded(
                        std::fs::File::open(&live)?,
                        crate::agecrypt::MAX_PLAINTEXT_BYTES,
                    )?;
                    (
                        if executable { "100755" } else { "100644" },
                        self.hash_blob(&bytes)?,
                    )
                } else {
                    bail!("cannot capture non-file {}", display_path(&live));
                };
                Ok(Overlay {
                    path: rel
                        .to_str()
                        .ok_or_else(|| eyre::eyre!("non-UTF-8 tracked path"))?
                        .replace('\\', "/"),
                    object: Some((mode.into(), oid)),
                })
            })();
            match captured {
                Ok(overlay) => overlays.push(overlay),
                Err(error) => omitted.push(super::store::PathReason {
                    path: display_path(&live),
                    reason: format!("unreadable: {error:#}"),
                }),
            }
        }
        self.compose(&self.empty_object("tree")?, &overlays)
    }

    /// One level of tree from an `ls-tree`-style listing.
    pub(crate) fn mktree(&self, listing: &str) -> Result<String> {
        self.output_str(PlumbingCall::new(["mktree"]).stdin(listing.as_bytes()))
    }

    /// A tree holding exactly `entries` (`(mode, oid, path)`, paths nested
    /// as deep as they like), built in a scratch index so the repository's
    /// own index is never touched. Objects need not exist for gitlinks
    /// (`160000`), as in any tree git writes.
    #[cfg(test)]
    pub(crate) fn write_tree(&self, entries: &[(String, String, String)]) -> Result<String> {
        if entries.is_empty() {
            return self.empty_object("tree");
        }
        let index = self
            .dir()
            .join(format!("mise-index-{}-write-tree", std::process::id()));
        let _ = std::fs::remove_file(&index);
        let mut info: Vec<u8> = vec![];
        for (mode, oid, path) in entries {
            info.extend_from_slice(format!("{mode} {oid}\t{path}").as_bytes());
            info.push(0);
        }
        let result = (|| -> Result<String> {
            // index-only operations; git still insists on a work tree
            self.git.run(
                PlumbingCall::new(["update-index", "-z", "--index-info"])
                    .work_tree(self.dir())
                    .index_file(&index)
                    .stdin(&info),
            )?;
            self.output_str(PlumbingCall::new(["write-tree"]).index_file(&index))
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    /// Writes the wrapper commit for a checkpoint: `snapshot/`, `meta.json`,
    /// and `blobs/<sha256>` for every referenced journal blob.
    pub(crate) fn write_checkpoint(
        &self,
        snapshot_tree: Option<&str>,
        checkpoint: &Checkpoint,
        blobs: &BTreeMap<String, String>,
    ) -> Result<String> {
        let commit = self.write_checkpoint_commit(snapshot_tree, checkpoint, blobs)?;
        self.advance_head(&commit)?;
        Ok(commit)
    }

    /// Create an ordinary child commit. The caller advances the branch with
    /// an expected-head check; recovery blobs never enter this tree.
    pub(crate) fn write_checkpoint_commit(
        &self,
        snapshot_tree: Option<&str>,
        checkpoint: &Checkpoint,
        _blobs: &BTreeMap<String, String>,
    ) -> Result<String> {
        let tree = match snapshot_tree {
            Some(tree) => tree.to_owned(),
            None => match self.ref_oid(Self::HISTORY_REF)? {
                Some(head) => self.output_tree_of(&head)?,
                None => self.empty_object("tree")?,
            },
        };
        let record = checkpoint.for_commit();
        let message = format!(
            "{}\n\n{}{}",
            checkpoint.description,
            Self::RECORD_TRAILER,
            serde_json::to_string(&record)?
        );
        let parent = self.ref_oid(Self::HISTORY_REF)?;
        self.commit_tree(&tree, parent.as_deref().into_iter().collect(), &message)
    }

    fn advance_head(&self, commit: &str) -> Result<()> {
        // The commit's parent is the head observed when it was prepared.
        // A concurrent writer must never have its commit overwritten.
        let parents = self.output_str(PlumbingCall::new(["show", "-s", "--format=%P", commit]))?;
        let expected = parents.split_whitespace().next();
        self.update_history_head(commit, expected)
    }

    pub(crate) fn update_history_head(&self, commit: &str, expected: Option<&str>) -> Result<()> {
        self.update_ref(Self::HISTORY_REF, commit, expected)?;
        self.git.run(PlumbingCall::new([
            "symbolic-ref",
            "HEAD",
            Self::HISTORY_REF,
        ]))
    }

    /// Ordinary ancestry is the source of truth, not per-checkpoint refs.
    pub(crate) fn checkpoint_refs(&self) -> Result<Vec<(String, String)>> {
        let Some(head) = self.ref_oid(Self::HISTORY_REF)? else {
            return Ok(vec![]);
        };
        self.rev_list(&head, usize::MAX)?
            .into_iter()
            .map(|commit| {
                let record = self.read_meta(&commit)?;
                Ok((record.uuid, commit))
            })
            .collect()
    }

    pub(crate) fn read_meta(&self, commit: &str) -> Result<Checkpoint> {
        let manifest = super::manifest::Manifest::read(self, commit)?;
        let message = self.output_str(PlumbingCall::new(["show", "-s", "--format=%B", commit]))?;
        let own_record = message
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix(Self::RECORD_TRAILER));
        let mut record = Checkpoint {
            schema_version: super::store::SCHEMA_VERSION,
            uuid: commit.into(),
            machine: super::store::Machine {
                id: "git".into(),
                name: "git".into(),
            },
            created_at: String::new(),
            mise_version: String::new(),
            trigger: super::store::Trigger::Edit,
            description: String::new(),
            description_source: super::store::DescriptionSource::User,
            summary: String::new(),
            task: None,
            labels: vec![],
            pinned: false,
            tree: super::store::TreeInfo {
                snapshot: None,
                available: true,
                reason: None,
                roots: vec![],
                coverage: Default::default(),
                modes: Default::default(),
            },
            changes: Default::default(),
            operation: None,
        };
        record.created_at =
            self.output_str(PlumbingCall::new(["show", "-s", "--format=%cI", commit]))?;
        record.description = message
            .lines()
            .next()
            .unwrap_or("edited tracked files")
            .into();
        record.summary = record.description.clone();
        if let Some(trailer) = own_record {
            let metadata: super::store::CommitRecord = serde_json::from_str(trailer)
                .wrap_err_with(|| format!("reading history metadata for {commit}"))?;
            record.trigger = metadata.trigger;
            record.description_source = metadata.description_source;
            record.task = metadata.task;
            record.labels = metadata.labels;
            record.pinned = metadata.pinned;
            let missing = |path: String| super::store::PathReason {
                path: super::tracked::tree_path_to_display(&path),
                reason: "not captured in this commit".into(),
            };
            record.tree.coverage.omitted = metadata.omitted.into_iter().map(missing).collect();
            record.tree.coverage.incomplete =
                metadata.incomplete.into_iter().map(missing).collect();
            record.operation = metadata.operation.map(|op| op.localize()).transpose()?;
        }
        record.tree.snapshot = Some(self.output_tree_of(commit)?);
        record.changes = Default::default();
        let parents = self.output_str(PlumbingCall::new(["show", "-s", "--format=%P", commit]))?;
        record.changes.since = parents.split_whitespace().next().map(str::to_owned);
        for change in self.changes(parents.split_whitespace().next(), commit)? {
            if change.path.starts_with(".mise-history/") {
                continue;
            }
            let path = super::tracked::tree_path_to_display(&change.path);
            match change.status {
                'A' => record.changes.added.push(path),
                'D' => record.changes.removed.push(path),
                _ => record.changes.modified.push(path),
            }
        }
        // The commit tree is authoritative even if a normal Git operation
        // reused a message containing metadata from an older commit.
        record.tree.snapshot = Some(self.output_tree_of(commit)?);
        record.tree.available = true;
        if let Some(manifest) = manifest {
            let previous = record
                .changes
                .since
                .as_deref()
                .map(|parent| super::manifest::Manifest::read(self, parent))
                .transpose()?
                .flatten()
                .unwrap_or_default();
            for path in manifest
                .permissions
                .keys()
                .chain(previous.permissions.keys())
                .collect::<BTreeSet<_>>()
            {
                if manifest.permissions.get(path) != previous.permissions.get(path) {
                    let path = super::tracked::tree_path_to_display(path);
                    if !record.changes.touches(&path) {
                        record.changes.modified.push(path);
                    }
                }
            }
            let tracked = manifest.tracking()?;
            record.tree.modes.clear();
            let layout = super::sync::layout::Roots::current();
            for (portable, bits) in &manifest.permissions {
                if let Some(path) = layout.locate(portable).path()
                    && tracked.entries.iter().any(|entry| {
                        path.starts_with(&entry.path)
                            && entry
                                .tree_path(path)
                                .is_ok_and(|mapped| mapped == *portable)
                    })
                {
                    record
                        .tree
                        .modes
                        .insert(crate::file::display_path(path), *bits);
                }
            }
            let mut coverage = tracked.coverage(&super::tracked::Walk {
                entries: tracked.entries.clone(),
                ..Default::default()
            });
            for entry in &mut coverage.entries {
                entry.state = record
                    .tree
                    .coverage
                    .entries
                    .iter()
                    .find(|prior| prior.path == entry.path && prior.variant == entry.variant)
                    .map_or_else(|| "saved".into(), |prior| prior.state.clone());
            }
            coverage.omitted.append(&mut record.tree.coverage.omitted);
            coverage
                .incomplete
                .append(&mut record.tree.coverage.incomplete);
            record.tree.coverage = coverage;
            let mut roots: BTreeMap<String, RootRecord> = BTreeMap::new();
            let layout = super::sync::layout::Roots::current();
            for file in self.ls_tree(commit)? {
                if layout.locate(&file.path).path().is_none() {
                    continue;
                }
                let label = file.path.split('/').next().unwrap_or_default().to_string();
                let root = roots.entry(label.clone()).or_insert_with(|| RootRecord {
                    label,
                    ..Default::default()
                });
                root.files += 1;
                root.bytes += file.size.unwrap_or_default();
            }
            record.tree.roots = roots.into_values().collect();
        }
        for root in &mut record.tree.roots {
            root.path = match root.label.split('@').next().unwrap_or_default() {
                "home" => crate::dirs::HOME.to_path_buf(),
                "config" => super::tracked::global_config_dir(),
                "fs" => PathBuf::from(std::path::MAIN_SEPARATOR.to_string()),
                other => bail!("unknown history path root: {other}"),
            };
        }
        Ok(record)
    }

    /// Recursive listing of a tree (or a path inside it).
    pub(crate) fn ls_tree(&self, spec: &str) -> Result<Vec<TreeEntry>> {
        let out = self
            .git
            .output(PlumbingCall::new(["ls-tree", "-r", "-l", "-z", spec]))?;
        let mut entries = vec![];
        for record in out.split(|byte| *byte == 0) {
            if record.is_empty() {
                continue;
            }
            let record = std::str::from_utf8(record).wrap_err(
                "history cannot represent a non-UTF-8 filename; refusing to change its bytes",
            )?;
            let Some((meta, path)) = record.split_once('\t') else {
                continue;
            };
            let mut fields = meta.split_whitespace();
            let (Some(mode), Some(_kind), Some(oid), Some(size)) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            entries.push(TreeEntry {
                mode: mode.to_string(),
                oid: oid.to_string(),
                size: size.parse().ok(),
                path: path.to_string(),
            });
        }
        Ok(entries)
    }

    /// The type of the object at `spec`, or `None` when nothing is there.
    pub(crate) fn object_type(&self, spec: &str) -> Result<Option<String>> {
        let output = self
            .git
            .output_unchecked(PlumbingCall::new(["cat-file", "-t", spec]))?;
        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else {
            Ok(None)
        }
    }

    /// An empty object of `kind` (`tree` or `blob`), written so it can stand
    /// in for a side of a diff where a path does not exist.
    pub(crate) fn empty_object(&self, kind: &str) -> Result<String> {
        match kind {
            "tree" => self.mktree(""),
            _ => self.hash_blob(b""),
        }
    }

    pub(crate) fn cat_object(&self, oid: &str) -> Result<Vec<u8>> {
        if let Some(bytes) = self.transient_blob(oid) {
            return Ok(bytes);
        }
        self.git
            .output(PlumbingCall::new(["cat-file", "blob", oid]))
            .wrap_err_with(|| format!("reading {oid}"))
    }

    pub(crate) fn blob_starts_with(&self, oid: &str, prefix: &[u8]) -> Result<bool> {
        if let Some(bytes) = self.transient_blob(oid) {
            return Ok(bytes.starts_with(prefix));
        }
        self.git.blob_starts_with(oid, prefix)
    }

    pub(crate) fn cat_object_bounded(&self, oid: &str, limit: u64) -> Result<Vec<u8>> {
        if let Some(bytes) = self.transient_blob(oid) {
            if bytes.len() as u64 > limit {
                bail!("object exceeds the encrypted content size limit");
            }
            return Ok(bytes);
        }
        let size: u64 = self
            .output_str(PlumbingCall::new(["cat-file", "-s", oid]))?
            .trim()
            .parse()?;
        if size > limit {
            eyre::bail!("object exceeds the encrypted content size limit");
        }
        self.cat_object(oid)
    }

    pub(crate) fn hash_blob(&self, bytes: &[u8]) -> Result<String> {
        self.output_str(PlumbingCall::new(["hash-object", "-w", "--stdin"]).stdin(bytes))
    }

    /// Compute the normal Git identity without writing an object. Read APIs
    /// can resolve it in memory until this repository handle is dropped.
    pub(crate) fn transient_blob_id(&self, bytes: &[u8]) -> Result<String> {
        let oid = self.output_str(PlumbingCall::new(["hash-object", "--stdin"]).stdin(bytes))?;
        self.transient
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .insert(oid.clone(), bytes.to_vec());
        Ok(oid)
    }

    fn transient_blob(&self, oid: &str) -> Option<Vec<u8>> {
        self.transient
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .get(oid)
            .cloned()
    }

    pub(crate) fn decrypted_object(&self, ciphertext: &str) -> Option<(String, String)> {
        self.transient
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .decrypted
            .get(ciphertext)
            .cloned()
    }

    pub(crate) fn remember_decrypted(&self, ciphertext: &str, object: (String, String)) {
        self.transient
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .decrypted
            .insert(ciphertext.into(), object);
    }

    /// Compares two trees, from `a` to `b`. With `paths`, a path that exists
    /// on only one side is compared against an empty tree or blob so an
    /// added or removed path shows up as its whole contents.
    pub(crate) fn diff(&self, a: &str, b: &str, opts: &DiffOpts) -> Result<DiffResult> {
        if !super::sync::files::encrypted_paths(self, Some(a))?.is_empty()
            || !super::sync::files::encrypted_paths(self, Some(b))?.is_empty()
        {
            return self.decrypted_diff(a, b, opts);
        }
        let (from, to) = match &opts.paths {
            Some((from_path, to_path)) => {
                let from = format!("{a}:{from_path}");
                let to = format!("{b}:{to_path}");
                match (self.object_type(&from)?, self.object_type(&to)?) {
                    (Some(_), Some(_)) => (from, to),
                    (Some(kind), None) => (from, self.empty_object(&kind)?),
                    (None, Some(kind)) => (self.empty_object(&kind)?, to),
                    (None, None) => {
                        let path = if from_path == to_path {
                            from_path.clone()
                        } else {
                            format!("{from_path} / {to_path}")
                        };
                        bail!("{path} is not in either snapshot");
                    }
                }
            }
            None => (a.to_string(), b.to_string()),
        };
        let call = PlumbingCall::new([
            "diff",
            "--no-ext-diff",
            "--exit-code",
            if opts.patch { "--patch" } else { "--stat" },
            if opts.color {
                "--color=always"
            } else {
                "--color=never"
            },
            &from,
            &to,
        ]);
        if opts.stream {
            let status = self.git.status_inherited(call)?;
            return match status.code() {
                Some(0) => Ok(DiffResult {
                    output: vec![],
                    changed: false,
                }),
                Some(1) => Ok(DiffResult {
                    output: vec![],
                    changed: true,
                }),
                _ => bail!("git diff failed ({status})"),
            };
        }
        let output = self.git.output_unchecked(call)?;
        match output.status.code() {
            Some(0) => Ok(DiffResult {
                output: output.stdout,
                changed: false,
            }),
            Some(1) => Ok(DiffResult {
                output: output.stdout,
                changed: true,
            }),
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("git diff failed ({}): {}", output.status, stderr.trim())
            }
        }
    }

    /// Decrypt only for this requested comparison. Git's no-index diff sees
    /// private temporary files, never plaintext objects in the repository.
    fn decrypted_diff(&self, a: &str, b: &str, opts: &DiffOpts) -> Result<DiffResult> {
        use std::io::Write;
        let mut paths = BTreeSet::new();
        for (tree, prefix) in [
            (a, opts.paths.as_ref().map(|p| p.0.as_str())),
            (b, opts.paths.as_ref().map(|p| p.1.as_str())),
        ] {
            for entry in self.ls_tree(tree)? {
                if entry.path.starts_with(".mise-history/") {
                    continue;
                }
                if let Some(prefix) = prefix {
                    if entry.path == prefix {
                        paths.insert(String::new());
                    } else if let Some(relative) = entry.path.strip_prefix(&format!("{prefix}/")) {
                        paths.insert(relative.to_string());
                    }
                } else {
                    paths.insert(entry.path);
                }
            }
        }
        if paths.is_empty() && opts.paths.is_some() {
            bail!("path is not in either snapshot");
        }
        let mut output = vec![];
        let mut changed = false;
        for relative in paths {
            let path = |prefix: &str| {
                if relative.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{prefix}/{relative}")
                }
            };
            let (left_path, right_path) = opts.paths.as_ref().map_or_else(
                || (relative.clone(), relative.clone()),
                |(left, right)| (path(left), path(right)),
            );
            let left = self.restored_object_at(a, &left_path)?;
            let right = self.restored_object_at(b, &right_path)?;
            if left == right {
                continue;
            }
            changed = true;
            if !opts.patch {
                output.extend_from_slice(format!("{right_path} | changed\n").as_bytes());
                continue;
            }
            let mut left_file = tempfile::NamedTempFile::new()?;
            let mut right_file = tempfile::NamedTempFile::new()?;
            if let Some((mode, oid)) = &left
                && mode != "160000"
            {
                left_file.write_all(&self.cat_object(oid)?)?;
            }
            if let Some((mode, oid)) = &right
                && mode != "160000"
            {
                right_file.write_all(&self.cat_object(oid)?)?;
            }
            let left_name = left_file.path().to_string_lossy().to_string();
            let right_name = right_file.path().to_string_lossy().to_string();
            let diff = self.git.output_unchecked(PlumbingCall::new([
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--no-textconv",
                "--patch",
                "--color=never",
                "--",
                &left_name,
                &right_name,
            ]))?;
            if !matches!(diff.status.code(), Some(0 | 1)) {
                bail!("git diff failed: {}", String::from_utf8_lossy(&diff.stderr));
            }
            let text = String::from_utf8_lossy(&diff.stdout)
                .replace(left_name.trim_start_matches('/'), &left_path)
                .replace(right_name.trim_start_matches('/'), &right_path);
            output.extend_from_slice(text.as_bytes());
            if left.as_ref().map(|v| &v.0) != right.as_ref().map(|v| &v.0) {
                output.extend_from_slice(
                    format!(
                        "{right_path}: mode {} -> {}\n",
                        left.as_ref().map_or("absent", |v| v.0.as_str()),
                        right.as_ref().map_or("absent", |v| v.0.as_str())
                    )
                    .as_bytes(),
                );
            }
        }
        Ok(DiffResult { output, changed })
    }

    /// The paths that differ between two snapshot trees (`None` = the empty
    /// tree), without rename detection so a rename reads as remove + add.
    pub(crate) fn changes(&self, from: Option<&str>, to: &str) -> Result<Vec<Change>> {
        let empty;
        let from = match from {
            Some(tree) => tree,
            None => {
                empty = self.empty_object("tree")?;
                &empty
            }
        };
        let out = self.git.output(PlumbingCall::new([
            "diff-tree",
            "-r",
            "-z",
            "--name-status",
            "--no-renames",
            from,
            to,
        ]))?;
        let mut fields = out.split(|byte| *byte == 0);
        let mut changes = vec![];
        while let Some(status) = fields.next() {
            if status.is_empty() {
                continue;
            }
            let Some(path) = fields.next() else {
                break;
            };
            let status = String::from_utf8_lossy(status);
            let Some(status) = status.chars().next() else {
                continue;
            };
            changes.push(Change {
                status,
                path: std::str::from_utf8(path).wrap_err("history cannot represent a non-UTF-8 filename; refusing to change its bytes")?.to_string(),
            });
        }
        Ok(changes)
    }

    /// The tree of a commit.
    pub(crate) fn output_tree_of(&self, commit: &str) -> Result<String> {
        self.output_str(PlumbingCall::new([
            "rev-parse",
            &format!("{commit}^{{tree}}"),
        ]))
    }

    /// The object at `path` inside `tree_ish`: its mode and oid.
    pub(crate) fn object_at(&self, tree_ish: &str, path: &str) -> Result<Option<(String, String)>> {
        let output = self
            .git
            .output_unchecked(PlumbingCall::new(["ls-tree", "-z", tree_ish, "--", path]))?;
        if !output.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let Some(record) = text.split('\0').find(|record| !record.is_empty()) else {
            return Ok(None);
        };
        let Some((meta, _)) = record.split_once('\t') else {
            return Ok(None);
        };
        let mut fields = meta.split_whitespace();
        let (Some(mode), Some(_kind), Some(oid)) = (fields.next(), fields.next(), fields.next())
        else {
            return Ok(None);
        };
        Ok(Some((mode.to_string(), oid.to_string())))
    }

    /// Read live-file contents for restoration. Decrypted objects exist only
    /// in this process; callers must never compose them into a Git tree.
    pub(crate) fn restored_object_at(
        &self,
        tree: &str,
        path: &str,
    ) -> Result<Option<(String, String)>> {
        let Some(object) = self.object_at(tree, path)? else {
            return Ok(None);
        };
        if matches!(object.0.as_str(), "040000" | "160000") {
            return Ok(Some(object));
        }
        let encrypted = super::manifest::Manifest::read(self, tree)?.is_some_and(|manifest| {
            manifest.encrypted_paths().iter().any(|prefix| {
                path == prefix
                    || path
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        });
        if encrypted {
            return super::sync::files::decrypt(
                self,
                path,
                &object,
                console::user_attended_stderr(),
            )
            .map(Some);
        }
        Ok(Some(object))
    }

    /// Builds a tree from `base` with `overlays` applied: each path is
    /// removed from the tree and, when an object is given, replaced by it.
    pub(crate) fn compose(&self, base: &str, overlays: &[Overlay]) -> Result<String> {
        if overlays.is_empty() {
            return Ok(base.to_string());
        }
        let index = self
            .dir()
            .join(format!("mise-index-{}-compose", std::process::id()));
        let _ = std::fs::remove_file(&index);
        let result = (|| -> Result<String> {
            self.git
                .run(PlumbingCall::new(["read-tree", base]).index_file(&index))?;
            for overlay in overlays {
                let listed = self.git.output(
                    PlumbingCall::new(["ls-files", "-z", "--", &overlay.path]).index_file(&index),
                )?;
                let mut removals: Vec<u8> = vec![];
                for entry in listed.split(|byte| *byte == 0) {
                    if entry.is_empty() {
                        continue;
                    }
                    removals.extend_from_slice(entry);
                    removals.push(0);
                }
                // index-only operations; git still insists on a work tree
                if !removals.is_empty() {
                    self.git.run(
                        PlumbingCall::new(["update-index", "--force-remove", "-z", "--stdin"])
                            .work_tree(self.dir())
                            .index_file(&index)
                            .stdin(&removals),
                    )?;
                }
                if let Some((mode, oid)) = &overlay.object {
                    if mode == "040000" {
                        let prefix = format!("--prefix={}/", overlay.path);
                        self.git.run(
                            PlumbingCall::new(["read-tree", &prefix, oid])
                                .work_tree(self.dir())
                                .index_file(&index),
                        )?;
                    } else {
                        let info = format!("{mode},{oid},{}", overlay.path);
                        self.git.run(
                            PlumbingCall::new(["update-index", "--add", "--cacheinfo", &info])
                                .work_tree(self.dir())
                                .index_file(&index),
                        )?;
                    }
                }
            }
            self.output_str(PlumbingCall::new(["write-tree"]).index_file(&index))
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    /// The commit a ref points at, if the ref exists.
    pub(crate) fn ref_oid(&self, name: &str) -> Result<Option<String>> {
        let output = self.git.output_unchecked(PlumbingCall::new([
            "rev-parse",
            "--verify",
            "--quiet",
            name,
        ]))?;
        if !output.status.success() {
            return Ok(None);
        }
        let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!oid.is_empty()).then_some(oid))
    }

    /// Every ref under `prefix` as `(name, oid)`.
    pub(crate) fn list_refs(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let out = self.output_str(PlumbingCall::new([
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            prefix,
        ]))?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let (name, oid) = line.split_once(' ')?;
                Some((name.to_string(), oid.to_string()))
            })
            .collect())
    }

    /// A commit of `tree` with the given parents.
    pub(crate) fn commit_tree(
        &self,
        tree: &str,
        parents: Vec<&str>,
        message: &str,
    ) -> Result<String> {
        let mut args = vec!["commit-tree".to_string(), tree.to_string()];
        for parent in parents {
            args.push("-p".to_string());
            args.push(parent.to_string());
        }
        args.push("-m".to_string());
        args.push(message.to_string());
        self.output_str(PlumbingCall::new(args))
    }

    /// Moves `name` to `commit` only if it still points at `expected`
    /// (`None`: must not exist).
    pub(crate) fn update_ref(
        &self,
        name: &str,
        commit: &str,
        expected: Option<&str>,
    ) -> Result<()> {
        let zero = "0000000000000000000000000000000000000000";
        self.git.run(PlumbingCall::new([
            "update-ref",
            name,
            commit,
            expected.unwrap_or(zero),
        ]))
    }

    /// Deletes a ref if it exists.
    pub(crate) fn delete_ref(&self, name: &str) -> Result<()> {
        if self.ref_oid(name)?.is_some() {
            self.git
                .run(PlumbingCall::new(["update-ref", "-d", name]))?;
        }
        Ok(())
    }

    /// Find all common ancestors without treating Git failures as absence.
    pub(crate) fn merge_bases(&self, local: &str, remote: &str) -> Result<Vec<String>> {
        let output =
            self.git
                .output_unchecked(PlumbingCall::new(["merge-base", "--all", local, remote]))?;
        if output.status.code() == Some(1) {
            return Ok(vec![]);
        }
        if !output.status.success() {
            bail!(
                "cannot determine dotfile Git ancestry: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let text = String::from_utf8(output.stdout)?;
        Ok(text.lines().map(str::to_owned).collect())
    }

    /// Merge complete ordinary trees using Git's recursive merge-base handling.
    /// Conflict trees are never adopted or written into live configuration.
    pub(crate) fn merge_tree(&self, local: &str, remote: &str) -> Result<(String, Vec<String>)> {
        let output = self.git.output_unchecked(PlumbingCall::new([
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "-z",
            local,
            remote,
        ]))?;
        if !output.status.success() && output.status.code() != Some(1) {
            bail!(
                "cannot merge dotfile Git history: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let text = String::from_utf8(output.stdout)?;
        let mut fields = text.split('\0');
        let tree = fields
            .next()
            .filter(|tree| !tree.is_empty())
            .ok_or_else(|| eyre::eyre!("Git did not return a merged tree"))?
            .to_owned();
        let conflicts = fields
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        Ok((tree, conflicts))
    }

    /// Up to `limit` commits reachable from `head`, newest first.
    pub(crate) fn rev_list(&self, head: &str, limit: usize) -> Result<Vec<String>> {
        let count = format!("--max-count={limit}");
        let mut args = vec!["rev-list", "--topo-order"];
        if limit != usize::MAX {
            args.push(&count);
        }
        args.push(head);
        let out = self.output_str(PlumbingCall::new(args))?;
        Ok(out.lines().map(str::to_string).collect())
    }

    /// Three-way merges blob contents; `None` when they conflict or are not
    /// text.
    pub(crate) fn merge3(
        &self,
        base: &[u8],
        ours: &[u8],
        theirs: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        for content in [base, ours, theirs] {
            if content.contains(&0) {
                return Ok(None);
            }
        }
        let dir = tempfile::tempdir()?;
        let (b, o, t) = (
            dir.path().join("base"),
            dir.path().join("ours"),
            dir.path().join("theirs"),
        );
        std::fs::write(&b, base)?;
        std::fs::write(&o, ours)?;
        std::fs::write(&t, theirs)?;
        let output = self.git.output_unchecked(PlumbingCall::new([
            "merge-file",
            "-p",
            "-L",
            "local",
            "-L",
            "base",
            "-L",
            "remote",
            &o.to_string_lossy(),
            &b.to_string_lossy(),
            &t.to_string_lossy(),
        ]))?;
        // exit status is the number of conflicts; negative on error
        match output.status.code() {
            Some(0) => Ok(Some(output.stdout)),
            Some(code) if code > 0 => Ok(None),
            _ => bail!(
                "git merge-file failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }
    }

    /// Runs a network command with the user's git configuration.
    pub(crate) fn network<'a>(
        &self,
        args: impl IntoIterator<Item = &'a str>,
    ) -> Result<std::process::Output> {
        self.git.network_output(PlumbingCall::new(args))
    }

    const ANNOTATION_TRAILER: &'static str = "Mise-Annotation: ";

    /// An annotation is an empty ordinary child commit, so labels and
    /// descriptions travel with the same ancestry without rewriting it.
    pub(crate) fn write_annotation(
        &self,
        target: &str,
        annotation: &super::store::Annotation,
    ) -> Result<()> {
        let head = self
            .ref_oid(Self::HISTORY_REF)?
            .ok_or_else(|| eyre::eyre!("no history to annotate"))?;
        if !self
            .rev_list(&head, usize::MAX)?
            .iter()
            .any(|commit| commit == target)
        {
            bail!("the annotated commit is not in the current history");
        }
        let message = format!(
            "annotate dotfile history\n\n{}{}",
            Self::ANNOTATION_TRAILER,
            serde_json::to_string(&(target, annotation))?
        );
        let commit = self.commit_tree(&self.output_tree_of(&head)?, vec![&head], &message)?;
        self.update_history_head(&commit, Some(&head))
    }

    pub(crate) fn read_annotation(
        &self,
        commit: &str,
    ) -> Result<Option<(String, super::store::Annotation)>> {
        let message = self.output_str(PlumbingCall::new(["show", "-s", "--format=%B", commit]))?;
        message
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix(Self::ANNOTATION_TRAILER))
            .map(|text| serde_json::from_str(text).map_err(Into::into))
            .transpose()
    }

    fn output_str(&self, call: PlumbingCall<'_>) -> Result<String> {
        self.git.output_str(call)
    }
}

#[cfg(unix)]
pub(crate) fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
pub(crate) fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(path.to_string_lossy().replace('\\', "/").into_bytes())
}

pub(crate) fn unavailable_reason() -> String {
    if cfg!(target_os = "macos") && Path::new("/usr/bin/git").exists() {
        "Xcode Command Line Tools are not installed (run `xcode-select --install`)".into()
    } else {
        "git not found".into()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn annotations_are_ordinary_commits_not_parallel_refs() {
        let temp = tempfile::tempdir().unwrap();
        let repo = super::HistoryRepo::open_or_init_in(temp.path())
            .unwrap()
            .unwrap();
        let tree = crate::system::history::manifest::Manifest::default()
            .write(&repo, &repo.empty_object("tree").unwrap())
            .unwrap();
        let original = repo.commit_tree(&tree, vec![], "saved files").unwrap();
        repo.update_history_head(&original, None).unwrap();
        let annotation = crate::system::history::store::Annotation {
            description: Some("configure editor".into()),
            labels: Some(vec!["working".into()]),
            updated_at: crate::system::history::store::now_rfc3339(),
            ..Default::default()
        };
        repo.write_annotation(&original, &annotation).unwrap();
        let head = repo
            .ref_oid(super::HistoryRepo::HISTORY_REF)
            .unwrap()
            .unwrap();
        assert_ne!(head, original);
        assert_eq!(repo.output_tree_of(&head).unwrap(), tree);
        assert_eq!(
            repo.rev_list(&head, usize::MAX).unwrap(),
            vec![head.clone(), original.clone()]
        );
        let (target, restored) = repo.read_annotation(&head).unwrap().unwrap();
        assert_eq!(target, original);
        assert_eq!(restored.description, annotation.description);
        assert_eq!(restored.labels, annotation.labels);
        assert!(repo.list_refs("refs/notes/").unwrap().is_empty());
    }

    use super::*;
    use std::process::Command;

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn repo(tmp: &Path) -> HistoryRepo {
        HistoryRepo::open_or_init_in(&tmp.join("state"))
            .unwrap()
            .expect("git is required for these tests")
    }

    fn root(label: &str, path: &Path, files: &[&str]) -> CaptureRoot {
        CaptureRoot {
            label: label.into(),
            path: path.to_path_buf(),
            files: files.iter().map(PathBuf::from).collect(),
            bytes: 0,
        }
    }

    fn paths_of(repo: &HistoryRepo, tree: &str) -> Vec<String> {
        repo.ls_tree(tree)
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect()
    }

    #[test]
    fn omitted_capture_paths_survive_rebuilding_from_git() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let manifest = super::super::manifest::Manifest {
            enrollment: vec![super::super::manifest::Enrollment {
                path: "home/.native".into(),
                autosave: true,
                encrypt: false,
                variants: vec![],
            }],
            ..Default::default()
        };
        let tree = manifest
            .write(&repo, &repo.empty_object("tree").unwrap())
            .unwrap();
        let tracked = manifest.tracking().unwrap();
        let mut checkpoint =
            crate::system::history::checkpoint::test_checkpoint("omitted", Some(&tree));
        checkpoint.tree.coverage = tracked.coverage(&super::super::tracked::Walk {
            entries: tracked.entries.clone(),
            ..Default::default()
        });
        checkpoint
            .tree
            .coverage
            .omitted
            .push(super::super::store::PathReason {
                path: crate::file::display_path(crate::dirs::HOME.join(".native/large")),
                reason: "size limit".into(),
            });
        checkpoint
            .tree
            .coverage
            .incomplete
            .push(super::super::store::PathReason {
                path: crate::file::display_path(crate::dirs::HOME.join(".native/unreadable")),
                reason: "scan limit".into(),
            });
        let commit = repo
            .write_checkpoint(Some(&tree), &checkpoint, &BTreeMap::new())
            .unwrap();
        let rebuilt = repo.read_meta(&commit).unwrap();
        assert_eq!(rebuilt.tree.coverage.omitted[0].path, "~/.native/large");
        assert_eq!(
            rebuilt.tree.coverage.incomplete[0].path,
            "~/.native/unreadable"
        );
    }

    #[test]
    fn capture_ignores_attributes_and_preserves_raw_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join(".gitattributes"), "* text eol=lf\n").unwrap();
        std::fs::write(home.join("native"), b"a\r\nb\r\n").unwrap();
        let Some(repo) = HistoryRepo::open_or_init_in(&tmp.path().join("store")).unwrap() else {
            return;
        };
        let captured = repo.capture(&[root("home", &home, &["native"])]).unwrap();
        let (_, oid) = repo
            .object_at(&captured.tree, "home/native")
            .unwrap()
            .unwrap();
        assert_eq!(repo.cat_object(&oid).unwrap(), b"a\r\nb\r\n");
    }

    #[test]
    fn capture_takes_exactly_the_listed_files() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join(".config/app")).unwrap();
        std::fs::write(home.join(".zshrc"), "one\n").unwrap();
        std::fs::write(home.join(".config/app/a.toml"), "a\n").unwrap();
        std::fs::write(home.join(".config/app/skipped"), "no\n").unwrap();
        std::fs::write(home.join(".gitignore"), "*\n").unwrap();
        let repo = repo(tmp.path());
        let result = repo
            .capture(&[root(
                "home",
                &home,
                &[".zshrc", ".config/app/a.toml", ".gitignore"],
            )])
            .unwrap();
        assert_eq!(
            paths_of(&repo, &result.tree),
            vec!["home/.config/app/a.toml", "home/.gitignore", "home/.zshrc"]
        );
        assert_eq!(result.roots[0].files, 3);
        // the same content is the same tree
        let again = repo
            .capture(&[root(
                "home",
                &home,
                &[".zshrc", ".config/app/a.toml", ".gitignore"],
            )])
            .unwrap();
        assert_eq!(again.tree, result.tree);
    }

    #[test]
    fn ordinary_commits_preserve_ancestry_without_recovery_blobs() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".zshrc"), "one\n").unwrap();
        let repo = repo(tmp.path());
        let captured = repo.capture(&[root("home", &home, &[".zshrc"])]).unwrap();
        let checkpoint =
            crate::system::history::checkpoint::test_checkpoint("abc", Some(&captured.tree));
        let blob = repo.hash_blob(b"journal content").unwrap();
        let mut blobs = BTreeMap::new();
        blobs.insert("deadbeef".to_string(), blob.clone());
        let commit = repo
            .write_checkpoint(Some(&captured.tree), &checkpoint, &blobs)
            .unwrap();
        assert_eq!(
            repo.checkpoint_refs().unwrap(),
            vec![(commit.clone(), commit.clone())]
        );
        let meta = repo.read_meta(&commit).unwrap();
        assert_eq!(meta.uuid, commit);
        let empty = repo.empty_object("tree").unwrap();
        let reused_metadata = repo
            .write_checkpoint_commit(Some(&empty), &checkpoint, &BTreeMap::new())
            .unwrap();
        assert_eq!(
            repo.read_meta(&reused_metadata).unwrap().tree.snapshot,
            Some(empty)
        );
        assert!(repo.object_at(&commit, "snapshot").unwrap().is_none());
        assert!(repo.object_at(&commit, "blobs/deadbeef").unwrap().is_none());
        assert_eq!(paths_of(&repo, &commit), vec!["home/.zshrc"]);
        let mut second = checkpoint.clone();
        second.uuid = "second".into();
        let next = repo
            .write_checkpoint(Some(&captured.tree), &second, &BTreeMap::new())
            .unwrap();
        assert_eq!(
            repo.rev_list(&next, 10).unwrap(),
            vec![next.clone(), commit]
        );
        assert_eq!(repo.ref_oid(HistoryRepo::HISTORY_REF).unwrap(), Some(next));
        assert!(repo.list_refs("refs/checkpoints/").unwrap().is_empty());
        let stale = repo
            .write_checkpoint_commit(Some(&captured.tree), &checkpoint, &BTreeMap::new())
            .unwrap();
        let parent = repo.ref_oid(HistoryRepo::HISTORY_REF).unwrap().unwrap();
        let manual = repo
            .commit_tree(&captured.tree, vec![&parent], "edit with ordinary git")
            .unwrap();
        repo.advance_head(&manual).unwrap();
        assert!(repo.advance_head(&stale).is_err());
        assert_eq!(
            repo.ref_oid(HistoryRepo::HISTORY_REF).unwrap(),
            Some(manual.clone())
        );
        let manual_record = repo.read_meta(&manual).unwrap();
        assert_eq!(manual_record.uuid, manual);
        assert_eq!(manual_record.description, "edit with ordinary git");
        assert!(manual_record.operation.is_none());
    }

    #[test]
    fn encryption_cache_keys_are_private_stable_and_store_specific() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let key = encryption_cache_key(first.path()).unwrap();
        assert_eq!(key, encryption_cache_key(first.path()).unwrap());
        let other = encryption_cache_key(second.path()).unwrap();
        assert_ne!(key, other);
        assert_ne!(
            blake3::keyed_hash(&key, b"guessable-secret"),
            blake3::keyed_hash(&other, b"guessable-secret")
        );
        let path = first.path().join("encryption-cache.key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::write(&path, b"broken").unwrap();
        assert!(encryption_cache_key(first.path()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"broken");
    }

    #[test]
    fn encrypted_capture_never_stores_plaintext_objects_and_reuses_ciphertext() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let path = home.join("secret");
        let plaintext = b"never-store-this-plaintext-in-git";
        std::fs::write(&path, plaintext).unwrap();
        let repo = repo(tmp.path());
        let identity = age::x25519::Identity::generate();
        let recipients = vec![identity.to_public().to_string()];
        let mut walk = super::super::tracked::Walk {
            roots: vec![root("home", &home, &["secret"])],
            ..Default::default()
        };
        let mut policy =
            crate::system::files::FilePolicy::for_mode(crate::system::files::FileMode::Track);
        policy.encrypt = true;
        walk.files.insert(path, (0, policy));
        let first = repo.capture_tracked(&walk, &recipients, false).unwrap();
        let second = repo.capture_tracked(&walk, &recipients, false).unwrap();
        assert_eq!(first.tree, second.tree);
        let cache_path = repo.dir().parent().unwrap().join("index/encryption.json");
        let cache = std::fs::read_to_string(&cache_path).unwrap();
        assert!(!cache.contains(&blake3::hash(plaintext).to_hex().to_string()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(cache_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let (_, encrypted) = repo.object_at(&first.tree, "home/secret").unwrap().unwrap();
        assert!(
            repo.cat_object(&encrypted)
                .unwrap()
                .starts_with(b"mise-encrypted-file-v1\n")
        );
        let plaintext_oid = repo
            .output_str(PlumbingCall::new(["hash-object", "--stdin"]).stdin(plaintext))
            .unwrap();
        assert!(repo.cat_object(&plaintext_oid).is_err());
        assert!(repo.capture_tracked(&walk, &[], false).is_err());
    }

    #[test]
    fn recovery_preimages_stay_outside_git_objects() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let bytes = vec![42; super::super::journal::BLOB_INLINE_MAX as usize + 1];
        let blob = super::super::journal::Blob::store_in(tmp.path(), &bytes).unwrap();
        assert!(blob.inline.is_none());
        let oid = repo
            .output_str(PlumbingCall::new(["hash-object", "--stdin"]).stdin(&bytes))
            .unwrap();
        assert!(repo.cat_object(&oid).is_err());
        assert_eq!(
            std::fs::read(super::super::journal::blobs_dir_in(tmp.path()).join(blob.sha256))
                .unwrap(),
            bytes
        );
    }

    #[test]
    fn changes_and_diff_between_trees() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".zshrc"), "one\n").unwrap();
        std::fs::write(home.join(".old"), "old\n").unwrap();
        let repo = repo(tmp.path());
        let a = repo
            .capture(&[root("home", &home, &[".zshrc", ".old"])])
            .unwrap();
        std::fs::write(home.join(".zshrc"), "two\n").unwrap();
        std::fs::write(home.join(".new"), "new\n").unwrap();
        let b = repo
            .capture(&[root("home", &home, &[".zshrc", ".new"])])
            .unwrap();
        let changes = repo.changes(Some(&a.tree), &b.tree).unwrap();
        assert_eq!(
            changes,
            vec![
                Change {
                    status: 'A',
                    path: "home/.new".into()
                },
                Change {
                    status: 'D',
                    path: "home/.old".into()
                },
                Change {
                    status: 'M',
                    path: "home/.zshrc".into()
                },
            ]
        );
        let initial = repo.changes(None, &a.tree).unwrap();
        assert_eq!(initial.len(), 2);
        assert!(initial.iter().all(|change| change.status == 'A'));
        let same = repo.diff(&a.tree, &a.tree, &DiffOpts::default()).unwrap();
        assert!(!same.changed);
        let patch = repo
            .diff(
                &a.tree,
                &b.tree,
                &DiffOpts {
                    patch: true,
                    paths: Some(("home/.zshrc".into(), "home/.zshrc".into())),
                    ..Default::default()
                },
            )
            .unwrap();
        let text = String::from_utf8_lossy(&patch.output);
        assert!(text.contains("-one") && text.contains("+two"), "{text}");
        let added = repo
            .diff(
                &a.tree,
                &b.tree,
                &DiffOpts {
                    patch: true,
                    paths: Some(("home/.new".into(), "home/.new".into())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(String::from_utf8_lossy(&added.output).contains("+new"));
        let missing = repo
            .diff(
                &a.tree,
                &b.tree,
                &DiffOpts {
                    paths: Some(("home/nope".into(), "home/nope".into())),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(missing.to_string().contains("not in either snapshot"));
    }

    #[test]
    fn compose_replaces_paths_without_a_separate_saved_history() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join("app")).unwrap();
        std::fs::write(home.join("app/state.json"), "v1\n").unwrap();
        std::fs::write(home.join(".zshrc"), "one\n").unwrap();
        let repo = repo(tmp.path());
        let v1 = repo
            .capture(&[root("home", &home, &["app/state.json", ".zshrc"])])
            .unwrap();
        let (mode, saved) = repo
            .object_at(&v1.tree, "home/app/state.json")
            .unwrap()
            .unwrap();
        assert_eq!(mode, "100644");
        std::fs::write(home.join("app/state.json"), "v2 live\n").unwrap();
        let v2 = repo
            .capture(&[root("home", &home, &["app/state.json", ".zshrc"])])
            .unwrap();
        let composed = repo
            .compose(
                &v2.tree,
                &[Overlay {
                    path: "home/app/state.json".into(),
                    object: Some((mode, saved.clone())),
                }],
            )
            .unwrap();
        assert_eq!(
            repo.object_at(&composed, "home/app/state.json")
                .unwrap()
                .unwrap()
                .1,
            saved
        );
        // an overlay without an object removes the path
        let without = repo
            .compose(
                &v2.tree,
                &[Overlay {
                    path: "home/app/state.json".into(),
                    object: None,
                }],
            )
            .unwrap();
        assert!(
            repo.object_at(&without, "home/app/state.json")
                .unwrap()
                .is_none()
        );
        assert!(repo.object_at(&without, "home/.zshrc").unwrap().is_some());
    }

    #[test]
    fn nested_repositories_become_gitlinks() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let nested = home.join(".config/plugin");
        std::fs::create_dir_all(&nested).unwrap();
        git_in(&nested, &["init", "-q"]);
        std::fs::write(nested.join("file"), "x\n").unwrap();
        git_in(&nested, &["add", "file"]);
        git_in(
            &nested,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ],
        );
        std::fs::write(nested.join("untracked"), "y\n").unwrap();
        std::fs::write(home.join(".zshrc"), "one\n").unwrap();
        let repo = repo(tmp.path());
        let result = repo
            .capture(&[root("home", &home, &[".zshrc", ".config/plugin"])])
            .unwrap();
        let entries = repo.ls_tree(&result.tree).unwrap();
        let link = entries
            .iter()
            .find(|entry| entry.path == "home/.config/plugin")
            .expect("gitlink recorded");
        assert_eq!(link.mode, "160000");
        // the nested repository itself was not touched
        assert_eq!(git_in(&nested, &["status", "--porcelain"]), "?? untracked");
    }
}
