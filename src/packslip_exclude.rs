//! Keep the skill links mise made out of `git status`.
//!
//! The links point at this machine's installs, so a committed one breaks on
//! every other machine, and with `skills.auto_sync` on a global setting makes
//! them appear in every project at once. They are listed in the repository's
//! `info/exclude`, which belongs to this clone alone, in a block mise owns and
//! rewrites. Nothing in the working tree changes, and the skills beside them
//! that people wrote stay visible to git.
//!
//! A linked worktree is left alone: it shares `info/exclude` with the main
//! checkout and every other worktree, and a pattern there applies to all of
//! them. One worktree's links could then hide another's handwritten skills,
//! and each sync would rewrite the others' entries.

use std::path::{Component, Path, PathBuf};

use eyre::{Result, bail};

use crate::file;

/// Where `dir` is kept in the working tree of the repository containing it,
/// and that repository's `info/exclude`. `None` outside a repository.
struct Repo {
    /// `dir` from the working tree root, as an anchored pattern prefix.
    prefix: String,
    exclude: PathBuf,
}

/// The git directory of the repository whose working tree is `root`: its
/// `.git` directory, or the one a submodule's `.git` file names under an
/// enclosing repository's `.git/modules`. `None` for a linked worktree, whose
/// `info/exclude` is not its own, and for a `.git` that is a link or names a
/// directory anywhere else: a tree that did not come from git, an unpacked
/// archive for one, would otherwise choose where the block is written.
fn own_git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let meta = dot_git.symlink_metadata().ok()?;
    let git_dir = if meta.is_dir() {
        dot_git
    } else if meta.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        let git_dir = dunce::canonicalize(root.join(target)).ok()?;
        root.ancestors().skip(1).find(|up| {
            let up_git = up.join(".git");
            up_git.symlink_metadata().is_ok_and(|meta| meta.is_dir())
                && git_dir.starts_with(up_git.join("modules"))
        })?;
        git_dir
    } else {
        return None;
    };
    (!git_dir.join("commondir").exists()).then_some(git_dir)
}

fn repo_of(dir: &Path) -> Result<Option<Repo>> {
    let Ok(dir) = dunce::canonicalize(dir) else {
        return Ok(None);
    };
    let Some(root) = dir.ancestors().find(|p| p.join(".git").exists()) else {
        return Ok(None);
    };
    let Some(git_dir) = own_git_dir(root) else {
        return Ok(None);
    };
    let Ok(rel) = dir.strip_prefix(root) else {
        return Ok(None);
    };
    let mut prefix = String::new();
    for part in rel.components() {
        let Component::Normal(part) = part else {
            return Ok(None);
        };
        let Some(part) = part.to_str() else {
            return Ok(None);
        };
        prefix.push('/');
        prefix.push_str(&escape(part)?);
    }
    Ok(Some(Repo {
        prefix,
        exclude: git_dir.join("info").join("exclude"),
    }))
}

