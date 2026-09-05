//! The shadow repository: a bare git repo mise owns that snapshots the
//! config directory and dotfiles root without touching the user's own
//! checkouts.
//!
//! A snapshot is one commit whose top-level tree holds one subtree per
//! root (`config`, `dotfiles`) plus the global lockfile as `mise.lock`.
//! Commits have no parents; the per-generation refs
//! `refs/generations/<id>/{before,after}` are the only things keeping
//! objects alive, so pruning a generation and running `gc` frees its
//! content.
//!
//! Roots are added with `git add -A -f` under a scratch index: the root's
//! own `.gitignore` is bypassed on purpose (an ignored file is often exactly
//! the secret a rollback must restore), git never indexes a `.git`
//! component so the root's own repository is untouched, and nested
//! repositories become gitlinks rather than content.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail};
use walkdir::WalkDir;

use super::store::{RootRecord, VcsInfo, store_dir_in};
use crate::dirs;
use crate::file::display_path;
use crate::git::{GitPlumbing, PlumbingCall};

/// Regular files above this size are left out of a snapshot.
pub(crate) const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// A root with more files than this is skipped entirely.
pub(crate) const MAX_FILES: u64 = 100_000;
/// A root with more bytes than this is skipped entirely.
pub(crate) const MAX_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapshotPhase {
    Before,
    After,
}

