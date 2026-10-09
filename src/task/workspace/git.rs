use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};

use eyre::{Result, eyre};

use crate::git::Git;

const DEFAULT_BASE: &str = "HEAD~1";
const DEFAULT_HEAD: &str = "HEAD";

/// Which kinds of change feed the affected calculation.
///
/// Selecting no source is the same as selecting all of them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AffectedSources {
    pub committed: bool,
    pub uncommitted: bool,
    pub untracked: bool,
}

impl AffectedSources {
    fn is_default(self) -> bool {
        !(self.committed || self.uncommitted || self.untracked)
    }

    fn wants_working_tree(self) -> bool {
        self.uncommitted || self.untracked
    }
}

/// Git revisions used to discover changed workspace paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceGitRevisions {
    pub base: String,
    pub head: String,
    sources: AffectedSources,
}

/// Changed workspace paths, remembering which source each came from so a path's
/// before and after content is read from the right side.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceChanges {
    pub paths: BTreeSet<PathBuf>,
    committed: BTreeSet<PathBuf>,
    working_tree: BTreeSet<PathBuf>,
    merge_base: Option<String>,
    head: String,
}

impl WorkspaceChanges {
    /// A path changed in the committed range starts at the merge base; any other starts at `HEAD`.
    pub fn file_before(&self, git: &Git, path: &Path) -> Result<Option<String>> {
        match (&self.merge_base, self.committed.contains(path)) {
            (Some(merge_base), true) => git.file_at_revision(merge_base, path),
            _ => git.file_at_revision(DEFAULT_HEAD, path),
        }
    }

    /// A path from an uncommitted or untracked source ends in the working tree; a path that only
    /// changed in the committed range ends at the head revision.
    pub fn file_after(&self, git: &Git, path: &Path) -> Result<Option<String>> {
        if self.working_tree.contains(path) {
            git.file_in_worktree(path)
        } else {
            git.file_at_revision(&self.head, path)
        }
    }
}

impl WorkspaceGitRevisions {
    /// Resolves explicit revisions, mise environment overrides, CI metadata,
    /// and finally the local `HEAD~1...HEAD` default, in that order.
    pub fn resolve(base: Option<&str>, head: Option<&str>, sources: AffectedSources) -> Self {
        Self::resolve_with(base, head, sources, |name| env::var(name).ok())
    }

    fn resolve_with(
        base: Option<&str>,
        head: Option<&str>,
        sources: AffectedSources,
        get_env: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let base = nonempty(base)
            .map(str::to_string)
            .or_else(|| env_value(&get_env, "MISE_AFFECTED_BASE"))
            .or_else(|| ci_base(&get_env))
            .unwrap_or_else(|| DEFAULT_BASE.to_string());
        // The source env vars are read here rather than bound to the flags, because a flag
        // bound to an env var would count as given and demand `--affected` on every `mise run`.
        let sources = AffectedSources {
            committed: sources.committed || env_enabled(&get_env, "MISE_AFFECTED_COMMITTED"),
            uncommitted: sources.uncommitted || env_enabled(&get_env, "MISE_AFFECTED_UNCOMMITTED"),
            untracked: sources.untracked || env_enabled(&get_env, "MISE_AFFECTED_UNTRACKED"),
        };
        let head = nonempty(head)
            .map(str::to_string)
            .or_else(|| env_value(&get_env, "MISE_AFFECTED_HEAD"))
            .or_else(|| ci_head(&get_env))
            .unwrap_or_else(|| DEFAULT_HEAD.to_string());
        Self {
            base,
            head,
            sources,
        }
    }

