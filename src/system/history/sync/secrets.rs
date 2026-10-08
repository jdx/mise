//! Content scan of the history a sync is about to publish.
//!
//! The credential filter in capture looks at file names. A tracked `.bashrc`
//! passes it however many tokens get exported there, and a token removed from
//! the file stays in every earlier saved version. This is the last chance to
//! see that before the versions leave the machine.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use eyre::{Result, bail};
use regex::Regex;

use crate::system::history::shadow::HistoryRepo;

/// Larger files are not shell or app config; scanning them is not worth the time.
const MAX_SCANNED_BYTES: u64 = 1 << 20;

/// Provider token formats, which are specific enough to flag anywhere.
static TOKEN_FORMATS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|xox[abprs]-[A-Za-z0-9-]{10,})|-----BEGIN [A-Z ]*PRIVATE KEY-----",
    )
    .unwrap()
});

/// `NAME_KEY=value`, `api-token: "value"`, `password = value`. The name has to
/// end in the keyword, so `keybind = ...` is not one, and a value that only
/// refers to another variable is not a secret.
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(?:[a-z0-9_.-]*[_.-](?:key|token|secret|password|passwd)|token|secret|password|passwd|apikey)["']?\s*[:=]\s*["']?[^\s"'$#{(<\[][^\s"']{7,}"#,
    )
    .unwrap()
});

/// A line that looks like it holds a secret.
fn suspicious(line: &str) -> bool {
    TOKEN_FORMATS.is_match(line) || ASSIGNMENT.is_match(line)
}

/// Fail when a version that is not yet on the origin looks like it holds a
/// secret. `upstream` is the origin's head; with none, every version counts.
/// Anything the origin already has is not rescanned, so a leak that was
/// published cannot block every later sync. Values are never printed.
pub(crate) fn audit_unpublished(
    repo: &HistoryRepo,
    head: &str,
    upstream: Option<&str>,
) -> Result<()> {
    let commits = match upstream {
        Some(upstream) => repo.rev_list_after(head, upstream)?,
        None => repo.rev_list(head, usize::MAX)?,
    };
    if commits.is_empty() {
        return Ok(());
    }
    let mut seen: BTreeSet<String> = match upstream {
        Some(upstream) => repo
            .ls_tree(upstream)?
            .into_iter()
            .map(|entry| entry.oid)
            .collect(),
        None => BTreeSet::new(),
    };
    // Oldest first, so the report names the commit that introduced the secret.
    for commit in commits.iter().rev() {
        for entry in repo.ls_tree(commit)? {
            if !matches!(entry.mode.as_str(), "100644" | "100755")
                || entry.path.starts_with(".mise-history/")
                || entry.size.is_some_and(|size| size > MAX_SCANNED_BYTES)
                || !seen.insert(entry.oid.clone())
            {
                continue;
            }
            let bytes = repo.cat_object(&entry.oid)?;
            if bytes.contains(&0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            if let Some(line) = text.lines().position(suspicious) {
                bail!(
                    "cannot publish: line {} of {} in saved version {} looks like a secret. Removing it from the file does not remove it from saved versions. Encrypt the file with `encrypt = true`, remove that history (see https://mise.jdx.dev/dotfiles/encryption.html#remove-plaintext-from-history), or run `mise dot sync --allow-plaintext-history` to publish it anyway",
                    line + 1,
                    entry.path,
                    &commit[..commit.len().min(12)]
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::history::shadow::Overlay;

    #[test]
    fn flags_secrets_and_not_ordinary_config() {
        for line in [
            "export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "export OPENAI_API_KEY=\"abcd1234efgh\"",
            "password: hunter2hunter2",
            "AWS=AKIAABCDEFGHIJKLMNOP",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "secret = 'abcdefgh'",
        ] {
            assert!(suspicious(line), "{line}");
        }
        for line in [
            "keybind = ctrl+shift+c=copy_to_clipboard",
            "export EDITOR=nvim",
            "export GITHUB_TOKEN=$(gh auth token)",
            "export API_KEY=$API_KEY_FILE",
            "export API_KEY=",
            "bind = SUPER, K, exec, toggle-key",
            "alias tokens='wc -w'",
        ] {
            assert!(!suspicious(line), "{line}");
        }
    }

    fn commit(repo: &HistoryRepo, parent: Option<&str>, body: &[u8]) -> String {
        let tree = repo
            .compose(
                &repo.empty_object("tree").unwrap(),
                &[Overlay {
                    path: "home/.bashrc".into(),
                    object: Some(("100644".into(), repo.hash_blob(body).unwrap())),
                }],
            )
            .unwrap();
        repo.commit_tree(&tree, parent.into_iter().collect(), "save")
            .unwrap()
    }

    #[test]
    fn a_removed_secret_still_blocks_until_published() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let leaked = commit(&repo, None, b"export MY_TOKEN=abcdefghijkl\n");
        let removed = commit(&repo, Some(&leaked), b"alias ll=ls\n");

        let err = audit_unpublished(&repo, &removed, None).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("line 1 of home/.bashrc"), "{message}");
        assert!(!message.contains("abcdefghijkl"), "{message}");

        // once the origin has the leaking version it no longer blocks
        audit_unpublished(&repo, &removed, Some(&removed)).unwrap();
        audit_unpublished(&repo, &removed, Some(&leaked)).unwrap();
    }

    #[test]
    fn clean_history_and_binary_files_pass() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let clean = commit(&repo, None, b"alias ll=ls\n");
        audit_unpublished(&repo, &clean, None).unwrap();
        let binary = commit(&repo, Some(&clean), b"\0export MY_TOKEN=abcdefghijkl\n");
        audit_unpublished(&repo, &binary, Some(&clean)).unwrap();
    }
}
