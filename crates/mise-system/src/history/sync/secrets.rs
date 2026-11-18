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

use crate::history::shadow::HistoryRepo;

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
/// refers to another variable is not a secret. A quoted value may hold spaces, and a path or command
/// substitution is not a secret.
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(?:[a-z0-9_.-]*[_.-](?:key|token|secret|password|passwd)|token|secret|password|passwd|apikey)["']?\s*[:=]\s*(?:"(?P<dq>[^"$`~/\s][^"]{7,})|'(?P<sq>[^'$`~/\s][^']{7,})|(?P<bare>[^\s"'$`~/#{(<\[;][^\s"';]{7,}|[^\s"'$`~/#{(<\[;][^\s"';]{0,6};[^\s"']{0,256}))"#,
    )
    .unwrap()
});

/// A line that looks like it holds a secret. `allowed` are values the user
/// vouched for (`history.allow_secret_values`). The line is judged as it is;
/// a finding is set aside only when its whole value, or the whole token, is
/// one of them. A key name, part of a longer value, or anything else on the
/// line still counts.
fn suspicious(line: &str, allowed: &[String]) -> bool {
    // a shell line is several commands; judge each in turn, without nesting
    let mut rest = line;
    loop {
        match scan(rest, allowed) {
            Scan::Flagged => return true,
            Scan::Clean => return false,
            Scan::Next(from) => rest = &rest[from..],
        }
    }
}

/// What `scan` found in a line.
enum Scan {
    Flagged,
    Clean,
    /// Clean up to here; the rest of the line is another shell command.
    Next(usize),
}

fn scan(line: &str, allowed: &[String]) -> Scan {
    let approved = |found: &str| allowed.iter().any(|value| value == found);
    // Where approved values sit, so a token format inside one is covered too.
    let mut covered = Vec::new();
    let mut end = line.len();
    let mut next_command = false;
    for found in ASSIGNMENT.captures_iter(line) {
        let Some(value) = ["dq", "sq", "bare"]
            .iter()
            .find_map(|name| found.name(name))
        else {
            continue;
        };
        // inside a scalar that was approved as a whole
        let whole = found.get(0).map_or(0..0, |whole| whole.range());
        if covered
            .last()
            .is_some_and(|range: &std::ops::Range<usize>| {
                range.start <= whole.start && whole.end <= range.end
            })
        {
            continue;
        }
        let text = value.as_str();
        if found.name("bare").is_some() && text.chars().count() < 8 {
            // too short to be a secret on its own, as before the pattern spanned `;`
            continue;
        }
        // `"key": "value",` and `{ key = "value" }` are structured text, where a
        // comma or closing bracket ends the value. In shell, `KEY="value",more`
        // is one word, so there only whitespace or `;` does. Spaces or a colon
        // around the separator are the evidence; a quoted name alone is not.
        let before = &line[found.get(0).map_or(0, |whole| whole.start())..value.start()];
        let before = before.strip_suffix(['"', '\'']).unwrap_or(before);
        let structured = before.contains(|c: char| c == ':' || c.is_whitespace());
        // a word that continues right after the value (`"changeme"-more`,
        // `changeme'more'`) is still part of it. An unquoted value stops at a
        // shell separator (`KEY=value;next`), but in structured text a `;` is
        // part of the value, so the capture is incomplete.
        let bare = found.name("bare").is_some();
        let continues = if bare {
            let rest = &line[value.end()..];
            let tail = rest.trim_start();
            rest.starts_with(['"', '\''])
                // a plain scalar runs to the end of the line or a comment
                || (structured && !(tail.is_empty() || tail.starts_with('#')))
        } else {
            line[value.end()..]
                .get(1..)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|next| {
                    !next.is_whitespace()
                        && next != ';'
                        && !(structured && matches!(next, ',' | '}' | ']' | ')' | '#'))
                })
        };
        if bare && structured {
            // a plain scalar may hold spaces and `;` and runs to the end of the
            // line or a ` #` comment; approving it takes the whole scalar
            let rest = &line[value.start()..];
            let scalar = rest[..rest.find(" #").unwrap_or(rest.len())].trim_end();
            if approved(scalar) {
                covered.push(value.start()..value.start() + scalar.len());
                continue;
            }
        }
        if bare && !structured && text.contains(';') {
            // shell: `KEY=value;next` is two commands. Judge the value alone,
            // then the rest of the line as its own command, quoted values
            // included, which the capture stopped short of.
            let first = text.split(';').next().unwrap_or_default();
            if approved(first) && !continues {
                covered.push(value.start()..value.start() + first.len());
            } else if first.chars().count() >= 8 {
                return Scan::Flagged;
            }
            end = value.start() + first.len() + 1;
            next_command = true;
            break;
        } else if approved(text) && !continues {
            covered.push(value.range());
        } else {
            return Scan::Flagged;
        }
    }
    // both lists are in line order, so one forward cursor covers them
    let mut next = 0;
    let flagged = TOKEN_FORMATS.find_iter(&line[..end]).any(|found| {
        while covered
            .get(next)
            .is_some_and(|range| range.end <= found.start())
        {
            next += 1;
        }
        let inside = covered
            .get(next)
            .is_some_and(|range| range.start <= found.start() && found.end() <= range.end);
        !approved(found.as_str()) && !inside
    });
    if flagged {
        Scan::Flagged
    } else if next_command {
        Scan::Next(end)
    } else {
        Scan::Clean
    }
}

