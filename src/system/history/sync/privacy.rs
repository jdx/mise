//! What never leaves the machine, and how outgoing representations are
//! filtered: file content, journal blobs, coverage, changes, descriptions.
//! Statements read "private unless explicitly overridden": a per-file
//! `[dotfiles]` entry with `share = true` or `backup = true` is the only
//! way a private file travels, and every override is listed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eyre::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::system::history::journal::JournalEntry;
use crate::system::history::shadow::HistoryRepo;
use crate::system::history::store::Checkpoint;

/// Names that usually hold secrets: reported at `origin set` with the
/// `track … --no-share --no-backup` line for each.
pub(crate) const SECRET_NAME_GLOBS: &[&str] = &[
    "*.pem",
    "*.key",
    "id_*",
    "*token*",
    ".env*",
    "*secret*",
    "age.txt",
    ".netrc",
    "credentials*",
    "*.kdbx",
    "*.gpg",
];

fn secret_set() -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for glob in SECRET_NAME_GLOBS {
        builder.add(Glob::new(glob).expect("static secret globs"));
    }
    builder.build().expect("static secret globs")
}

/// The paths whose file names look like secrets.
pub(crate) fn secret_names<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<PathBuf> {
    let set = secret_set();
    paths
        .into_iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| set.is_match(Path::new(name)))
        })
        .map(Path::to_path_buf)
        .collect()
}

/// Whether a setup-branch path is private by default: `*.local.toml`
/// anywhere, or a credential store at the configuration root.
pub(crate) fn is_private_branch_path(branch_path: &str) -> bool {
    let name = branch_path.rsplit('/').next().unwrap_or(branch_path);
    if name.ends_with(".local.toml") {
        return true;
    }
    let at_config_root =
        !branch_path.starts_with("sources/") && !branch_path.starts_with("tracked/");
    at_config_root && crate::system::history::tracked::is_credential_name(name)
}

/// A masked copy of a checkpoint record for upload: private paths leave no
/// trace beyond their count.
pub(crate) fn mask_checkpoint(checkpoint: &Checkpoint, private: &BTreeSet<String>) -> Checkpoint {
    let mut masked = checkpoint.clone();
    let hidden = |path: &str| {
        private.iter().any(|p| {
            path == p
                || path
                    .strip_prefix(p.as_str())
                    .is_some_and(|r| r.starts_with('/'))
        })
    };
    let mut count = 0usize;
    masked.tree.coverage.entries.retain(|entry| {
        let keep = entry.mode != "private" && !hidden(&entry.path);
        if !keep {
            count += 1;
        }
        keep
    });
    for list in [
        &mut masked.changes.added,
        &mut masked.changes.modified,
        &mut masked.changes.removed,
    ] {
        let before = list.len();
        list.retain(|path| !hidden(path));
        count += before - list.len();
    }
    if let Some(operation) = &mut masked.operation {
        for entry in &mut operation.journal {
            if let JournalEntry::PathChanged { path, .. } = entry
                && hidden(&crate::file::display_path(path))
            {
                count += 1;
                *entry = JournalEntry::Note {
                    message: "a private path (not included)".to_string(),
                };
            }
        }
        operation.affected.retain(|path| !hidden(path));
        operation.directories.retain(|path| !hidden(path));
    }
    if count > 0 {
        masked.description = format!(
            "{} ({count} private file(s) not included)",
            masked.description
        );
        masked.summary = format!("{} ({count} private file(s) not included)", masked.summary);
    }
    masked
}

/// A committed private file found in the setup branch's history.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CommittedPrivate {
    pub commit: String,
    pub path: String,
}

/// Inspects the fetched branch's history (newest first, capped) for
/// committed private content: `*.local.toml`, credential stores, and the
/// given `share = false` destinations. Rewriting history is the user's
/// decision, never mise's; this only says what is there.
pub(crate) fn committed_private(
    repo: &HistoryRepo,
    head: &str,
    unshared: &[String],
    limit: usize,
) -> Result<Vec<CommittedPrivate>> {
    let mut found = vec![];
    for commit in repo.rev_list(head, limit)? {
        for path in repo.changed_names(&commit)? {
            if is_private_branch_path(&path) || unshared.iter().any(|u| u == &path) {
                found.push(CommittedPrivate {
                    commit: commit.clone(),
                    path,
                });
            }
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names_match_by_file_name() {
        let paths = [
            PathBuf::from("/home/u/.ssh/id_ed25519"),
            PathBuf::from("/home/u/.config/app/api_token.json"),
            PathBuf::from("/home/u/.config/hypr/bindings.lua"),
            PathBuf::from("/home/u/.netrc"),
        ];
        let found = secret_names(paths.iter().map(PathBuf::as_path));
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(!found.contains(&PathBuf::from("/home/u/.config/hypr/bindings.lua")));
    }

    #[test]
    fn private_branch_paths() {
        assert!(is_private_branch_path("config.local.toml"));
        assert!(is_private_branch_path("conf.d/work.local.toml"));
        assert!(is_private_branch_path("github_tokens.toml"));
        assert!(!is_private_branch_path("config.toml"));
        assert!(!is_private_branch_path(
            "tracked/home/.config/github_tokens.toml"
        ));
    }
}
