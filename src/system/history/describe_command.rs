//! An optional command that names checkpoints (`history.describe_command`):
//! an agent, say. It receives one JSON object on stdin (the checkpoint's
//! uuid, trigger, and computed description, the changed paths that are not
//! private, and a unified diff of the changed files that are backed up, at
//! most 64 KiB) and prints one line, which becomes the description. The
//! checkpoint is durable before the command runs and stays as it is when
//! the command fails, prints nothing, or takes longer than 30 seconds; the
//! watcher runs it once per checkpoint it saved, never per event or retry.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use eyre::{Result, bail};
use serde::Serialize;

use super::checkpoint::{Store, annotate, describe_changes};
use super::shadow::DiffOpts;
use super::store::{self, Annotation, Changes, DescriptionSource, Entry};

/// How long the command may take, how much diff it is given, and how long a
/// description it may print.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);
const DIFF_LIMIT: usize = 64 * 1024;
const DESCRIPTION_LIMIT: usize = 200;

#[derive(Serialize)]
struct Input<'a> {
    uuid: &'a str,
    trigger: &'a str,
    /// The computed description, with private paths counted, not named.
    description: String,
    added: Vec<&'a str>,
    modified: Vec<&'a str>,
    removed: Vec<&'a str>,
    /// A unified diff of the changed files that are backed up (never a
    /// private file, never one with `backup = false`), cut at the limit.
    diff: String,
    diff_truncated: bool,
}

/// The configured command, if any.
pub(crate) fn configured() -> Option<String> {
    let command = crate::config::Settings::get()
        .history
        .describe_command
        .trim()
        .to_string();
    (!command.is_empty()).then_some(command)
}

/// Runs the command for `entry` and records what it printed. `Ok(None)`
/// when it printed nothing usable; an error when it could not run, timed
/// out, or failed. The checkpoint is unchanged in every case but success.
pub(crate) fn run(store: &Store, entry: &Entry, command: &str) -> Result<Option<String>> {
    let input = serde_json::to_vec(&input(store, entry)?)?;
    let mut child = shell(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    // stdin is written and closed on its own thread: a command that answers
    // before reading everything must not block us
    let mut stdin = child.stdin.take().expect("piped");
    std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("took longer than {}s", TIMEOUT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let output = reader.join().unwrap_or_default();
    if !status.success() {
        bail!("exited with {status}");
    }
    let Some(line) = first_line(&output) else {
        return Ok(None);
    };
    annotate(
        store,
        entry,
        Annotation {
            description: Some(line.clone()),
            description_source: Some(DescriptionSource::Command),
            pinned: None,
            labels: None,
            updated_at: store::now_rfc3339(),
        },
    )?;
    Ok(Some(line))
}

/// The first non-empty line, trimmed, cut at the limit.
fn first_line(output: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(output);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(DESCRIPTION_LIMIT).collect())
}

fn shell(command: &str) -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        cmd
    }
}

/// What the command is told: never a private path, never the contents of a
/// file that is not backed up.
fn input<'a>(store: &Store, entry: &'a Entry) -> Result<Input<'a>> {
    let checkpoint = &entry.checkpoint;
    let mut withheld: BTreeSet<&str> = BTreeSet::new();
    let mut unbacked: BTreeSet<&str> = BTreeSet::new();
    for coverage in &checkpoint.tree.coverage.entries {
        if coverage.private.is_some() || coverage.mode == "private" {
            withheld.insert(coverage.path.as_str());
        } else if !coverage.backup {
            unbacked.insert(coverage.path.as_str());
        }
    }
    let under = |path: &str, roots: &BTreeSet<&str>| {
        roots.iter().any(|root| {
            path == *root
                || path
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let visible = |paths: &'a [String]| -> Vec<&'a str> {
        paths
            .iter()
            .map(String::as_str)
            .filter(|path| !under(path, &withheld))
            .collect()
    };
    let added = visible(&checkpoint.changes.added);
    let modified = visible(&checkpoint.changes.modified);
    let removed = visible(&checkpoint.changes.removed);
    let hidden = checkpoint.changes.added.len()
        + checkpoint.changes.modified.len()
        + checkpoint.changes.removed.len()
        - added.len()
        - modified.len()
        - removed.len();
    let visible_changes = Changes {
        since: checkpoint.changes.since.clone(),
        added: added.iter().map(|path| path.to_string()).collect(),
        modified: modified.iter().map(|path| path.to_string()).collect(),
        removed: removed.iter().map(|path| path.to_string()).collect(),
        truncated: checkpoint.changes.truncated,
    };
    let mut description = if hidden == 0 {
        checkpoint.description.clone()
    } else {
        describe_changes(&visible_changes)
    };
    if hidden > 0 {
        if !description.is_empty() {
            description.push_str("; ");
        }
        description.push_str(&format!(
            "{hidden} private file{} changed",
            if hidden == 1 { "" } else { "s" }
        ));
    }
    let (diff, diff_truncated) = match (
        store.repo(),
        &checkpoint.tree.snapshot,
        checkpoint
            .changes
            .since
            .as_deref()
            .and_then(|since| {
                store::read_meta_cache_in(store.state_dir(), since)
                    .ok()
                    .flatten()
            })
            .and_then(|previous| previous.tree.snapshot),
    ) {
        (Some(repo), Some(snapshot), Some(previous)) => {
            let mut text = String::new();
            for path in added.iter().chain(&modified).chain(&removed) {
                if under(path, &unbacked) {
                    continue;
                }
                let tree_path = super::tracked::display_to_tree_path(path);
                let result = repo.diff(
                    &previous,
                    snapshot,
                    &DiffOpts {
                        patch: true,
                        stream: false,
                        color: false,
                        paths: Some((tree_path.clone(), tree_path)),
                    },
                )?;
                text.push_str(&String::from_utf8_lossy(&result.output));
                if text.len() > DIFF_LIMIT {
                    break;
                }
            }
            let truncated = text.len() > DIFF_LIMIT;
            if truncated {
                let mut cut = DIFF_LIMIT;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.truncate(cut);
            }
            (text, truncated)
        }
        _ => (String::new(), false),
    };
    Ok(Input {
        uuid: &checkpoint.uuid,
        trigger: checkpoint.trigger.as_str(),
        description,
        added,
        modified,
        removed,
        diff,
        diff_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_is_the_description() {
        assert_eq!(
            first_line(b"\n  tidy zsh aliases  \nmore\n").as_deref(),
            Some("tidy zsh aliases")
        );
        assert_eq!(first_line(b"   \n"), None);
        let long = "x".repeat(300);
        assert_eq!(
            first_line(long.as_bytes()).unwrap().len(),
            DESCRIPTION_LIMIT
        );
    }
}
