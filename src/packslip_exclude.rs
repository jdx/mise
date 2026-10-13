//! Keep the skill links mise made out of `git status`.
//!
//! The links point at this machine's installs, so a committed one breaks on
//! every other machine, and with `skills.auto_sync` on a global setting makes
//! them appear in every project at once. They are listed in the repository's
//! `info/exclude`, which belongs to this clone alone, in a block mise owns and
//! rewrites. Nothing in the working tree changes, and the skills beside them
//! that people wrote stay visible to git.

use std::path::{Component, Path, PathBuf};

use eyre::Result;

use crate::file;

/// Where `dir` is kept in the working tree of the repository containing it,
/// and that repository's `info/exclude`. `None` outside a repository.
struct Repo {
    /// `dir` from the working tree root, as an anchored pattern prefix.
    prefix: String,
    exclude: PathBuf,
}

/// The directory git keeps shared state in for the repository at `dot_git`,
/// a `.git` directory or the file a linked worktree or submodule has instead.
fn common_dir(dot_git: &Path) -> Option<PathBuf> {
    let git_dir = if dot_git.is_dir() {
        dot_git.to_path_buf()
    } else {
        let text = std::fs::read_to_string(dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        dot_git.parent()?.join(target)
    };
    // A linked worktree has its own git dir and shares the main one's
    // `info/`, named relative to itself by `commondir`.
    match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(common) => Some(git_dir.join(common.trim())),
        Err(_) => Some(git_dir),
    }
}

fn repo_of(dir: &Path) -> Option<Repo> {
    let dir = dunce::canonicalize(dir).ok()?;
    let root = dir.ancestors().find(|p| p.join(".git").exists())?;
    let common = common_dir(&root.join(".git"))?;
    let mut prefix = String::new();
    for part in dir.strip_prefix(root).ok()?.components() {
        let Component::Normal(part) = part else {
            return None;
        };
        prefix.push('/');
        prefix.push_str(&escape(part.to_str()?));
    }
    Some(Repo {
        prefix,
        exclude: common.join("info").join("exclude"),
    })
}

/// A name as a literal gitignore pattern.
fn escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (i, c) in name.chars().enumerate() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']') || (i == 0 && matches!(c, '#' | '!' | ' ')) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn markers(prefix: &str) -> (String, String) {
    (
        format!("# >>> mise skills ({prefix})"),
        format!("# <<< mise skills ({prefix})"),
    )
}

