//! The `!@gitignore` entry in task `sources`.
//!
//! The entry drops the files git ignores from what the entries before it
//! matched. A file git tracks is never dropped, even when an ignore rule
//! matches it, since git does not ignore tracked files either. Only the
//! repository's own rules apply: every `.gitignore` in the work tree and
//! `.git/info/exclude`. The user's global excludes file is left out so that
//! what counts as a source is the same on every machine. Inside a submodule or
//! another nested repository, that repository's index and rules apply, as in
//! git.

use crate::task::task_source_checker::lexical_normalize;
use gix::bstr::BString;
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The `sources` entry that drops git-ignored files.
pub(crate) const GITIGNORE_ENTRY: &str = "!@gitignore";

/// Whether `entry` is the `!@gitignore` entry rather than a pattern.
pub fn is_gitignore_entry(entry: &str) -> bool {
    entry == GITIGNORE_ENTRY
}

/// Answers whether git ignores a path in one work tree.
pub(crate) struct GitIgnored {
    /// The work tree root, lexically normalized.
    workdir: PathBuf,
    /// The work tree root with symlinks resolved, for paths that arrive
    /// resolved (the task cache audit reads them from strace).
    canonical_workdir: Option<PathBuf>,
    index: gix::worktree::Index,
    ignore_case: bool,
    info_exclude: Gitignore,
    /// The rules of each directory's `.gitignore`, read on first use.
    dirs: Mutex<HashMap<PathBuf, Arc<Gitignore>>>,
    /// The nested repositories (submodules and the like) below the work tree,
    /// keyed by their path relative to it and opened on first use.
    nested: Mutex<HashMap<PathBuf, Option<Arc<GitIgnored>>>>,
}

impl GitIgnored {
    /// Find the work tree that contains `dir`. Returns `None` outside a git
    /// work tree, where git ignores nothing.
    pub(crate) fn discover(dir: &Path) -> Option<Self> {
        let dir = absolute(dir);
        let repo = match gix::discover_opts(
            &dir,
            Default::default(),
            gix::open::Options::isolated(),
        ) {
            Ok(repo) => repo,
            Err(err) => {
                debug!(
                    "{GITIGNORE_ENTRY}: {} is not in a git work tree, so no source is ignored: {err}",
                    dir.display()
                );
                return None;
            }
        };
        let Some(workdir) = repo.workdir() else {
            debug!(
                "{GITIGNORE_ENTRY}: the repository at {} is bare",
                dir.display()
            );
            return None;
        };
        let workdir = lexical_normalize(&absolute(workdir));
        let index = match repo.index_or_empty() {
            Ok(index) => index,
            Err(err) => {
                warn!(
                    "{GITIGNORE_ENTRY}: could not read the git index of {}, so no source is ignored: {err}",
                    workdir.display()
                );
                return None;
            }
        };
        let ignore_case = repo
            .config_snapshot()
            .boolean("core.ignoreCase")
            .unwrap_or(false);
        let info_exclude = read_rules(
            &workdir,
            &repo.common_dir().join("info").join("exclude"),
            ignore_case,
        );
        Some(Self {
            canonical_workdir: workdir.canonicalize().ok().filter(|c| *c != workdir),
            workdir,
            index,
            ignore_case,
            info_exclude,
            dirs: Mutex::new(HashMap::new()),
            nested: Mutex::new(HashMap::new()),
        })
    }

