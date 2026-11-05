//! Selects tasks for `mise run --affected` outside a monorepo, by matching the changed files
//! against each task's `sources`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, eyre};
use serde::{Serialize, Serializer};

use crate::config::Config;
use crate::dirs;
use crate::git::Git;
use crate::task::Task;
use crate::task::task_source_checker::{
    build_source_matcher, is_source, lexical_normalize, parent_dir_pops_glob, task_cwd,
};
use crate::task::workspace::git::{WorkspaceChanges, WorkspaceGitRevisions};

/// Why `--affected` selected a task in a project without a monorepo root.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AffectedTaskReason {
    /// A changed file matches the task's `sources`.
    ChangedSource {
        #[serde(serialize_with = "serialize_lossy")]
        path: PathBuf,
    },
    /// A changed file is a config file that defines the task.
    TaskConfig {
        #[serde(serialize_with = "serialize_lossy")]
        path: PathBuf,
    },
    /// A changed file is the script the task's `file` names.
    TaskFile {
        #[serde(serialize_with = "serialize_lossy")]
        path: PathBuf,
    },
    /// The task declares no `sources`, so any change may affect it.
    NoSources,
    /// A `sources` entry uses the task's arguments, which are not known yet.
    ArgumentSources,
}

/// The changed files of each Git checkout a task's files are in, read once per checkout.
///
/// The model: a checkout's changed files are what Git reports in it, with each changed
/// submodule's own changed files added under the submodule's path, recursively. Every
/// glob and path is matched against that one list, so a pattern reaches into submodules
/// like any other directory. A submodule therefore never counts as a checkout of its own:
/// its files belong to the outermost superproject. Only an unrelated checkout nested in
/// another one, such as an ignored clone, is read separately.
pub struct AffectedRepos<'a> {
    revisions: &'a WorkspaceGitRevisions,
    /// Canonical checkout roots and their changes; the first is the default checkout.
    repos: Vec<(PathBuf, WorkspaceChanges)>,
    /// Which entry of `repos` holds each directory looked up so far.
    dirs: BTreeMap<PathBuf, usize>,
}

impl<'a> AffectedRepos<'a> {
    /// Reads the changes of the checkout `mise` runs in, or failing that, the one that
    /// holds the project root (a config above the checkout, outside Git).
    pub fn new(revisions: &'a WorkspaceGitRevisions, config: &Config) -> Result<Self> {
        let root = dirs::CWD
            .iter()
            .chain(config.project_root.iter())
            .find_map(|dir| checkout_root(dir))
            .ok_or_else(|| eyre!("--affected needs a Git repository"))?;
        let mut changes = revisions.changed_paths(&root)?;
        add_submodule_changes(&root, &mut changes, 0);
        Ok(Self {
            revisions,
            repos: vec![(root, changes)],
            dirs: BTreeMap::new(),
        })
    }

    /// Describes what was compared in the default checkout, for an explanation header.
    pub fn compared(&self) -> String {
        self.repos[0].1.compared(self.revisions)
    }

    /// The index of the checkout that holds `dir`, or of the default checkout when no
    /// checkout does.
    fn index_for(&mut self, dir: &Path) -> usize {
        if let Some(index) = self.dirs.get(dir) {
            return *index;
        }
        let index = match checkout_root(dir) {
            Some(root) => match self.repos.iter().position(|(r, _)| *r == root) {
                Some(index) => index,
                None => {
                    let mut changes = self.nested_changes(&root);
                    add_submodule_changes(&root, &mut changes, 0);
                    self.repos.push((root, changes));
                    self.repos.len() - 1
                }
            },
            None => 0,
        };
        self.dirs.insert(dir.to_path_buf(), index);
        index
    }

    /// Changes in an unrelated checkout nested in the default one.
    ///
    /// It shares no history with the default checkout, so it tries the same revision
    /// names. When they do not resolve there, only the working tree counts, and only if it
    /// counted in the default checkout.
    fn nested_changes(&self, root: &Path) -> WorkspaceChanges {
        let default_changes = &self.repos[0].1;
        let mut candidates =
            vec![default_changes.revisions_for(&self.revisions.base, &self.revisions.head)];
        candidates.extend(default_changes.working_tree_revisions());
        for revisions in candidates {
            match revisions.changed_paths(root) {
                Ok(changes) => return changes,
                Err(err) => debug!(
                    "--affected: cannot compare {}...{} in {}: {err}",
                    revisions.base,
                    revisions.head,
                    root.display()
                ),
            }
        }
        warn!(
            "--affected: no changes could be read in {}; its tasks count as unchanged",
            root.display()
        );
        WorkspaceChanges::default()
    }
}