/// `text` with the block between `begin` and `end` replaced by `entries`, or
/// removed when there are none, and every other line left as it was. A marker
/// with no end after it is one a person cut into: only the marker goes, and
/// what followed it stays theirs.
fn with_block(text: &str, begin: &str, end: &str, entries: &[String]) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    while let Some(start) = lines.iter().position(|line| *line == begin) {
        let stop = lines[start..].iter().position(|line| *line == end);
        lines.drain(start..stop.map_or(start + 1, |n| start + n + 1));
    }
    let mut out = lines.join("\n");
    if !entries.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(begin);
        for entry in entries {
            out.push('\n');
            out.push_str(entry);
        }
        out.push('\n');
        out.push_str(end);
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Make the repository ignore `names` in `dir`, and the `state_file` mise
/// records them in, and nothing else there. With no names, stop ignoring what
/// an earlier call listed. Does nothing outside a git repository.
pub(crate) fn exclude_links<'a>(
    dir: &Path,
    names: impl IntoIterator<Item = &'a str>,
    state_file: &str,
) -> Result<()> {
    let Some(repo) = repo_of(dir) else {
        return Ok(());
    };
    let mut entries: Vec<String> = names
        .into_iter()
        .map(|name| format!("{}/{}", repo.prefix, escape(name)))
        .collect();
    if !entries.is_empty() {
        entries.push(format!("{}/{}", repo.prefix, escape(state_file)));
    }
    let (begin, end) = markers(&repo.prefix);
    let before = std::fs::read_to_string(&repo.exclude).unwrap_or_default();
    let after = with_block(&before, &begin, &end, &entries);
    if after != before {
        if let Some(parent) = repo.exclude.parent() {
            file::create_dir_all(parent)?;
        }
        file::write_atomic(&repo.exclude, after)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".git")).unwrap();
        std::fs::create_dir_all(root.path().join("app/.agents/skills")).unwrap();
        root
    }

    fn exclude_of(root: &Path) -> String {
        std::fs::read_to_string(root.join(".git/info/exclude")).unwrap_or_default()
    }

    #[test]
    fn links_are_listed_anchored_beside_what_was_already_there() {
        let root = repo();
        std::fs::create_dir_all(root.path().join(".git/info")).unwrap();
        std::fs::write(root.path().join(".git/info/exclude"), "*.swp\n").unwrap();
        let dir = root.path().join("app/.agents/skills");
        exclude_links(&dir, ["aube", "hk"], ".mise-skills.json").unwrap();
        assert_eq!(
            exclude_of(root.path()),
            "*.swp\n\
             # >>> mise skills (/app/.agents/skills)\n\
             /app/.agents/skills/aube\n\
             /app/.agents/skills/hk\n\
             /app/.agents/skills/.mise-skills.json\n\
             # <<< mise skills (/app/.agents/skills)\n"
        );
    }

    #[test]
    fn the_block_follows_the_links_and_leaves_with_the_last_one() {
        let root = repo();
        let dir = root.path().join("app/.agents/skills");
        exclude_links(&dir, ["aube", "hk"], ".mise-skills.json").unwrap();
        exclude_links(&dir, ["hk"], ".mise-skills.json").unwrap();
        let text = exclude_of(root.path());
        assert!(text.contains("/app/.agents/skills/hk\n"));
        assert!(!text.contains("aube"));
        exclude_links(&dir, [], ".mise-skills.json").unwrap();
        assert_eq!(exclude_of(root.path()), "");
    }

    #[test]
    fn each_directory_keeps_its_own_block() {
        let root = repo();
        let a = root.path().join("app/.agents/skills");
        let b = root.path().join("app/.claude/skills");
        std::fs::create_dir_all(&b).unwrap();
        exclude_links(&a, ["one"], ".mise-skills.json").unwrap();
        exclude_links(&b, ["two"], ".mise-skills.json").unwrap();
        exclude_links(&a, [], ".mise-skills.json").unwrap();
        let text = exclude_of(root.path());
        assert!(text.contains("/app/.claude/skills/two\n"));
        assert!(!text.contains(".agents"));
    }

    #[test]
    fn a_marker_with_no_end_costs_only_the_marker() {
        let root = repo();
        std::fs::create_dir_all(root.path().join(".git/info")).unwrap();
        std::fs::write(
            root.path().join(".git/info/exclude"),
            "# >>> mise skills (/app/.agents/skills)\nmine\n",
        )
        .unwrap();
        let dir = root.path().join("app/.agents/skills");
        exclude_links(&dir, [], ".mise-skills.json").unwrap();
        assert_eq!(exclude_of(root.path()), "mine\n");
    }

    #[test]
    fn names_are_literal_patterns() {
        assert_eq!(escape("a*b"), "a\\*b");
        assert_eq!(escape("!bang"), "\\!bang");
        assert_eq!(escape("plain-name_1"), "plain-name_1");
    }

    #[test]
    fn a_linked_worktree_writes_to_the_shared_info_directory() {
        let main = tempfile::tempdir().unwrap();
        let wt_git = main.path().join(".git/worktrees/wt");
        std::fs::create_dir_all(&wt_git).unwrap();
        std::fs::write(wt_git.join("commondir"), "../..\n").unwrap();
        let work = tempfile::tempdir().unwrap();
        std::fs::write(
            work.path().join(".git"),
            format!("gitdir: {}\n", wt_git.display()),
        )
        .unwrap();
        let dir = work.path().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        exclude_links(&dir, ["a"], ".mise-skills.json").unwrap();
        let text =
            std::fs::read_to_string(main.path().join(".git/worktrees/wt/../../info/exclude"))
                .unwrap();
        assert!(text.contains("/skills/a\n"));
    }

    #[test]
    fn outside_a_repository_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        exclude_links(dir.path(), ["a"], ".mise-skills.json").unwrap();
        assert!(!dir.path().join(".git").exists());
    }
}