/// Fail when a version that is not yet on the origin looks like it holds a
/// secret. `upstream` is the origin's head; with none, every version counts.
/// Anything the origin already has is not rescanned, so a leak that was
/// published cannot block every later sync. Values are never printed.
/// `allowed` lists values that are known to be harmless, such as a placeholder.
pub(crate) fn audit_unpublished(
    repo: &HistoryRepo,
    head: &str,
    upstream: Option<&str>,
    allowed: &[String],
) -> Result<()> {
    let commits = match upstream {
        Some(upstream) => repo.rev_list_after(head, upstream)?,
        None => repo.rev_list(head, usize::MAX)?,
    };
    if commits.is_empty() {
        return Ok(());
    }
    // Contents the origin has anywhere in its history are already out, even
    // when they were removed and later saved again unchanged.
    let mut seen: BTreeSet<String> = match upstream {
        Some(upstream) => repo.reachable_objects(upstream)?,
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
            if let Some(line) = text.lines().position(|line| suspicious(line, allowed)) {
                bail!(
                    "cannot publish: line {} of {} in saved version {} looks like a secret. Removing it from the file does not remove it from saved versions. Rotate the secret and remove that history (see https://mise.jdx.dev/dotfiles/encryption.html#remove-plaintext-from-history), then set `encrypt = true` on the file so later saves are protected, run `mise dot sync --allow-plaintext-history` to publish it anyway, or list a harmless value in `history.allow_secret_values`",
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
    use crate::history::shadow::Overlay;

    #[test]
    fn flags_secrets_and_not_ordinary_config() {
        for line in [
            "export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "export OPENAI_API_KEY=\"abcd1234efgh\"",
            "password: hunter2hunter2",
            "password: abc;defghijkl",
            "password: ab;cdefghijklmnop",
            "password: ab;cdefg",
            "password: abcdefg;",
            r#"API_KEY=test;DEPLOY_TOKEN="abcdefghijkl1234""#,
            "API_KEY=test;DEPLOY_TOKEN=abcdefghijkl1234",
            "password: abcdefg; # note",
            "AWS=AKIAABCDEFGHIJKLMNOP",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "secret = 'abcdefgh'",
            "DB_PASSWORD=\"correct horse battery staple\"",
            "token: 'two words here'",
        ] {
            assert!(suspicious(line, &[]), "{line}");
        }
        for line in [
            "keybind = ctrl+shift+c=copy_to_clipboard",
            "export EDITOR=nvim",
            "export GITHUB_TOKEN=$(gh auth token)",
            "export API_KEY=$API_KEY_FILE",
            "export API_KEY=",
            "bind = SUPER, K, exec, toggle-key",
            "alias tokens='wc -w'",
            "export DB_PASSWORD=\"$(pass show db)\"",
            "export DB_PASSWORD=`pass show db`",
            "password_file = ~/.config/app/password.txt",
            "secret = \"/run/secrets/app-secret\"",
            "export DB_PASSWORD=\"$DB_PASSWORD_FILE contents\"",
        ] {
            assert!(!suspicious(line, &[]), "{line}");
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

        let err = audit_unpublished(&repo, &removed, None, &[]).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("line 1 of home/.bashrc"), "{message}");
        assert!(!message.contains("abcdefghijkl"), "{message}");

        // once the origin has the leaking version it no longer blocks
        audit_unpublished(&repo, &removed, Some(&removed), &[]).unwrap();
        audit_unpublished(&repo, &removed, Some(&leaked), &[]).unwrap();
    }

    #[test]
    fn contents_published_earlier_do_not_block_when_saved_again() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let leaked = commit(&repo, None, b"export MY_TOKEN=abcdefghijkl\n");
        let removed = commit(&repo, Some(&leaked), b"alias ll=ls\n");
        let published = commit(&repo, Some(&removed), b"alias la=ls\n");
        let resaved = commit(&repo, Some(&published), b"export MY_TOKEN=abcdefghijkl\n");
        // the origin has the leaking contents in an earlier commit, not its newest tree
        audit_unpublished(&repo, &resaved, Some(&published), &[]).unwrap();
    }

    #[test]
    fn clean_history_and_binary_files_pass() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let clean = commit(&repo, None, b"alias ll=ls\n");
        audit_unpublished(&repo, &clean, None, &[]).unwrap();
        let binary = commit(&repo, Some(&clean), b"\0export MY_TOKEN=abcdefghijkl\n");
        audit_unpublished(&repo, &binary, Some(&clean), &[]).unwrap();
    }

    #[test]
    fn an_allowed_value_is_blanked_and_nothing_else() {
        let allowed = vec!["ctx7_api_key".to_string()];
        let placeholder = "\"CONTEXT7_API_KEY\" = \"ctx7_api_key\"";
        assert!(suspicious(placeholder, &[]));
        assert!(!suspicious(placeholder, &allowed));
        // the same name with another value, and another secret on the line, still count
        assert!(suspicious(
            "\"CONTEXT7_API_KEY\" = \"ctx7_other_key\"",
            &allowed
        ));
        assert!(suspicious(
            "A_KEY=ctx7_api_key B_TOKEN=abcdefghijkl",
            &allowed
        ));
        assert!(!suspicious(
            placeholder,
            &["".to_string(), "ctx7_api_key".to_string()]
        ));
        // an approved string as a key name, or inside a different value, hides nothing
        for line in [
            "\"ctx7_api_key\" = \"abcdefghijkl\"",
            "ctx7_api_key_token = \"abcdefghijkl\"",
            "API_KEY = \"xctx7_api_key\"",
            "API_KEY = \"ctx7_api_key-abcdefghij\"",
            "API_KEY=ctx7_api_key=abcdefghijkl",
            "password: ctx7_api_key,",
            "API_KEY=\"ctx7_api_key\"-actual-secret",
            "API_KEY='ctx7_api_key'extra",
            "API_KEY=\"ctx7_api_key\",actual-secret",
            "API_KEY=ctx7_api_key;B_TOKEN=abcdefghijkl",
            "API_KEY=ctx7_api_key;X=ghp_111111111111111111111111111111",
            "API_KEY=\"ctx7_api_key\"#actual-secret",
            "API_KEY=ctx7_api_key;DEPLOY_TOKEN=abcdefghijkl1234",
            "API_KEY=ctx7_api_key;DEPLOY_TOKEN=\"abcdefghijkl1234\"",
            "password: ctx7_api_key;sample-tail",
            "password: ctx7_api_key extra words here",
            "API_KEY=ctx7_api_key;DEPLOY_TOKEN=abcdefghijkl1234x",
            "export \"API_KEY\"=\"ctx7_api_key\",sample-tail",
            "API_KEY=ctx7_api_key'additional'",
            "API_KEY=\"ctx7_api_key abcdefghijkl\"",
            "export API_KEY=ctx7_api_key B_TOKEN=abcdefghijkl",
        ] {
            assert!(suspicious(line, &allowed), "{line}");
        }
        // a token format inside an approved whole value is covered, one outside is not
        let bearer = vec!["Bearer ghp_000000000000000000000000000000".to_string()];
        assert!(!suspicious(
            "API_TOKEN=\"Bearer ghp_000000000000000000000000000000\"",
            &bearer
        ));
        assert!(suspicious(
            "API_TOKEN=\"Bearer ghp_000000000000000000000000000000\" X=ghp_111111111111111111111111111111",
            &bearer
        ));
        // a neighbouring benign assignment is not read as the approved key's value
        for line in [
            "export API_KEY=ctx7_api_key # note",
            "export API_KEY=ctx7_api_key EDITOR=vim",
            "export API_KEY=ctx7_api_key; echo ready",
            "API_KEY='ctx7_api_key'",
            "export API_KEY=ctx7_api_key;echo ready",
            "API_KEY=ctx7_api_key;EDITOR=vimvimvim",
            "password: ctx7_api_key # note",
            "API_KEY=\"ctx7_api_key\"; echo ready",
            "\"API_KEY\": \"ctx7_api_key\",",
            "API_KEY = \"ctx7_api_key\"#comment",
            "{ \"API_KEY\": \"ctx7_api_key\" }",
            "{\"API_KEY\":\"ctx7_api_key\"}",
        ] {
            assert!(!suspicious(line, &allowed), "{line}");
        }
        assert!(!suspicious("API_KEY=test;echo ready", &[]));
        // many commands on one line neither nest calls nor hide a later secret
        let long = "API_KEY=a;".repeat(100_000);
        assert!(!suspicious(&long, &[]));
        assert!(suspicious(
            &format!("{long}DEPLOY_TOKEN=abcdefghijkl1234"),
            &[]
        ));
        let both = vec!["samplevalue api_key=another-example".to_string()];
        assert!(!suspicious(
            "password: samplevalue api_key=another-example",
            &both
        ));
        // a plain scalar is approved as a whole, up to a comment
        let scalar = vec!["ctx7_api_key extra words here".to_string()];
        assert!(!suspicious(
            "password: ctx7_api_key extra words here",
            &scalar
        ));
        assert!(!suspicious(
            "password: ctx7_api_key extra words here # note",
            &scalar
        ));
        assert!(suspicious("password: ctx7_api_key extra words", &scalar));
    }

    #[test]
    fn an_allowed_value_unblocks_the_audit() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let saved = commit(&repo, None, b"KEY_X_TOKEN = \"placeholder-1\"\n");
        assert!(audit_unpublished(&repo, &saved, None, &[]).is_err());
        audit_unpublished(&repo, &saved, None, &["placeholder-1".to_string()]).unwrap();
    }
}