    /// Whether git ignores `path`. Paths outside the work tree are not ignored.
    ///
    /// A path that exists as a directory is checked as one even when the
    /// caller says otherwise, since a rule such as `out/` matches only
    /// directories and `sources` globs also match directories.
    pub(crate) fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let path = lexical_normalize(&absolute(path));
        let is_dir = is_dir || path.is_dir();
        let Some(relative) = self.relative(&path) else {
            return false;
        };
        let parts = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part),
                _ => None,
            })
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return false;
        }
        // Git keeps its own files out of the work tree it reports.
        if parts.iter().any(|part| *part == ".git") {
            return true;
        }
        if self.is_tracked(&relative, is_dir) {
            return false;
        }
        // Below a submodule the outer repository tracks, the submodule's own
        // index and rules decide, even when an outer rule ignores a directory
        // above it.
        let mut prefix = PathBuf::new();
        for part in &parts[..parts.len() - 1] {
            prefix.push(part);
            if self.is_gitlink(&prefix)
                && let Some(nested) = self.nested_repo(&prefix)
            {
                return nested.is_ignored(&path, is_dir);
            }
        }
        // Git does not look inside an ignored directory, so a rule cannot
        // re-include a file below one.
        let mut prefix = PathBuf::new();
        for (i, part) in parts.iter().enumerate() {
            let parent = prefix.clone();
            prefix.push(part);
            let last = i + 1 == parts.len();
            let as_dir = !last || is_dir;
            // A submodule is tracked as a single index entry, which no rule
            // can ignore.
            let gitlink = as_dir && self.is_gitlink(&prefix);
            if !gitlink && self.rules_ignore(&parent, &prefix, as_dir) == Some(true) {
                return true;
            }
            // Below a nested repository, its own index and rules decide. A
            // `.git` that does not open as one is an ordinary entry to git,
            // so the walk goes on with this work tree's rules.
            if !last
                && self.workdir.join(&prefix).join(".git").exists()
                && let Some(nested) = self.nested_repo(&prefix)
            {
                return nested.is_ignored(&path, is_dir);
            }
        }
        false
    }

    fn nested_repo(&self, relative: &Path) -> Option<Arc<GitIgnored>> {
        let mut nested = self.nested.lock().unwrap_or_else(|e| e.into_inner());
        nested
            .entry(relative.to_path_buf())
            .or_insert_with(|| {
                // Discovery walks upward, so it only counts when it finds a
                // work tree rooted at this very directory.
                let dir = self.workdir.join(relative);
                Self::discover(&dir)
                    .filter(|repo| repo.workdir == dir)
                    .map(Arc::new)
            })
            .clone()
    }

    fn relative(&self, path: &Path) -> Option<PathBuf> {
        if let Ok(relative) = path.strip_prefix(&self.workdir) {
            return Some(relative.to_path_buf());
        }
        let canonical = self.canonical_workdir.as_ref()?;
        path.strip_prefix(canonical).ok().map(Path::to_path_buf)
    }

    /// Whether the index tracks `relative` as a submodule.
    fn is_gitlink(&self, relative: &Path) -> bool {
        index_path(relative)
            .and_then(|key| self.index.entry_by_path(key.as_ref()).map(|e| e.mode))
            .is_some_and(|mode| mode == gix::index::entry::Mode::COMMIT)
    }

    fn is_tracked(&self, relative: &Path, is_dir: bool) -> bool {
        let Some(key) = index_path(relative) else {
            return false;
        };
        if is_dir {
            let mut prefix = key;
            prefix.push(b'/');
            self.index
                .prefixed_entries(prefix.as_ref())
                .is_some_and(|entries| !entries.is_empty())
        } else {
            self.index.entry_by_path(key.as_ref()).is_some()
        }
    }

    /// What the rules say about `relative`, whose parent directory is
    /// `parent`: `Some(true)` when the deciding rule ignores it, `Some(false)`
    /// when it re-includes it, and `None` when no rule matches. A deeper
    /// `.gitignore` wins over a shallower one, and every `.gitignore` wins over
    /// `.git/info/exclude`.
    fn rules_ignore(&self, parent: &Path, relative: &Path, is_dir: bool) -> Option<bool> {
        let path = self.workdir.join(relative);
        for dir in parent.ancestors() {
            let rules = self.dir_rules(&self.workdir.join(dir));
            if let Some(ignored) = decision(rules.matched(&path, is_dir)) {
                return Some(ignored);
            }
        }
        decision(self.info_exclude.matched(&path, is_dir))
    }

    fn dir_rules(&self, dir: &Path) -> Arc<Gitignore> {
        let mut dirs = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
        dirs.entry(dir.to_path_buf())
            .or_insert_with(|| Arc::new(read_rules(dir, &dir.join(".gitignore"), self.ignore_case)))
            .clone()
    }
}

fn decision<T>(m: Match<T>) -> Option<bool> {
    match m {
        Match::Ignore(_) => Some(true),
        Match::Whitelist(_) => Some(false),
        Match::None => None,
    }
}

/// Read the ignore rules in `file`, anchored at `dir`. A missing file has no
/// rules.
fn read_rules(dir: &Path, file: &Path, ignore_case: bool) -> Gitignore {
    if !file.is_file() {
        return Gitignore::empty();
    }
    let mut builder = GitignoreBuilder::new(dir);
    if let Err(err) = builder.case_insensitive(ignore_case) {
        debug!("{GITIGNORE_ENTRY}: {err}");
    }
    if let Some(err) = builder.add(file) {
        warn!(
            "{GITIGNORE_ENTRY}: could not read every rule in {}: {err}",
            file.display()
        );
    }
    builder.build().unwrap_or_else(|err| {
        warn!(
            "{GITIGNORE_ENTRY}: could not use the rules in {}: {err}",
            file.display()
        );
        Gitignore::empty()
    })
}