/// Adds the changed files of each changed submodule of the checkout at `root` to
/// `changes`, under the submodule's path, following the rule on
/// [`WorkspaceChanges::submodule_changes`]: the commits the checkout records for the
/// submodule at its merge base and head are diffed directly, and the submodule's working
/// tree counts only when it is checked out at the head commit.
///
/// Git lists a changed submodule as one path, a directory with its own `.git`. A
/// submodule removed in the range has no head commit and stays that single path.
fn add_submodule_changes(root: &Path, changes: &mut WorkspaceChanges, depth: usize) {
    // Submodules nest rarely; the bound only stops a cycle of symlinks.
    if depth > 16 {
        return;
    }
    let git = Git::new(root);
    let submodules = changes
        .paths
        .iter()
        .filter(|path| root.join(path).join(".git").exists())
        .cloned()
        .collect::<Vec<_>>();
    for submodule in submodules {
        let Ok(Some(to)) = git.object_at(changes.head(), &submodule) else {
            continue;
        };
        let from = changes
            .merge_base()
            .and_then(|merge_base| git.object_at(merge_base, &submodule).ok().flatten());
        let sub_root = root.join(&submodule);
        match changes.submodule_changes(&sub_root, from.as_deref(), &to) {
            Ok(mut sub_changes) => {
                add_submodule_changes(&sub_root, &mut sub_changes, depth + 1);
                changes.add_submodule_paths(&submodule, &sub_changes.paths);
            }
            Err(err) => debug!(
                "--affected: cannot compare the commits recorded for {}: {err}",
                sub_root.display()
            ),
        }
    }
}

/// The canonical root of the outermost checkout holding `dir`, or its nearest existing
/// ancestor: a submodule's superproject, all the way up, so its files are matched in the
/// same list as the rest of the project.
/// dunce, because Git does not accept the `\\?\` paths `canonicalize` gives on Windows.
fn checkout_root(dir: &Path) -> Option<PathBuf> {
    let dir = dir.ancestors().find(|dir| dir.is_dir())?;
    let mut root = Git::new(dir).toplevel().ok()?;
    while let Ok(Some(parent)) = Git::new(&root).superproject() {
        root = parent;
    }
    Some(dunce::canonicalize(&root).unwrap_or(root))
}

/// Paths come from Git and need not be UTF-8, which serde refuses for `PathBuf`.
fn serialize_lossy<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}

