//! Selects tasks for `mise run --affected` outside a monorepo, by matching the changed files
//! against each task's `sources`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::Result;
use serde::Serialize;

use crate::config::Config;
use crate::dirs;
use crate::task::Task;
use crate::task::task_source_checker::{
    build_source_matcher, is_source, lexical_normalize, task_cwd,
};

/// Why `--affected` selected a task in a project without a monorepo root.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AffectedTaskReason {
    /// A changed file matches the task's `sources`.
    ChangedSource { path: PathBuf },
    /// A changed file defines the task.
    TaskConfig { path: PathBuf },
    /// The task declares no `sources`, so any change may affect it.
    NoSources,
    /// A `sources` entry uses the task's arguments, which are not known yet.
    ArgumentSources,
}

/// Returns why the changed files affect `task`, or an empty set when they do not.
///
/// `repo_root` is the canonical root of the Git working tree, and `changes` are paths
/// relative to it. Freshness checks treat a task without `sources` as always stale, so
/// such a task is affected by any change; with no change at all, nothing is.
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
    // Git prints the canonical root, but the task's patterns are written in the namespace
    // of its directory, which may reach the repository through a symlink. Match in the
    // task's namespace when one of its ancestors is the repository root.
    let (match_root, match_cwd) = match cwd
        .ancestors()
        .find(|dir| dir.canonicalize().is_ok_and(|dir| dir == repo_root))
    {
        Some(root) => (root.to_path_buf(), cwd.clone()),
        None => (
            repo_root.to_path_buf(),
            cwd.canonicalize().unwrap_or_else(|_| cwd.clone()),
        ),
    };
    reasons.extend(
        changed_sources(&match_root, &match_cwd, &task.sources, changes)
            .into_iter()
            .map(|path| AffectedTaskReason::ChangedSource { path }),
    );
    for config_source in task.config_sources() {
        let path = if config_source.is_absolute() {
            config_source.to_path_buf()
        } else {
            cwd.join(config_source)
        };
        let path = path.canonicalize().unwrap_or(path);
        if let Ok(relative) = path.strip_prefix(repo_root)
            && changes.contains(relative)
        {
            reasons.insert(AffectedTaskReason::TaskConfig {
                path: relative.to_path_buf(),
            });
        }
    }
    Ok(reasons)
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
}