/// A name as a literal gitignore pattern. A line break would end the pattern
/// and start another, so a name holding one is refused.
fn escape(name: &str) -> Result<String> {
    if name.contains(['\n', '\r']) {
        bail!("{name:?} has a line break and cannot be written as an ignore pattern");
    }
    let mut out = String::with_capacity(name.len());
    let last = name.chars().count().saturating_sub(1);
    for (i, c) in name.chars().enumerate() {
        // Git drops trailing spaces from a pattern unless they are escaped.
        if matches!(c, '\\' | '*' | '?' | '[' | ']')
            || (i == 0 && matches!(c, '#' | '!' | ' '))
            || (i == last && c == ' ')
        {
            out.push('\\');
        }
        out.push(c);
    }
    Ok(out)
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
    let Some(repo) = repo_of(dir)? else {
        return Ok(());
    };
    let mut entries = names
        .into_iter()
        .map(|name| Ok(format!("{}/{}", repo.prefix, escape(name)?)))
        .collect::<Result<Vec<_>>>()?;
    if !entries.is_empty() {
        entries.push(format!("{}/{}", repo.prefix, escape(state_file)?));
    }
    let (begin, end) = markers(&repo.prefix);
    let lock_path = repo.exclude.with_extension("mise-lock");
    // Git makes `info` and the file in it as a plain directory and file. A
    // link at either, or at the lock, would take the block and the names in
    // it to wherever the link points, so it is never written through.
    for path in [
        repo.exclude.parent(),
        Some(&*repo.exclude),
        Some(&*lock_path),
    ] {
        if let Some(path) = path.filter(|path| file::is_symlink_or_junction(path)) {
            bail!("{} is a link", file::display_path(path));
        }
    }
    if let Some(parent) = repo.exclude.parent() {
        file::create_dir_all(parent)?;
    }
    // Two syncs, each for its own directory, would otherwise both read the
    // old file and the later write would drop the other's block.
    let mut lock = fslock::LockFile::open(&lock_path)?;
    lock.lock()?;
    // Only a file that is not there is empty. One that cannot be read, for
    // instance because a pattern in it is not UTF-8, is somebody's rules, and
    // writing over it would lose them.
    let before = match std::fs::read_to_string(&repo.exclude) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err.into()),
    };
    let after = with_block(&before, &begin, &end, &entries);
    if after != before {
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
        assert_eq!(escape("a*b").unwrap(), "a\\*b");
        assert_eq!(escape("!bang").unwrap(), "\\!bang");
        assert_eq!(escape("plain-name_1").unwrap(), "plain-name_1");
        // Git drops a trailing space unless it is escaped.
        assert_eq!(escape("demo ").unwrap(), "demo\\ ");
        // A line break would start another pattern.
        assert!(escape("demo\n*").is_err());
        assert!(escape("demo\r").is_err());
    }

    #[test]
    fn an_exclude_file_that_cannot_be_read_is_left_as_it_is() {
        let root = repo();
        std::fs::create_dir_all(root.path().join(".git/info")).unwrap();
        let exclude = root.path().join(".git/info/exclude");
        std::fs::write(&exclude, b"caf\xe9\n").unwrap();
        let dir = root.path().join("app/.agents/skills");
        assert!(exclude_links(&dir, ["a"], ".mise-skills.json").is_err());
        assert_eq!(std::fs::read(&exclude).unwrap(), b"caf\xe9\n");
    }

    #[test]
    fn a_linked_worktree_is_left_alone() {
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
        assert!(!main.path().join(".git/info").exists());
        assert!(!wt_git.join("info").exists());
    }

    #[test]
    fn a_submodule_keeps_its_own_exclude() {
        let super_repo = tempfile::tempdir().unwrap();
        let modules = super_repo.path().join(".git/modules/sub");
        std::fs::create_dir_all(&modules).unwrap();
        let work = super_repo.path().join("sub");
        std::fs::create_dir_all(work.join("skills")).unwrap();
        std::fs::write(work.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
        exclude_links(&work.join("skills"), ["a"], ".mise-skills.json").unwrap();
        let text = std::fs::read_to_string(modules.join("info/exclude")).unwrap();
        assert!(text.contains("/skills/a\n"));
    }

    #[test]
    fn a_git_file_naming_a_directory_elsewhere_is_left_alone() {
        let elsewhere = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        std::fs::write(
            work.path().join(".git"),
            format!("gitdir: {}\n", elsewhere.path().display()),
        )
        .unwrap();
        // Nor one inside the tree, which is not where git keeps a submodule.
        let nested = work.path().join("nested");
        std::fs::create_dir_all(nested.join(".gd")).unwrap();
        std::fs::write(nested.join(".git"), "gitdir: .gd\n").unwrap();
        for dir in [work.path().join("skills"), nested.join("skills")] {
            std::fs::create_dir_all(&dir).unwrap();
            exclude_links(&dir, ["a"], ".mise-skills.json").unwrap();
        }
        assert!(!elsewhere.path().join("info").exists());
        assert!(!nested.join(".gd/info").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_git_directory_is_left_alone() {
        let elsewhere = repo();
        let work = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path().join(".git"), work.path().join(".git"))
            .unwrap();
        let dir = work.path().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        exclude_links(&dir, ["a"], ".mise-skills.json").unwrap();
        assert!(!elsewhere.path().join(".git/info").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_exclude_file_is_not_written_through() {
        let outside = tempfile::tempdir().unwrap();
        let theirs = outside.path().join("theirs");
        std::fs::write(&theirs, "export EDITOR=vi\n").unwrap();
        let dir = |root: &Path| root.join("app/.agents/skills");

        // The file itself, to a file that is there and to one that is not.
        for target in [&theirs, &outside.path().join("new")] {
            let root = repo();
            std::fs::create_dir_all(root.path().join(".git/info")).unwrap();
            std::os::unix::fs::symlink(target, root.path().join(".git/info/exclude")).unwrap();
            assert!(exclude_links(&dir(root.path()), ["a"], ".mise-skills.json").is_err());
        }
        // The directory it is in.
        let root = repo();
        std::os::unix::fs::symlink(outside.path(), root.path().join(".git/info")).unwrap();
        assert!(exclude_links(&dir(root.path()), ["a"], ".mise-skills.json").is_err());
        // The lock beside it.
        let root = repo();
        std::fs::create_dir_all(root.path().join(".git/info")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("lock"),
            root.path().join(".git/info/exclude.mise-lock"),
        )
        .unwrap();
        assert!(exclude_links(&dir(root.path()), ["a"], ".mise-skills.json").is_err());
        assert_eq!(exclude_of(root.path()), "");

        assert_eq!(
            std::fs::read_to_string(&theirs).unwrap(),
            "export EDITOR=vi\n"
        );
        let left: Vec<_> = std::fs::read_dir(outside.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["theirs"]);
    }

    #[test]
    fn outside_a_repository_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        exclude_links(dir.path(), ["a"], ".mise-skills.json").unwrap();
        assert!(!dir.path().join(".git").exists());
    }
}