/// Returns why the changed files affect `task`, or an empty set when they do not.
///
/// Changes are read from every Git checkout that holds one of the task's files: its
/// directory, its config files and `file` script, and the leading directories of its
/// `sources`. A submodule's files count as part of its outermost superproject (see
/// [`AffectedRepos`]), so paths are reported relative to that checkout. A file outside every checkout
/// counts against the checkout `mise` runs in. Freshness checks treat a task without
/// `sources` as always stale, so such a task is affected by any change in those
/// checkouts; with no change at all, nothing is.
pub async fn affected_task_reasons(
    task: &Task,
    config: &Arc<Config>,
    repos: &mut AffectedRepos<'_>,
) -> Result<BTreeSet<AffectedTaskReason>> {
    let mut reasons = BTreeSet::new();
    let cwd = task_cwd(task, config).await?;
    let cwd = lexical_normalize(&if cwd.is_absolute() {
        cwd
    } else {
        dirs::CWD.clone().unwrap_or_default().join(cwd)
    });
    let config_paths = task
        .config_sources()
        .into_iter()
        .map(|source| {
            if source.is_absolute() {
                source.to_path_buf()
            } else {
                cwd.join(source)
            }
        })
        .collect::<Vec<_>>();
    // A `file` that cannot render yet, such as one using the task's arguments, is left to
    // fail when the task runs; it only drops the script check here, as in freshness.
    let file = match task.file_path(config).await {
        Ok(file) => file.map(|file| lexical_normalize(&file)),
        Err(err) => {
            debug!(
                "--affected: cannot resolve the file of {}: {err}",
                task.name
            );
            None
        }
    };
    let argument_sources = task
        .sources
        .iter()
        .any(|source| super::tera_template_has_usage_ref(source));
    // Git reports each file by its path in the repository, with no symlink in it. The
    // up-to-date check reaches the same files through whatever links the task's directory
    // and patterns go through, so resolve those links before matching.
    let real_cwd = resolve_dirs(&cwd);
    let patterns = if argument_sources {
        Vec::new()
    } else {
        task.sources
            .iter()
            .flat_map(|source| resolve_pattern(&cwd, source))
            .collect::<Vec<_>>()
    };

    let mut dirs = vec![cwd.clone()];
    dirs.extend(
        config_paths
            .iter()
            .chain(&file)
            .filter_map(|p| p.parent().map(Path::to_path_buf)),
    );
    dirs.extend(patterns.iter().map(|pattern| literal_dir(pattern)));
    let indexes = dirs
        .iter()
        .map(|dir| repos.index_for(dir))
        .collect::<BTreeSet<_>>();

    let mut any_change = false;
    for index in indexes {
        let (repo_root, changes) = &repos.repos[index];
        let changes = &changes.paths;
        if changes.is_empty() {
            continue;
        }
        any_change = true;
        reasons.extend(
            changed_sources(repo_root, &real_cwd, &patterns, changes)
                .into_iter()
                .map(|path| AffectedTaskReason::ChangedSource { path }),
        );
        for path in &config_paths {
            for path in changed_definition(repo_root, path, changes) {
                reasons.insert(AffectedTaskReason::TaskConfig { path });
            }
        }
        if let Some(file) = &file {
            for path in changed_definition(repo_root, file, changes) {
                reasons.insert(AffectedTaskReason::TaskFile { path });
            }
        }
    }
    if any_change && task.sources.is_empty() {
        reasons.insert(AffectedTaskReason::NoSources);
    } else if any_change && argument_sources {
        reasons.insert(AffectedTaskReason::ArgumentSources);
    }
    Ok(reasons)
}

/// The directory a resolved pattern starts from: its components before the first glob.
fn literal_dir(pattern: &str) -> PathBuf {
    let body = pattern.strip_prefix('!').unwrap_or(pattern);
    let mut dir = PathBuf::new();
    for component in Path::new(body).components() {
        if component
            .as_os_str()
            .to_string_lossy()
            .contains(['*', '?', '[', '{'])
        {
            break;
        }
        dir.push(component);
    }
    dir
}

/// Returns the changed paths that name the file at `path`: the file itself, which Git
/// reports when it is a symlink that changed, and the file it links to.
fn changed_definition(repo_root: &Path, path: &Path, changes: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut candidates = vec![resolve_parent(path)];
    if let Ok(target) = dunce::canonicalize(path) {
        candidates.push(target);
    }
    candidates
        .into_iter()
        .filter_map(|candidate| {
            candidate
                .strip_prefix(repo_root)
                .ok()
                .filter(|relative| changes.contains(*relative))
                .map(Path::to_path_buf)
        })
        .collect()
}

/// Resolves the symlinks in the existing part of `path` and appends the rest unchanged.
fn resolve_dirs(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(real) = dunce::canonicalize(ancestor) {
            return match path.strip_prefix(ancestor) {
                Ok(rest) if !rest.as_os_str().is_empty() => real.join(rest),
                _ => real,
            };
        }
    }
    path.to_path_buf()
}

/// Resolves the symlinks in the directories leading to `path`, keeping its last component,
/// so a symlinked file keeps the name Git reports it by.
fn resolve_parent(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => resolve_dirs(parent).join(name),
        _ => resolve_dirs(path),
    }
}