    /// Collects workspace-relative paths changed in the selected sources.
    ///
    /// The working tree only counts when `head` is the current checkout. By
    /// default a different `head` leaves a pure commit range; asking for
    /// uncommitted or untracked files explicitly alongside one is an error.
    pub fn changed_paths(&self, workspace_root: &Path) -> Result<WorkspaceChanges> {
        let git = Git::new(workspace_root);
        let on_checkout = git.same_commit(&self.head, DEFAULT_HEAD)?;
        let sources = if self.sources.is_default() {
            AffectedSources {
                committed: true,
                uncommitted: on_checkout,
                untracked: on_checkout,
            }
        } else if self.sources.wants_working_tree() && !on_checkout {
            return Err(eyre!(
                "--affected-uncommitted and --affected-untracked need the head revision to be the current checkout, but head is {}",
                self.head
            ));
        } else {
            self.sources
        };

        let mut committed = BTreeSet::new();
        let mut working_tree = BTreeSet::new();
        let mut merge_base = None;
        if sources.committed {
            committed = git.changed_paths(&self.base, &self.head)?;
            merge_base = Some(git.merge_base(&self.base, &self.head)?);
        }
        if sources.uncommitted {
            working_tree.extend(git.uncommitted_paths()?);
        }
        if sources.untracked {
            working_tree.extend(git.untracked_paths()?);
        }

        let paths = committed.union(&working_tree).cloned().collect();
        Ok(WorkspaceChanges {
            paths,
            committed,
            working_tree,
            merge_base,
            head: self.head.clone(),
        })
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
        WorkspaceGitRevisions::resolve_with(base, head, AffectedSources::default(), |name| {
            values.get(name).cloned()
        })
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
                sources: AffectedSources::default(),
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
    fn source_environment_variables_select_sources() {
        let revisions = resolve_with_env(
            None,
            None,
            &[
                ("MISE_AFFECTED_COMMITTED", "1"),
                ("MISE_AFFECTED_UNTRACKED", "true"),
                ("MISE_AFFECTED_UNCOMMITTED", "0"),
            ],
        );

        assert_eq!(
            revisions.sources,
            AffectedSources {
                committed: true,
                uncommitted: false,
                untracked: true,
            }
        );
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

        let revisions = WorkspaceGitRevisions::resolve(
            Some("HEAD~1"),
            Some("HEAD"),
            AffectedSources::default(),
        );

        assert_eq!(
            revisions.changed_paths(root).unwrap().paths,
            BTreeSet::from([
                PathBuf::from("nested/keep.txt"),
                PathBuf::from("nested/space name.txt"),
                PathBuf::from("new.txt"),
                PathBuf::from("old.txt"),
            ])
        );
        assert_eq!(
            revisions.changed_paths(&root.join("nested")).unwrap().paths,
            BTreeSet::from([PathBuf::from("keep.txt"), PathBuf::from("space name.txt"),])
        );
    }

    fn repo_with_working_tree_changes() -> tempfile::TempDir {
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
        std::fs::write(root.join("committed.txt"), "0\n").unwrap();
        std::fs::write(root.join("staged.txt"), "0\n").unwrap();
        std::fs::write(root.join("unstaged.txt"), "0\n").unwrap();
        std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "initial"]);
        std::fs::write(root.join("committed.txt"), "1\n").unwrap();
        git(&["commit", "-q", "-am", "committed"]);
        std::fs::write(root.join("staged.txt"), "1\n").unwrap();
        git(&["add", "staged.txt"]);
        std::fs::write(root.join("unstaged.txt"), "1\n").unwrap();
        std::fs::write(root.join("untracked.txt"), "1\n").unwrap();
        std::fs::write(root.join("ignored.txt"), "1\n").unwrap();
        temp
    }

    /// Resolves with no ambient environment, so CI's own `GITHUB_SHA` and friends cannot leak in.
    fn local_revisions(sources: AffectedSources) -> WorkspaceGitRevisions {
        WorkspaceGitRevisions::resolve_with(None, None, sources, |_| None)
    }

    fn paths(names: &[&str]) -> BTreeSet<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn working_tree_changes_join_the_committed_range_by_default() {
        let repo = repo_with_working_tree_changes();
        let revisions = local_revisions(AffectedSources::default());

        let changes = revisions.changed_paths(repo.path()).unwrap();

        assert_eq!(
            changes.paths,
            paths(&[
                "committed.txt",
                "staged.txt",
                "unstaged.txt",
                "untracked.txt"
            ])
        );
        assert_eq!(
            changes.working_tree,
            paths(&["staged.txt", "unstaged.txt", "untracked.txt"])
        );
    }

    #[test]
    fn each_source_flag_limits_the_calculation() {
        let repo = repo_with_working_tree_changes();
        let only = |committed, uncommitted, untracked| {
            local_revisions(AffectedSources {
                committed,
                uncommitted,
                untracked,
            })
            .changed_paths(repo.path())
            .unwrap()
        };

        let committed = only(true, false, false);
        assert_eq!(committed.paths, paths(&["committed.txt"]));
        assert!(committed.working_tree.is_empty());
        assert_eq!(
            only(false, true, false).paths,
            paths(&["staged.txt", "unstaged.txt"])
        );
        assert_eq!(only(false, false, true).paths, paths(&["untracked.txt"]));
        assert_eq!(
            only(false, true, true).paths,
            paths(&["staged.txt", "unstaged.txt", "untracked.txt"])
        );
        assert!(only(false, true, true).committed.is_empty());
    }

    #[test]
    fn a_head_other_than_the_checkout_stays_a_pure_commit_range() {
        let repo = repo_with_working_tree_changes();
        let revisions = WorkspaceGitRevisions::resolve(
            Some("HEAD~1"),
            Some("HEAD~1"),
            AffectedSources::default(),
        );

        let changes = revisions.changed_paths(repo.path()).unwrap();

        assert_eq!(changes.paths, paths(&[]));
        assert!(changes.working_tree.is_empty());

        let explicit = WorkspaceGitRevisions::resolve(
            Some("HEAD~1"),
            Some("HEAD~1"),
            AffectedSources {
                uncommitted: true,
                ..AffectedSources::default()
            },
        );
        assert!(explicit.changed_paths(repo.path()).is_err());
    }

    #[test]
    fn a_lockfile_edit_is_read_from_the_working_tree_only_when_that_source_is_enabled() {
        let repo = repo_with_working_tree_changes();
        let git = Git::new(repo.path());
        let revisions = local_revisions(AffectedSources {
            committed: true,
            untracked: true,
            ..AffectedSources::default()
        });

        let changes = revisions.changed_paths(repo.path()).unwrap();

        // Edited in the working tree but not selected: still reads the committed content.
        assert_eq!(changes.paths, paths(&["committed.txt", "untracked.txt"]));
        assert!(!changes.paths.contains(Path::new("unstaged.txt")));
        assert_eq!(
            changes
                .file_after(&git, Path::new("unstaged.txt"))
                .unwrap()
                .as_deref(),
            Some("0\n")
        );
        assert_eq!(
            changes
                .file_after(&git, Path::new("untracked.txt"))
                .unwrap()
                .as_deref(),
            Some("1\n")
        );
        assert_eq!(
            changes
                .file_before(&git, Path::new("committed.txt"))
                .unwrap()
                .as_deref(),
            Some("0\n")
        );
        assert_eq!(
            changes
                .file_before(&git, Path::new("untracked.txt"))
                .unwrap(),
            None
        );
    }
}
