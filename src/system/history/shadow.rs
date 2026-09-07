//! The bare repository mise owns under `$MISE_STATE_DIR/history/repo.git`.
//!
//! A checkpoint is one **wrapper commit** without parents whose tree is
//! `snapshot/` (the captured files, rooted at `home/` for `$HOME` and `fs/`
//! for anything outside it), `meta.json` (the checkpoint record), and
//! `blobs/<sha256>` (journal content the record references). The refs
//! `refs/checkpoints/<uuid>` are the only things keeping objects alive, so
//! pruning a checkpoint and running `gc` frees its content.
//!
//! Files are added with `git add -f` under a scratch index from literal
//! pathspecs mise's own walker produced: the root's `.gitignore` is bypassed
//! on purpose (an ignored file is often exactly the secret a rollback must
//! restore), git never indexes a `.git` component so the user's own
//! repositories are untouched, and nested repositories become gitlinks
//! rather than content. The user's checkouts and indexes are never touched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail};

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
}

impl HistoryRepo {
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
        Ok(Some(Self { git }))
    }

    pub(crate) fn dir(&self) -> &Path {
        self.git.git_dir()
    }

    /// Builds the snapshot tree for `roots`: one subtree per root holding
    /// exactly the listed files.
    pub(crate) fn capture(&self, roots: &[CaptureRoot]) -> Result<CaptureResult> {
        let mut warnings = vec![];
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
                .tree_for_root(root, &mut warnings)
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
            warnings,
        })
    }

    /// Adds a root's files under a scratch index and returns the tree id.
    fn tree_for_root(&self, root: &CaptureRoot, warnings: &mut Vec<String>) -> Result<String> {
        let index = self
            .dir()
            .join(format!("mise-index-{}-{}", std::process::id(), root.label));
        let _ = std::fs::remove_file(&index);
        let mut pathspecs: Vec<u8> = vec![];
        for rel in &root.files {
            pathspecs.extend_from_slice(b":(literal)");
            pathspecs.extend_from_slice(path_bytes(rel).as_ref());
            pathspecs.push(0);
        }
        let add = PlumbingCall::new([
            "add",
            "-f",
            "--ignore-errors",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ])
        .work_tree(&root.path)
        .cwd(&root.path)
        .index_file(&index)
        .stdin(&pathspecs);
        // `--ignore-errors` keeps adding past unreadable files but still exits
        // non-zero, so the status is advisory here and the tree is what counts.
        let output = self.git.output_unchecked(add)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            for line in stderr.lines().filter(|line| !line.trim().is_empty()) {
                warnings.push(format!("{}: {line}", root.label));
            }
        }
        let tree = self.output_str(PlumbingCall::new(["write-tree"]).index_file(&index));
        let _ = std::fs::remove_file(&index);
        tree
    }

    /// One level of tree from an `ls-tree`-style listing.
    pub(crate) fn mktree(&self, listing: &str) -> Result<String> {
        self.output_str(PlumbingCall::new(["mktree"]).stdin(listing.as_bytes()))
    }

    /// A tree holding exactly `entries` (`(mode, oid, path)`, paths nested
    /// as deep as they like), built in a scratch index so the repository's
    /// own index is never touched. Objects need not exist for gitlinks
    /// (`160000`), as in any tree git writes.
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
        self.set_checkpoint_ref(&checkpoint.uuid, &commit)?;
        Ok(commit)
    }

    /// The wrapper commit alone, without pointing the checkpoint's ref at
    /// it: for a filtered copy that leaves the machine (a backup) while the
    /// full local checkpoint stays what the ref names.
    pub(crate) fn write_checkpoint_commit(
        &self,
        snapshot_tree: Option<&str>,
        checkpoint: &Checkpoint,
        blobs: &BTreeMap<String, String>,
    ) -> Result<String> {
        let mut meta = serde_json::to_string_pretty(checkpoint)?;
        meta.push('\n');
        let meta_oid = self.hash_blob(meta.as_bytes())?;
        let mut listing = String::new();
        if !blobs.is_empty() {
            let blob_listing = blobs
                .iter()
                .map(|(sha256, oid)| format!("100644 blob {oid}\t{sha256}\n"))
                .collect::<String>();
            let blobs_tree = self.mktree(&blob_listing)?;
            listing.push_str(&format!("040000 tree {blobs_tree}\tblobs\n"));
        }
        listing.push_str(&format!("100644 blob {meta_oid}\tmeta.json\n"));
        if let Some(tree) = snapshot_tree {
            listing.push_str(&format!("040000 tree {tree}\tsnapshot\n"));
        }
        let tree = self.mktree(&listing)?;
        let message = format!(
            "checkpoint {} ({}): {}",
            checkpoint.uuid,
            checkpoint.trigger.as_str(),
            checkpoint.description
        );
        self.output_str(PlumbingCall::new(["commit-tree", &tree, "-m", &message]))
    }

    pub(crate) fn checkpoint_ref(uuid: &str) -> String {
        format!("refs/checkpoints/{uuid}")
    }

    pub(crate) fn set_checkpoint_ref(&self, uuid: &str, commit: &str) -> Result<()> {
        self.git.run(PlumbingCall::new([
            "update-ref",
            &Self::checkpoint_ref(uuid),
            commit,
        ]))
    }

    /// Drops a checkpoint's ref. A missing ref is not an error.
    pub(crate) fn delete_checkpoint_ref(&self, uuid: &str) {
        let name = Self::checkpoint_ref(uuid);
        if let Err(err) = self
            .git
            .output_unchecked(PlumbingCall::new(["update-ref", "-d", &name]))
        {
            warn!("history: deleting {name}: {err}");
        }
    }

    /// Every `refs/checkpoints/<uuid>` with its commit.
    pub(crate) fn checkpoint_refs(&self) -> Result<Vec<(String, String)>> {
        let out = self.output_str(PlumbingCall::new([
            "for-each-ref",
            "--format=%(refname:strip=2) %(objectname)",
            "refs/checkpoints/",
        ]))?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let (uuid, commit) = line.split_once(' ')?;
                Some((uuid.to_string(), commit.to_string()))
            })
            .collect())
    }

    /// Reads `meta.json` from a wrapper commit.
    pub(crate) fn read_meta(&self, commit: &str) -> Result<Checkpoint> {
        let spec = format!("{commit}:meta.json");
        let bytes = self
            .git
            .output(PlumbingCall::new(["cat-file", "blob", &spec]))?;
        serde_json::from_slice(&bytes).wrap_err_with(|| format!("reading {spec}"))
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
        self.git
            .output(PlumbingCall::new(["cat-file", "blob", oid]))
            .wrap_err_with(|| format!("reading {oid}"))
    }

    pub(crate) fn blob_starts_with(&self, oid: &str, prefix: &[u8]) -> Result<bool> {
        self.git.blob_starts_with(oid, prefix)
    }

    pub(crate) fn cat_object_bounded(&self, oid: &str, limit: u64) -> Result<Vec<u8>> {
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

    /// Compares two trees, from `a` to `b`. With `paths`, a path that exists
    /// on only one side is compared against an empty tree or blob so an
    /// added or removed path shows up as its whole contents.
    pub(crate) fn diff(&self, a: &str, b: &str, opts: &DiffOpts) -> Result<DiffResult> {
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

    pub(crate) const PROMOTED_REF: &'static str = "refs/promoted";

    /// The head of the promotion chain, if any.
    pub(crate) fn promoted_head(&self) -> Result<Option<String>> {
        let output = self.git.output_unchecked(PlumbingCall::new([
            "rev-parse",
            "--verify",
            "-q",
            Self::PROMOTED_REF,
        ]))?;
        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else {
            Ok(None)
        }
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

    /// Appends a promotion commit whose tree is `tree`, expecting `expected`
    /// as the current head (compare-and-swap).
    pub(crate) fn write_promotion(
        &self,
        tree: &str,
        expected: Option<&str>,
        message: &str,
    ) -> Result<String> {
        let commit = match expected {
            Some(parent) => self.output_str(PlumbingCall::new([
                "commit-tree",
                tree,
                "-p",
                parent,
                "-m",
                message,
            ]))?,
            None => self.output_str(PlumbingCall::new(["commit-tree", tree, "-m", message]))?,
        };
        let mut args = vec!["update-ref", Self::PROMOTED_REF, &commit];
        let zero = "0000000000000000000000000000000000000000";
        let old = expected.unwrap_or(zero);
        args.push(old);
        self.git.run(PlumbingCall::new(args))?;
        Ok(commit)
    }

    /// Squashes the promotion chain to one parentless commit holding the
    /// current promoted state. Every promoted version a retained checkpoint
    /// needs is in that checkpoint's own snapshot, so nothing else keeps
    /// expired versions alive and `gc` can free them.
    pub(crate) fn compact_promotions(&self) -> Result<bool> {
        let Some(head) = self.promoted_head()? else {
            return Ok(false);
        };
        let depth = self.output_str(PlumbingCall::new(["rev-list", "--count", &head]))?;
        if depth == "1" {
            return Ok(false);
        }
        let tree = self.output_tree_of(&head)?;
        let commit = self.output_str(PlumbingCall::new([
            "commit-tree",
            &tree,
            "-m",
            "promoted state (compacted)",
        ]))?;
        self.git.run(PlumbingCall::new([
            "update-ref",
            Self::PROMOTED_REF,
            &commit,
            &head,
        ]))?;
        Ok(true)
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

    /// Up to `limit` commits reachable from `head`, newest first.
    pub(crate) fn rev_list(&self, head: &str, limit: usize) -> Result<Vec<String>> {
        let out = self.output_str(PlumbingCall::new([
            "rev-list",
            &format!("--max-count={limit}"),
            head,
        ]))?;
        Ok(out.lines().map(str::to_string).collect())
    }

    /// The paths a commit changed against its first parent (all of them for
    /// a root commit).
    pub(crate) fn changed_names(&self, commit: &str) -> Result<Vec<String>> {
        let out = self.git.output(PlumbingCall::new([
            "diff-tree",
            "-r",
            "--root",
            "--name-only",
            "--no-commit-id",
            "-z",
            commit,
        ]))?;
        Ok(out
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
            .map(|record| String::from_utf8_lossy(record).into_owned())
            .collect())
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

    pub(crate) const NOTES_REF: &'static str = "refs/notes/mise-history";

    /// Attaches (or replaces) the annotation of a wrapper commit.
    pub(crate) fn write_note(&self, commit: &str, text: &str) -> Result<()> {
        self.git.run(
            PlumbingCall::new([
                "notes",
                "--ref",
                Self::NOTES_REF,
                "add",
                "-f",
                "-F",
                "-",
                commit,
            ])
            .stdin(text.as_bytes()),
        )
    }

    /// The annotation of a wrapper commit, if any.
    pub(crate) fn read_note(&self, commit: &str) -> Result<Option<String>> {
        let output = self.git.output_unchecked(PlumbingCall::new([
            "notes",
            "--ref",
            Self::NOTES_REF,
            "show",
            commit,
        ]))?;
        if output.status.success() {
            Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
        } else {
            Ok(None)
        }
    }

    /// Frees objects no ref keeps alive.
    pub(crate) fn gc(&self) -> Result<()> {
        self.git
            .run(PlumbingCall::new(["gc", "--prune=now", "--quiet"]))
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
    fn wrapper_commit_round_trips_meta_and_blobs() {
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
            vec![("abc".to_string(), commit.clone())]
        );
        let meta = repo.read_meta(&commit).unwrap();
        assert_eq!(meta.uuid, "abc");
        assert_eq!(
            repo.object_at(&commit, "snapshot")
                .unwrap()
                .map(|(_, oid)| oid),
            Some(captured.tree.clone())
        );
        assert_eq!(
            git_in(
                &tmp.path().join("state/history/repo.git"),
                &["cat-file", "blob", &format!("{commit}:blobs/deadbeef")]
            ),
            "journal content"
        );
        assert_eq!(
            paths_of(&repo, &format!("{commit}:snapshot")),
            vec!["home/.zshrc"]
        );
        let state = tmp.path().join("state");
        assert_eq!(
            git_in(&state.join("history/repo.git"), &["fsck", "--strict"]),
            ""
        );
        repo.delete_checkpoint_ref("abc");
        assert!(repo.checkpoint_refs().unwrap().is_empty());
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
    fn compose_replaces_paths_and_promotions_chain() {
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
        // a promotion records the saved object
        let promoted_tree = repo
            .compose(
                &repo.empty_object("tree").unwrap(),
                &[Overlay {
                    path: "promoted/home/app/state.json".into(),
                    object: Some((mode.clone(), saved.clone())),
                }],
            )
            .unwrap();
        let head = repo
            .write_promotion(&promoted_tree, None, "promote")
            .unwrap();
        assert_eq!(
            repo.promoted_head().unwrap().as_deref(),
            Some(head.as_str())
        );
        // the live file moves on; the composed tree keeps the saved version
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
        // a stale expectation is refused
        assert!(repo.write_promotion(&promoted_tree, None, "stale").is_err());
        let next = repo
            .write_promotion(&promoted_tree, Some(&head), "next")
            .unwrap();
        assert_eq!(
            repo.promoted_head().unwrap().as_deref(),
            Some(next.as_str())
        );
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