impl SnapshotPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SnapshotRoot {
    pub label: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct SnapshotResult {
    pub commit: String,
    pub tree: String,
    pub roots: Vec<RootRecord>,
    pub lockfile_blob: Option<String>,
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
    pub color: bool,
    /// Restrict the comparison to a path inside each snapshot tree, resolved
    /// separately for `a` and `b` since a root may move between them.
    pub paths: Option<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct DiffResult {
    pub output: Vec<u8>,
    pub changed: bool,
}

#[derive(Debug)]
pub(crate) struct ShadowRepo {
    git: GitPlumbing,
}

impl ShadowRepo {
    pub(crate) fn path_in(state_dir: &Path) -> PathBuf {
        store_dir_in(state_dir).join("generations.git")
    }

    /// Opens the shadow repository, creating it on first use. `Ok(None)`
    /// means no git binary mise is willing to run is available.
    pub(crate) fn open_or_init_in(state_dir: &Path) -> Result<Option<Self>> {
        if crate::git::plumbing_binary().is_none() {
            return Ok(None);
        }
        let git = GitPlumbing::new(Self::path_in(state_dir));
        git.init_bare()
            .wrap_err_with(|| format!("initializing {}", display_path(git.git_dir())))?;
        Ok(Some(Self { git }))
    }

    pub(crate) fn dir(&self) -> &Path {
        self.git.git_dir()
    }

    /// Snapshots `roots` and the lockfile bytes into one commit and points
    /// `refs/generations/<id>/<phase>` at it.
    pub(crate) fn snapshot(
        &self,
        roots: &[SnapshotRoot],
        lockfile: Option<&[u8]>,
        id: u64,
        phase: SnapshotPhase,
        message: &str,
    ) -> Result<SnapshotResult> {
        let mut warnings = vec![];
        let mut records = plan_roots(roots);
        let mut entries: Vec<(String, String, String)> = vec![]; // (mode type, oid, name)
        for record in &mut records {
            if record.skipped.is_some()
                || record.alias_of.is_some()
                || record.contained_in.is_some()
            {
                continue;
            }
            let scanned = scan_root(&record.path);
            warnings.extend(scanned.warnings.iter().cloned());
            if let Some(reason) = scanned.skipped {
                record.skipped = Some(reason);
                continue;
            }
            record.files = scanned.files;
            record.bytes = scanned.bytes;
            let tree = self
                .tree_for_root(
                    &record.path,
                    &record.label,
                    &scanned.excludes,
                    &mut warnings,
                )
                .wrap_err_with(|| format!("snapshotting {}", display_path(&record.path)))?;
            entries.push(("040000 tree".into(), tree.clone(), record.label.clone()));
            record.tree = Some(tree);
        }
        let lockfile_blob = match lockfile {
            Some(bytes) => {
                let oid = self
                    .output_str(PlumbingCall::new(["hash-object", "-w", "--stdin"]).stdin(bytes))?;
                entries.push(("100644 blob".into(), oid.clone(), "mise.lock".into()));
                Some(oid)
            }
            None => None,
        };
        entries.sort_by(|a, b| a.2.cmp(&b.2));
        let listing = entries
            .iter()
            .map(|(kind, oid, name)| format!("{kind} {oid}\t{name}\n"))
            .collect::<String>();
        let tree = self.output_str(PlumbingCall::new(["mktree"]).stdin(listing.as_bytes()))?;
        let commit = self.output_str(PlumbingCall::new(["commit-tree", &tree, "-m", message]))?;
        self.git.run(PlumbingCall::new([
            "update-ref",
            &format!("refs/generations/{id}/{}", phase.as_str()),
            &commit,
        ]))?;
        Ok(SnapshotResult {
            commit,
            tree,
            roots: records,
            lockfile_blob,
            warnings,
        })
    }

    /// Adds a root's files under a scratch index and returns the tree id.
    fn tree_for_root(
        &self,
        root: &Path,
        label: &str,
        excludes: &[PathBuf],
        warnings: &mut Vec<String>,
    ) -> Result<String> {
        let index = self
            .dir()
            .join(format!("mise-index-{}-{label}", std::process::id()));
        let _ = std::fs::remove_file(&index);
        let mut pathspecs: Vec<u8> = b".\0".to_vec();
        for rel in excludes {
            pathspecs.extend_from_slice(b":(exclude,literal)");
            pathspecs.extend_from_slice(path_bytes(rel).as_ref());
            pathspecs.push(0);
        }
        let add = PlumbingCall::new([
            "add",
            "-A",
            "-f",
            "--ignore-errors",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ])
        .work_tree(root)
        .cwd(root)
        .index_file(&index)
        .stdin(&pathspecs);
        // `--ignore-errors` keeps adding past unreadable files but still exits
        // non-zero, so the status is advisory here and the tree is what counts.
        let output = self.git.output_unchecked(add)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            for line in stderr.lines().filter(|line| !line.trim().is_empty()) {
                warnings.push(format!("{label}: {line}"));
            }
        }
        let tree = self.output_str(PlumbingCall::new(["write-tree"]).index_file(&index));
        let _ = std::fs::remove_file(&index);
        tree
    }

    /// Recursive listing of one root inside a snapshot commit.
    pub(crate) fn ls_tree(&self, commit: &str, label: &str) -> Result<Vec<TreeEntry>> {
        let spec = format!("{commit}:{label}");
        let out = self
            .git
            .output(PlumbingCall::new(["ls-tree", "-r", "-l", "-z", &spec]))?;
        let mut entries = vec![];
        for record in out.split(|byte| *byte == 0) {
            if record.is_empty() {
                continue;
            }
            let record = String::from_utf8_lossy(record);
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

    /// The type of the object at `spec` (`<tree>:<path>`), or `None` when
    /// nothing is there.
    fn object_type(&self, spec: &str) -> Result<Option<String>> {
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
    /// in for a side of a diff where a path does not exist yet.
    fn empty_object(&self, kind: &str) -> Result<String> {
        let args: &[&str] = match kind {
            "tree" => &["mktree"],
            _ => &["hash-object", "-w", "--stdin"],
        };
        self.output_str(PlumbingCall::new(args.iter().copied()).stdin(b""))
    }

    /// Compares two snapshot trees, from `a` to `b`. With `paths`, a path that
    /// exists on only one side is compared against an empty tree or blob so an
    /// added or removed root shows up as its whole contents.
    pub(crate) fn diff(&self, a: &str, b: &str, opts: &DiffOpts) -> Result<DiffResult> {
        let (from, to) = match &opts.paths {
            Some((from_path, to_path)) => {
                let from = format!("{a}:{from_path}");
                let to = format!("{b}:{to_path}");
                match (self.object_type(&from)?, self.object_type(&to)?) {
                    (Some(_), Some(_)) => (from, to),
                    (Some(kind), None) => {
                        let empty = self.empty_object(&kind)?;
                        (from, empty)
                    }
                    (None, Some(kind)) => {
                        let empty = self.empty_object(&kind)?;
                        (empty, to)
                    }
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
        let output = self.git.output_unchecked(PlumbingCall::new([
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
        ]))?;
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

    /// Drops a generation's refs. Missing refs are not an error.
    pub(crate) fn delete_refs(&self, id: u64) {
        for phase in [SnapshotPhase::Before, SnapshotPhase::After] {
            let name = format!("refs/generations/{id}/{}", phase.as_str());
            if let Err(err) =
                self.git
                    .output_unchecked(PlumbingCall::new(["update-ref", "-d", &name]))
            {
                warn!("bootstrap generations: deleting {name}: {err}");
            }
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
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(path.to_string_lossy().replace('\\', "/").into_bytes())
}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Never snapshot the home directory, anything above it, or a filesystem root.
fn refused(path: &Path) -> bool {
    let home = canonical(&dirs::HOME);
    path.parent().is_none() || home.starts_with(path)
}

/// Decides which roots get their own tree and which are aliases of or
/// contained in another root. Records keep the caller's order.
fn plan_roots(roots: &[SnapshotRoot]) -> Vec<RootRecord> {
    let mut records: Vec<RootRecord> = roots
        .iter()
        .map(|root| RootRecord {
            label: root.label.clone(),
            path: root.path.clone(),
            ..Default::default()
        })
        .collect();
    let canonicals: Vec<Option<PathBuf>> = records
        .iter_mut()
        .map(|record| {
            if !record.path.is_dir() {
                record.skipped = Some("missing".into());
                return None;
            }
            let path = canonical(&record.path);
            if refused(&path) {
                record.skipped = Some("refused".into());
                return None;
            }
            record.vcs = vcs_info(&path);
            Some(path)
        })
        .collect();
    // Outer roots first so containment is always "later inside earlier".
    let mut order: Vec<usize> = (0..records.len())
        .filter(|i| canonicals[*i].is_some())
        .collect();
    order.sort_by_key(|i| canonicals[*i].as_ref().map(|p| p.components().count()));
    let mut accepted: Vec<usize> = vec![];
    for i in order {
        let path = canonicals[i].as_ref().expect("filtered above");
        let mut decided = false;
        for &j in &accepted {
            let other = canonicals[j]
                .as_ref()
                .expect("accepted roots are canonical");
            if path == other {
                records[i].alias_of = Some(records[j].label.clone());
                decided = true;
                break;
            }
            if let Ok(subpath) = path.strip_prefix(other) {
                records[i].contained_in = Some(records[j].label.clone());
                records[i].subpath = Some(subpath.to_path_buf());
                decided = true;
                break;
            }
        }
        if !decided {
            accepted.push(i);
        }
    }
    records
}

fn vcs_info(path: &Path) -> Option<VcsInfo> {
    let root = crate::git::root_of(path)?;
    let git = crate::git::Git::new(&root);
    Some(VcsInfo {
        head: git.current_sha().ok(),
        branch: git.current_branch().ok().filter(|b| !b.is_empty()),
        root,
    })
}

struct ScannedRoot {
    files: u64,
    bytes: u64,
    /// Paths relative to the root that must not be added.
    excludes: Vec<PathBuf>,
    warnings: Vec<String>,
    skipped: Option<String>,
}

/// Directories mise owns that must never be captured if they happen to sit
/// under a snapshot root (a `dotfiles.root` of `~/.local`, say).
fn mise_dirs() -> Vec<PathBuf> {
    [
        *dirs::STATE,
        *dirs::CACHE,
        *dirs::DATA,
        *dirs::INSTALLS,
        *dirs::DOWNLOADS,
        *dirs::PLUGINS,
    ]
    .into_iter()
    .map(canonical)
    .collect()
}

fn scan_root(root: &Path) -> ScannedRoot {
    let mise_dirs = mise_dirs();
    let root_canonical = canonical(root);
    let mut scanned = ScannedRoot {
        files: 0,
        bytes: 0,
        excludes: vec![],
        warnings: vec![],
        skipped: None,
    };
    // Directories mise owns that sit under the root are pruned from the walk
    // below, so they must be handed to git as exclusions here.
    for dir in &mise_dirs {
        if let Ok(rel) = dir.strip_prefix(&root_canonical)
            && !rel.as_os_str().is_empty()
        {
            scanned.excludes.push(rel.to_path_buf());
        }
    }
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            !(entry.file_type().is_dir()
                && (entry.file_name() == ".git" || mise_dirs.contains(&canonical(entry.path()))))
        });
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                scanned
                    .warnings
                    .push(format!("skipped unreadable path: {err}"));
                continue;
            }
        };
        let rel = match entry.path().strip_prefix(root) {
            Ok(rel) if !rel.as_os_str().is_empty() => rel.to_path_buf(),
            _ => continue,
        };
        let file_type = entry.file_type();
        if file_type.is_dir() {
            continue;
        }
        if file_type.is_symlink() {
            scanned.files += 1;
            continue;
        }
        if !file_type.is_file() {
            scanned.warnings.push(format!(
                "skipped special file: {}",
                display_path(entry.path())
            ));
            scanned.excludes.push(rel);
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size > MAX_FILE_BYTES {
            scanned.warnings.push(format!(
                "skipped {} ({} MiB is over the {} MiB snapshot limit)",
                display_path(entry.path()),
                size / (1024 * 1024),
                MAX_FILE_BYTES / (1024 * 1024)
            ));
            scanned.excludes.push(rel);
            continue;
        }
        scanned.files += 1;
        scanned.bytes += size;
        if scanned.files > MAX_FILES || scanned.bytes > MAX_BYTES {
            scanned.warnings.push(format!(
                "skipped {}: more than {} files or {} MiB",
                display_path(&root_canonical),
                MAX_FILES,
                MAX_BYTES / (1024 * 1024)
            ));
            scanned.skipped = Some("too-large".into());
            break;
        }
    }
    scanned
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

    fn shadow(tmp: &Path) -> ShadowRepo {
        ShadowRepo::open_or_init_in(&tmp.join("state"))
            .unwrap()
            .expect("git is required for these tests")
    }

    fn files_of(repo: &ShadowRepo, commit: &str, label: &str) -> Vec<(String, String)> {
        repo.ls_tree(commit, label)
            .unwrap()
            .into_iter()
            .map(|entry| (entry.mode, entry.path))
            .collect()
    }

    fn root(label: &str, path: &Path) -> SnapshotRoot {
        SnapshotRoot {
            label: label.into(),
            path: path.to_path_buf(),
        }
    }

    #[test]
    fn snapshot_captures_files_symlinks_and_modes() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::fs::create_dir_all(config.join("conf.d")).unwrap();
        std::fs::write(config.join("config.toml"), "[tools]\n").unwrap();
        std::fs::write(config.join("conf.d/extra.toml"), "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(config.join("run.sh"), "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(
                config.join("run.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            std::os::unix::fs::symlink("config.toml", config.join("link")).unwrap();
        }
        let repo = shadow(tmp.path());
        let result = repo
            .snapshot(
                &[root("config", &config)],
                Some(b"lock"),
                1,
                SnapshotPhase::Before,
                "test",
            )
            .unwrap();
        assert_eq!(result.roots.len(), 1);
        assert!(result.roots[0].tree.is_some());
        assert!(result.lockfile_blob.is_some());
        let files = files_of(&repo, &result.commit, "config");
        assert!(files.contains(&("100644".into(), "config.toml".into())));
        assert!(files.contains(&("100644".into(), "conf.d/extra.toml".into())));
        #[cfg(unix)]
        {
            assert!(files.contains(&("100755".into(), "run.sh".into())));
            assert!(files.contains(&("120000".into(), "link".into())));
        }
        let lock = git_in(
            repo.dir(),
            &["cat-file", "-p", &format!("{}:mise.lock", result.commit)],
        );
        assert_eq!(lock, "lock");
        assert_eq!(
            git_in(repo.dir(), &["rev-parse", "refs/generations/1/before"]),
            result.commit
        );
    }

    #[test]
    fn snapshot_ignores_gitignore_and_leaves_the_root_repo_alone() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dotfiles = tmp.path().join("dotfiles");
        std::fs::create_dir_all(&dotfiles).unwrap();
        git_in(&dotfiles, &["-c", "init.defaultBranch=main", "init", "-q"]);
        git_in(&dotfiles, &["config", "user.email", "t@example.com"]);
        git_in(&dotfiles, &["config", "user.name", "t"]);
        std::fs::write(dotfiles.join(".gitignore"), "secret\n").unwrap();
        std::fs::write(dotfiles.join("zshrc"), "alias ll='ls -l'\n").unwrap();
        std::fs::write(dotfiles.join("secret"), "token\n").unwrap();
        git_in(&dotfiles, &["add", "."]);
        git_in(&dotfiles, &["commit", "-qm", "init"]);
        let head_before = git_in(&dotfiles, &["rev-parse", "HEAD"]);
        let index_before = std::fs::read(dotfiles.join(".git/index")).unwrap();

        let repo = shadow(tmp.path());
        let result = repo
            .snapshot(
                &[root("dotfiles", &dotfiles)],
                None,
                1,
                SnapshotPhase::After,
                "test",
            )
            .unwrap();
        let files = files_of(&repo, &result.commit, "dotfiles");
        let names = files.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>();
        assert!(names.contains(&"secret"), "{names:?}");
        assert!(names.contains(&"zshrc"));
        assert!(names.contains(&".gitignore"));
        assert!(!names.iter().any(|n| n.starts_with(".git/")), "{names:?}");
        assert_eq!(git_in(&dotfiles, &["rev-parse", "HEAD"]), head_before);
        assert_eq!(
            std::fs::read(dotfiles.join(".git/index")).unwrap(),
            index_before
        );
        assert_eq!(git_in(&dotfiles, &["status", "--porcelain"]), "");
        let vcs = result.roots[0].vcs.as_ref().unwrap();
        assert_eq!(vcs.head.as_deref(), Some(head_before.as_str()));
    }

    #[test]
    fn nested_repositories_become_gitlinks() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let plugin = config.join("plugins/vim");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(config.join("config.toml"), "").unwrap();
        git_in(&plugin, &["-c", "init.defaultBranch=main", "init", "-q"]);
        git_in(&plugin, &["config", "user.email", "t@example.com"]);
        git_in(&plugin, &["config", "user.name", "t"]);
        std::fs::write(plugin.join("plugin.vim"), "").unwrap();
        git_in(&plugin, &["add", "."]);
        git_in(&plugin, &["commit", "-qm", "init"]);
        let repo = shadow(tmp.path());
        let result = repo
            .snapshot(
                &[root("config", &config)],
                None,
                2,
                SnapshotPhase::Before,
                "test",
            )
            .unwrap();
        let files = files_of(&repo, &result.commit, "config");
        assert!(
            files.contains(&("160000".into(), "plugins/vim".into())),
            "{files:?}"
        );
        assert!(!files.iter().any(|(_, p)| p == "plugins/vim/plugin.vim"));
    }

    #[test]
    fn roots_are_deduplicated_nested_and_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().join("outer");
        let inner = outer.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let records = plan_roots(&[
            root("config", &inner),
            root("dotfiles", &outer),
            root("again", &inner),
            root("missing", &tmp.path().join("nope")),
        ]);
        assert_eq!(records[0].contained_in.as_deref(), Some("dotfiles"));
        assert_eq!(records[0].subpath.as_deref(), Some(Path::new("inner")));
        assert!(records[1].contained_in.is_none() && records[1].alias_of.is_none());
        // the duplicate of a contained root is itself contained, not an alias
        assert_eq!(records[2].contained_in.as_deref(), Some("dotfiles"));
        let same = plan_roots(&[root("config", &outer), root("dotfiles", &outer)]);
        assert_eq!(same[1].alias_of.as_deref(), Some("config"));
        assert_eq!(records[3].skipped.as_deref(), Some("missing"));
        assert!(refused(Path::new("/")));
        assert!(refused(&canonical(&dirs::HOME)));
    }

    #[test]
    fn large_and_special_files_are_excluded() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("small"), "ok").unwrap();
        let big = std::fs::File::create(config.join("big")).unwrap();
        big.set_len(MAX_FILE_BYTES + 1).unwrap();
        #[cfg(unix)]
        nix::unistd::mkfifo(&config.join("fifo"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        let repo = shadow(tmp.path());
        let result = repo
            .snapshot(
                &[root("config", &config)],
                None,
                3,
                SnapshotPhase::Before,
                "test",
            )
            .unwrap();
        let names = files_of(&repo, &result.commit, "config")
            .into_iter()
            .map(|(_, p)| p)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["small".to_string()]);
        assert!(
            result.warnings.iter().any(|w| w.contains("big")),
            "{:?}",
            result.warnings
        );
        assert_eq!(result.roots[0].files, 1);
    }

    #[test]
    fn diff_between_snapshots() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("zshrc"), "one\n").unwrap();
        let repo = shadow(tmp.path());
        let a = repo
            .snapshot(
                &[root("config", &config)],
                None,
                5,
                SnapshotPhase::Before,
                "a",
            )
            .unwrap();
        std::fs::write(config.join("zshrc"), "two\n").unwrap();
        let b = repo
            .snapshot(
                &[root("config", &config)],
                None,
                5,
                SnapshotPhase::After,
                "b",
            )
            .unwrap();
        let same = repo.diff(&a.tree, &a.tree, &DiffOpts::default()).unwrap();
        assert!(!same.changed && same.output.is_empty());
        let stat = repo.diff(&a.tree, &b.tree, &DiffOpts::default()).unwrap();
        assert!(stat.changed);
        assert!(String::from_utf8_lossy(&stat.output).contains("config/zshrc"));
        let patch = repo
            .diff(
                &a.tree,
                &b.tree,
                &DiffOpts {
                    patch: true,
                    paths: Some(("config/zshrc".into(), "config/zshrc".into())),
                    ..Default::default()
                },
            )
            .unwrap();
        let text = String::from_utf8_lossy(&patch.output);
        assert!(text.contains("-one") && text.contains("+two"), "{text}");
        let missing = repo
            .diff(
                &a.tree,
                &b.tree,
                &DiffOpts {
                    paths: Some(("config/nope".into(), "config/nope".into())),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("config/nope is not in either snapshot"),
            "{missing}"
        );
        // a path on one side only is compared against an empty object
        std::fs::write(config.join("added"), "added\n").unwrap();
        let c = repo
            .snapshot(
                &[root("config", &config)],
                None,
                6,
                SnapshotPhase::After,
                "c",
            )
            .unwrap();
        let added = repo
            .diff(
                &b.tree,
                &c.tree,
                &DiffOpts {
                    patch: true,
                    paths: Some(("config/added".into(), "config/added".into())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(added.changed);
        let text = String::from_utf8_lossy(&added.output);
        assert!(text.contains("+added"), "{text}");
        let removed = repo
            .diff(
                &c.tree,
                &b.tree,
                &DiffOpts {
                    patch: true,
                    paths: Some(("config/added".into(), "config/added".into())),
                    ..Default::default()
                },
            )
            .unwrap();
        let text = String::from_utf8_lossy(&removed.output);
        assert!(text.contains("-added"), "{text}");
    }

    #[test]
    fn empty_and_pruned_snapshots() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        let repo = shadow(tmp.path());
        let empty = repo
            .snapshot(
                &[root("config", &config)],
                None,
                4,
                SnapshotPhase::Before,
                "empty",
            )
            .unwrap();
        assert!(files_of(&repo, &empty.commit, "config").is_empty());
        repo.delete_refs(4);
        repo.delete_refs(99);
        repo.gc().unwrap();
        assert!(
            Command::new("git")
                .args(["rev-parse", "--verify", "-q", "refs/generations/4/before"])
                .current_dir(repo.dir())
                .output()
                .map(|out| !out.status.success())
                .unwrap()
        );
    }
}
