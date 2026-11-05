//! Selects tasks for `mise run --affected` outside a monorepo, by matching the changed files
//! against each task's `sources`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::Result;
use serde::{Serialize, Serializer};

use crate::config::Config;
use crate::dirs;
use crate::task::Task;
use crate::task::task_source_checker::{
    build_source_matcher, is_source, lexical_normalize, parent_dir_pops_glob, task_cwd,
};

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

/// Paths come from Git and need not be UTF-8, which serde refuses for `PathBuf`.
fn serialize_lossy<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}

/// Returns why the changed files affect `task`, or an empty set when they do not.
///
/// `repo_root` is the root of the Git working tree with symlinks resolved, and `changes`
/// are paths relative to it, as Git reports them. Freshness checks treat a task without
/// `sources` as always stale, so such a task is affected by any change; with no change at
/// all, nothing is.
pub async fn affected_task_reasons(
    task: &Task,
    config: &Arc<Config>,
    repo_root: &Path,
    changes: &BTreeSet<PathBuf>,
) -> Result<BTreeSet<AffectedTaskReason>> {
    let mut reasons = BTreeSet::new();
    if changes.is_empty() {
        return Ok(reasons);
    }
    if task.sources.is_empty() {
        reasons.insert(AffectedTaskReason::NoSources);
        return Ok(reasons);
    }
    if task
        .sources
        .iter()
        .any(|source| super::tera_template_has_usage_ref(source))
    {
        reasons.insert(AffectedTaskReason::ArgumentSources);
        return Ok(reasons);
    }
    let cwd = task_cwd(task, config).await?;
    let cwd = lexical_normalize(&if cwd.is_absolute() {
        cwd
    } else {
        dirs::CWD.clone().unwrap_or_default().join(cwd)
    });
    // Git reports each file by its path in the repository, with no symlink in it. The
    // up-to-date check reaches the same files through whatever links the task's directory
    // and patterns go through, so resolve those links before matching.
    let real_cwd = resolve_dirs(&cwd);
    let patterns = task
        .sources
        .iter()
        .map(|source| resolve_pattern(&cwd, source))
        .collect::<Vec<_>>();
    reasons.extend(
        changed_sources(repo_root, &real_cwd, &patterns, changes)
            .into_iter()
            .map(|path| AffectedTaskReason::ChangedSource { path }),
    );
    for config_source in task.config_sources() {
        let path = if config_source.is_absolute() {
            config_source.to_path_buf()
        } else {
            cwd.join(config_source)
        };
        for path in changed_definition(repo_root, &path, changes) {
            reasons.insert(AffectedTaskReason::TaskConfig { path });
        }
    }
    if let Some(file) = task.file_path(config).await? {
        for path in changed_definition(repo_root, &lexical_normalize(&file), changes) {
            reasons.insert(AffectedTaskReason::TaskFile { path });
        }
    }
    Ok(reasons)
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
fn resolve_pattern(task_cwd: &Path, pattern: &str) -> String {
    let (prefix, body) = if pattern.starts_with("\\!") {
        return pattern.to_string();
    } else if let Some(rest) = pattern.strip_prefix('!') {
        ("!", rest)
    } else {
        ("", pattern)
    };
    let body_path = Path::new(body);
    if parent_dir_pops_glob(body_path) {
        return pattern.to_string();
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
    let resolved = if !rest.as_os_str().is_empty() {
        resolve_dirs(&literal).join(rest)
    } else if literal.is_dir() {
        resolve_dirs(&literal)
    } else {
        resolve_parent(&literal)
    };
    let trailing_slash = if body.ends_with('/') { "/" } else { "" };
    format!("{prefix}{}{trailing_slash}", resolved.to_string_lossy())
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
            format!("{}", root.join("real/src/**").display())
        );
        assert_eq!(
            resolve_pattern(&link, "!src/*.snap"),
            format!("!{}", root.join("real/src/*.snap").display())
        );
        // A symlinked file keeps its own name, the path Git reports when the link changes.
        assert_eq!(
            resolve_pattern(&root, "mise.toml"),
            format!("{}", root.join("mise.toml").display())
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