/// Rewrites a `sources` entry as an absolute pattern whose literal leading directories have
/// their symlinks resolved, so it selects files by the paths Git reports for them.
///
/// The part from the first glob component on stays as written. A pattern whose `..` would
/// collapse a glob component is returned unchanged, as the source matcher does.
fn resolve_pattern(task_cwd: &Path, pattern: &str) -> Vec<String> {
    let (prefix, body) = if pattern.starts_with("\\!") {
        return vec![pattern.to_string()];
    } else if let Some(rest) = pattern.strip_prefix('!') {
        ("!", rest)
    } else {
        ("", pattern)
    };
    let body_path = Path::new(body);
    if parent_dir_pops_glob(body_path) {
        return vec![pattern.to_string()];
    }
    let absolute = lexical_normalize(&task_cwd.join(body_path));
    let mut literal = PathBuf::new();
    let mut rest = PathBuf::new();
    for component in absolute.components() {
        let part = component.as_os_str();
        if rest.as_os_str().is_empty() && !part.to_string_lossy().contains(['*', '?', '[', '{']) {
            literal.push(part);
        } else {
            rest.push(part);
        }
    }
    let mut resolved = if !rest.as_os_str().is_empty() {
        vec![resolve_dirs(&literal).join(&rest)]
    } else if literal.is_dir() {
        vec![resolve_dirs(&literal)]
    } else {
        vec![resolve_parent(&literal)]
    };
    // A literal file that is a symlink is matched by both names: Git reports the link when
    // it is retargeted and the target when its contents change, and the up-to-date check
    // follows the link. A glob that matches a link is matched by the link's name only.
    if rest.as_os_str().is_empty()
        && literal.is_symlink()
        && let Ok(target) = dunce::canonicalize(&literal)
        && !resolved.contains(&target)
    {
        resolved.push(target);
    }
    let trailing_slash = if body.ends_with('/') { "/" } else { "" };
    resolved
        .into_iter()
        .map(|path| format!("{prefix}{}{trailing_slash}", path.to_string_lossy()))
        .collect()
}

/// Returns the paths in `changes`, relative to `match_root`, that `sources` select for a
/// task running in `task_cwd`.
fn changed_sources(
    match_root: &Path,
    task_cwd: &Path,
    sources: &[String],
    changes: &BTreeSet<PathBuf>,
) -> BTreeSet<PathBuf> {
    let matcher = build_source_matcher(match_root, task_cwd, sources);
    changes
        .iter()
        .filter(|path| is_source(&matcher, &match_root.join(path)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(task_cwd: &str, sources: &[&str], changes: &[&str]) -> Vec<String> {
        let sources = sources.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let changes = changes.iter().map(PathBuf::from).collect();
        changed_sources(Path::new("/repo"), Path::new(task_cwd), &sources, &changes)
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn sources_select_only_matching_changes() {
        assert_eq!(
            matches("/repo", &["src/**"], &["docs/index.md", "src/lib.rs"]),
            ["src/lib.rs"]
        );
    }

    #[test]
    fn negated_sources_exclude_changes() {
        assert_eq!(
            matches(
                "/repo",
                &["src/**", "!src/**/*.snap"],
                &["src/a.snap", "src/b.rs"]
            ),
            ["src/b.rs"]
        );
    }

    #[test]
    fn sources_are_relative_to_the_task_directory() {
        assert_eq!(
            matches(
                "/repo/app",
                &["src/**", "../shared/*.rs"],
                &["src/lib.rs", "app/src/lib.rs", "shared/util.rs"]
            ),
            ["app/src/lib.rs", "shared/util.rs"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn patterns_resolve_symlinked_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("real/src")).unwrap();
        std::os::unix::fs::symlink("real", root.join("link")).unwrap();
        std::fs::write(root.join("real/mise.toml"), "").unwrap();
        std::os::unix::fs::symlink("real/mise.toml", root.join("mise.toml")).unwrap();
        let link = root.join("link");

        assert_eq!(
            resolve_pattern(&link, "src/**"),
            [format!("{}", root.join("real/src/**").display())]
        );
        assert_eq!(
            resolve_pattern(&link, "!src/*.snap"),
            [format!("!{}", root.join("real/src/*.snap").display())]
        );
        // A symlinked file matches by its own name, the path Git reports when the link
        // changes, and by its target, the path Git reports when the contents change.
        assert_eq!(
            resolve_pattern(&root, "mise.toml"),
            [
                format!("{}", root.join("mise.toml").display()),
                format!("{}", root.join("real/mise.toml").display())
            ]
        );
        let changes = [PathBuf::from("mise.toml"), PathBuf::from("real/mise.toml")]
            .into_iter()
            .collect();
        assert_eq!(
            changed_definition(&root, &root.join("mise.toml"), &changes),
            [PathBuf::from("mise.toml"), PathBuf::from("real/mise.toml")]
        );
    }
}
