use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::git::Git;

const DEFAULT_BASE: &str = "HEAD~1";
const DEFAULT_HEAD: &str = "HEAD";

/// Git revisions used to discover changed workspace paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceGitRevisions {
    pub base: String,
    pub head: String,
}

impl WorkspaceGitRevisions {
    /// Resolves explicit revisions, mise environment overrides, CI metadata,
    /// and finally the local `HEAD~1...HEAD` default, in that order.
    pub fn resolve(base: Option<&str>, head: Option<&str>) -> Self {
        Self::resolve_with(base, head, |name| env::var(name).ok())
    }

    fn resolve_with(
        base: Option<&str>,
        head: Option<&str>,
        get_env: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let base = nonempty(base)
            .map(str::to_string)
            .or_else(|| env_value(&get_env, "MISE_AFFECTED_BASE"))
            .or_else(|| ci_base(&get_env))
            .unwrap_or_else(|| DEFAULT_BASE.to_string());
        let head = nonempty(head)
            .map(str::to_string)
            .or_else(|| env_value(&get_env, "MISE_AFFECTED_HEAD"))
            .or_else(|| ci_head(&get_env))
            .unwrap_or_else(|| DEFAULT_HEAD.to_string());
        Self { base, head }
    }

    /// Collects workspace-relative paths changed between the configured revisions.
    pub fn changed_paths(&self, workspace_root: &Path) -> Result<BTreeSet<PathBuf>> {
        Git::new(workspace_root).changed_paths(&self.base, &self.head)
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn env_value(get_env: &impl Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    get_env(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_enabled(get_env: &impl Fn(&str) -> Option<String>, name: &str) -> bool {
    env_value(get_env, name)
        .is_some_and(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
}

fn ci_base(get_env: &impl Fn(&str) -> Option<String>) -> Option<String> {
    if env_enabled(get_env, "GITHUB_ACTIONS") {
        return env_value(get_env, "GITHUB_BASE_REF").map(remote_branch);
    }
    if env_enabled(get_env, "GITLAB_CI") {
        return env_value(get_env, "CI_MERGE_REQUEST_DIFF_BASE_SHA").or_else(|| {
            env_value(get_env, "CI_MERGE_REQUEST_TARGET_BRANCH_NAME").map(remote_branch)
        });
    }
    None
}

fn ci_head(get_env: &impl Fn(&str) -> Option<String>) -> Option<String> {
    if env_enabled(get_env, "GITHUB_ACTIONS") {
        return env_value(get_env, "GITHUB_SHA");
    }
    if env_enabled(get_env, "GITLAB_CI") {
        return env_value(get_env, "CI_COMMIT_SHA");
    }
    None
}

fn remote_branch(branch: String) -> String {
    let branch = branch.strip_prefix("refs/heads/").unwrap_or(&branch);
    if branch.starts_with("refs/remotes/") || branch.starts_with("origin/") {
        branch.to_string()
    } else {
        format!("origin/{branch}")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::process::Command;

    use super::*;

    fn resolve_with_env(
        base: Option<&str>,
        head: Option<&str>,
        values: &[(&str, &str)],
    ) -> WorkspaceGitRevisions {
        let values = values
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>();
        WorkspaceGitRevisions::resolve_with(base, head, |name| values.get(name).cloned())
    }

    #[test]
    fn explicit_revisions_override_environment_and_ci_defaults() {
        let revisions = resolve_with_env(
            Some("release-base"),
            Some("release-head"),
            &[
                ("MISE_AFFECTED_BASE", "env-base"),
                ("MISE_AFFECTED_HEAD", "env-head"),
                ("GITHUB_ACTIONS", "true"),
                ("GITHUB_BASE_REF", "main"),
                ("GITHUB_SHA", "github-head"),
            ],
        );

        assert_eq!(
            revisions,
            WorkspaceGitRevisions {
                base: "release-base".to_string(),
                head: "release-head".to_string(),
            }
        );
    }

    #[test]
    fn mise_environment_overrides_ci_defaults() {
        let revisions = resolve_with_env(
            None,
            None,
            &[
                ("MISE_AFFECTED_BASE", "env-base"),
                ("MISE_AFFECTED_HEAD", "env-head"),
                ("GITLAB_CI", "true"),
                ("CI_MERGE_REQUEST_DIFF_BASE_SHA", "gitlab-base"),
                ("CI_COMMIT_SHA", "gitlab-head"),
            ],
        );

        assert_eq!(revisions.base, "env-base");
        assert_eq!(revisions.head, "env-head");
    }

    #[test]
    fn github_and_gitlab_metadata_supply_ci_defaults() {
        let github = resolve_with_env(
            None,
            None,
            &[
                ("GITHUB_ACTIONS", "true"),
                ("GITHUB_BASE_REF", "refs/heads/main"),
                ("GITHUB_SHA", "github-head"),
            ],
        );
        let gitlab = resolve_with_env(
            None,
            None,
            &[
                ("GITLAB_CI", "true"),
                ("CI_MERGE_REQUEST_DIFF_BASE_SHA", "gitlab-base"),
                ("CI_COMMIT_SHA", "gitlab-head"),
            ],
        );

        assert_eq!(github.base, "origin/main");
        assert_eq!(github.head, "github-head");
        assert_eq!(gitlab.base, "gitlab-base");
        assert_eq!(gitlab.head, "gitlab-head");
    }

    #[test]
    fn local_defaults_compare_head_to_its_first_parent() {
        let revisions = resolve_with_env(None, None, &[]);

        assert_eq!(revisions.base, DEFAULT_BASE);
        assert_eq!(revisions.head, DEFAULT_HEAD);
    }

    #[test]
    fn changed_paths_are_relative_and_include_both_sides_of_renames() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["-c", "init.defaultBranch=main", "init", "-q"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(root.join("old.txt"), "old\n").unwrap();
        std::fs::write(root.join("nested/keep.txt"), "before\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "initial"]);
        std::fs::rename(root.join("old.txt"), root.join("new.txt")).unwrap();
        std::fs::write(root.join("nested/keep.txt"), "after\n").unwrap();
        std::fs::write(root.join("nested/space name.txt"), "new\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "change"]);

        let revisions = WorkspaceGitRevisions::resolve(Some("HEAD~1"), Some("HEAD"));

        assert_eq!(
            revisions.changed_paths(root).unwrap(),
            BTreeSet::from([
                PathBuf::from("nested/keep.txt"),
                PathBuf::from("nested/space name.txt"),
                PathBuf::from("new.txt"),
                PathBuf::from("old.txt"),
            ])
        );
        assert_eq!(
            revisions.changed_paths(&root.join("nested")).unwrap(),
            BTreeSet::from([PathBuf::from("keep.txt"), PathBuf::from("space name.txt"),])
        );
    }
}