/// The path as the git index spells it: the path's bytes, `/`-separated and
/// relative to the work tree root. `None` only for a Windows path that is not
/// valid Unicode, which git cannot track.
fn index_path(relative: &Path) -> Option<BString> {
    let bytes = gix::path::into_bstr(relative).ok()?;
    Some(gix::path::to_unix_separators_on_windows(bytes).into_owned())
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "core.excludesFile=",
            ])
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        git(tmp.path(), &["init", "-q"]);
        tmp
    }

    #[test]
    fn ignores_what_the_rules_ignore() {
        let tmp = repo();
        let root = tmp.path();
        write(&root.join(".gitignore"), "out/\n*.log\n!keep.log\n");
        write(&root.join("pkg/.gitignore"), "local.txt\n");
        write(&root.join(".git/info/exclude"), "scratch/\n");
        let ignored = GitIgnored::discover(root).unwrap();
        assert!(ignored.is_ignored(&root.join("pkg/out/generated.js"), false));
        assert!(ignored.is_ignored(&root.join("pkg/debug.log"), false));
        assert!(!ignored.is_ignored(&root.join("pkg/keep.log"), false));
        assert!(ignored.is_ignored(&root.join("pkg/local.txt"), false));
        assert!(!ignored.is_ignored(&root.join("local.txt"), false));
        assert!(ignored.is_ignored(&root.join("scratch/a"), false));
        assert!(ignored.is_ignored(&root.join(".git/HEAD"), false));
        assert!(!ignored.is_ignored(&root.join("pkg/src/a.txt"), false));
        assert!(ignored.is_ignored(&root.join("pkg/out"), true));
    }

    #[test]
    fn a_rule_cannot_reinclude_below_an_ignored_directory() {
        let tmp = repo();
        let root = tmp.path();
        write(&root.join(".gitignore"), "out/\n!out/keep.js\n");
        let ignored = GitIgnored::discover(root).unwrap();
        assert!(ignored.is_ignored(&root.join("out/keep.js"), false));
    }

    #[test]
    fn tracked_files_are_not_ignored() {
        let tmp = repo();
        let root = tmp.path();
        write(&root.join("out/tracked.js"), "x");
        git(root, &["add", "out/tracked.js"]);
        write(&root.join(".gitignore"), "out/\n");
        write(&root.join("out/untracked.js"), "x");
        let ignored = GitIgnored::discover(root).unwrap();
        assert!(!ignored.is_ignored(&root.join("out/tracked.js"), false));
        assert!(ignored.is_ignored(&root.join("out/untracked.js"), false));
        assert!(!ignored.is_ignored(&root.join("out"), true));
    }

    #[test]
    fn a_nested_repository_uses_its_own_index_and_rules() {
        let tmp = repo();
        let root = tmp.path();
        write(&root.join(".gitignore"), "*.txt\nsub\n");
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        git(&sub, &["init", "-q"]);
        write(&sub.join(".gitignore"), "gen/\n");
        write(&sub.join("gen/tracked.js"), "x");
        write(&sub.join("gen/untracked.js"), "x");
        write(&sub.join("notes.txt"), "x");
        git(&sub, &["add", "-f", "gen/tracked.js"]);
        git(&sub, &["commit", "-q", "-m", "init"]);
        // Track `sub` in the outer repository as a submodule entry.
        git(root, &["add", "-f", "sub"]);
        let ignored = GitIgnored::discover(root).unwrap();
        assert!(!ignored.is_ignored(&sub.join("gen/tracked.js"), false));
        assert!(ignored.is_ignored(&sub.join("gen/untracked.js"), false));
        // The outer `*.txt` rule does not reach into the nested repository,
        // and the outer `sub` rule does not ignore a tracked submodule.
        assert!(!ignored.is_ignored(&sub.join("notes.txt"), false));
        assert!(!ignored.is_ignored(&sub, true));
    }

    #[test]
    fn a_tracked_submodule_below_an_ignored_directory_keeps_its_files() {
        let tmp = repo();
        let root = tmp.path();
        write(&root.join(".gitignore"), "vendor/\n");
        let sub = root.join("vendor/sub");
        fs::create_dir_all(&sub).unwrap();
        git(&sub, &["init", "-q"]);
        write(&sub.join("src.js"), "x");
        git(&sub, &["add", "src.js"]);
        git(&sub, &["commit", "-q", "-m", "init"]);
        git(root, &["add", "-f", "vendor/sub"]);
        write(&root.join("vendor/other.js"), "x");
        let ignored = GitIgnored::discover(root).unwrap();
        assert!(!ignored.is_ignored(&sub.join("src.js"), false));
        assert!(ignored.is_ignored(&root.join("vendor/other.js"), false));
    }

    #[cfg(unix)]
    #[test]
    fn a_tracked_symlink_below_an_ignored_directory_is_not_a_submodule() {
        let tmp = repo();
        let root = tmp.path();
        let other = root.join("other");
        fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-q"]);
        write(&other.join("file.js"), "x");
        fs::create_dir_all(root.join("lib")).unwrap();
        std::os::unix::fs::symlink("../other", root.join("lib/link")).unwrap();
        git(root, &["add", "lib/link"]);
        write(&root.join(".gitignore"), "lib/\n");
        let ignored = GitIgnored::discover(root).unwrap();
        // `lib/link` is a tracked symlink, not a submodule, so `lib/` still
        // ignores what is below it.
        assert!(ignored.is_ignored(&root.join("lib/link/file.js"), false));
    }

    #[test]
    fn a_stray_git_entry_does_not_hide_the_outer_rules() {
        let tmp = repo();
        let root = tmp.path();
        write(&root.join(".gitignore"), "out/\n");
        write(&root.join("pkg/.git"), "not a repository\n");
        let ignored = GitIgnored::discover(root).unwrap();
        assert!(ignored.is_ignored(&root.join("pkg/out/generated.js"), false));
        assert!(!ignored.is_ignored(&root.join("pkg/src/a.js"), false));
    }

    #[cfg(unix)]
    #[test]
    fn a_tracked_file_with_a_non_utf8_name_is_not_ignored() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let tmp = repo();
        let root = tmp.path();
        let name = OsStr::from_bytes(b"out/caf\xe9.js");
        write(&root.join(name), "x");
        git(root, &["add", "out"]);
        write(&root.join(".gitignore"), "out/\n");
        let ignored = GitIgnored::discover(root).unwrap();
        assert!(!ignored.is_ignored(&root.join(name), false));
    }

    #[test]
    fn the_entry_applies_to_sources_outside_the_root() {
        use crate::task::task_source_checker::{build_source_matcher, is_source};
        let tmp = repo();
        let root = tmp.path();
        write(&root.join(".gitignore"), "out/\n");
        let pkg = root.join("pkg");
        fs::create_dir_all(&pkg).unwrap();
        let is = |sources: &[&str], path: &str| {
            let sources = sources.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            is_source(
                &build_source_matcher(&pkg, &pkg, &sources),
                &root.join(path),
            )
        };
        let sources = ["../shared/**", GITIGNORE_ENTRY];
        assert!(is(&sources, "shared/src/a.js"));
        assert!(!is(&sources, "shared/out/b.js"));
        assert!(is(&[GITIGNORE_ENTRY, "../shared/**"], "shared/out/b.js"));
        let braced = ["{../shared,src}/**", GITIGNORE_ENTRY];
        assert!(!is(&braced, "shared/out/b.js"));
        assert!(is(&braced, "shared/src/a.js"));
        // A later braced entry adds an ignored file outside the root back.
        let later = ["src/**", GITIGNORE_ENTRY, "{../shared,src}/**"];
        assert!(is(&later, "shared/out/input.js"));
        assert!(!is(&later, "pkg/out/c.js"));
        let both = ["{../shared,src}/**", GITIGNORE_ENTRY, "{../shared,src}/**"];
        assert!(is(&both, "shared/out/a.js"));
        // Without the entry, a file the glob found outside the root is kept.
        assert!(is(&["../shared/**"], "shared/out/b.js"));
    }

    #[test]
    fn nothing_is_ignored_outside_a_work_tree() {
        let tmp = repo();
        let root = tmp.path();
        write(&root.join(".gitignore"), "*\n");
        let ignored = GitIgnored::discover(root).unwrap();
        assert!(!ignored.is_ignored(Path::new("/elsewhere/file"), false));
    }

    #[test]
    fn the_entry_applies_in_order() {
        use crate::task::task_source_checker::{build_source_matcher, is_source};
        let tmp = repo();
        let root = tmp.path();
        write(&root.join(".gitignore"), "out/\ndist/\n");
        let is = |sources: &[&str], path: &str| {
            let sources = sources.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            is_source(
                &build_source_matcher(root, root, &sources),
                &root.join(path),
            )
        };
        let sources = ["pkg/**", GITIGNORE_ENTRY, "pkg/dist/**"];
        assert!(is(&sources, "pkg/src/a.txt"));
        assert!(!is(&sources, "pkg/out/generated.js"));
        assert!(is(&sources, "pkg/dist/bundle.js"));
        // Without the entry, or before every pattern, it drops nothing.
        assert!(is(&["pkg/**"], "pkg/out/generated.js"));
        assert!(is(&[GITIGNORE_ENTRY, "pkg/**"], "pkg/out/generated.js"));
        // A `!` entry after a re-included path still excludes.
        assert!(!is(
            &["pkg/**", GITIGNORE_ENTRY, "pkg/dist/**", "!pkg/dist/*.map"],
            "pkg/dist/bundle.js.map"
        ));
    }
}
