//! An optional command that names checkpoints (`history.describe_command`):
//! an agent, say. It receives one JSON object on stdin (the checkpoint's
//! uuid, trigger, and computed description, the changed tracked paths,
//! and a unified diff of the changed unencrypted tracked files, at
//! most 64 KiB) and prints one line, which becomes the description. The
//! checkpoint is durable before the command runs and stays as it is when
//! the command fails, prints nothing, or takes longer than 30 seconds; the
//! watcher runs it once per checkpoint it saved, never per event or retry.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use eyre::{Result, bail};
use serde::Serialize;

use super::checkpoint::{Store, annotate};
use super::shadow::DiffOpts;
use super::store::{self, Annotation, DescriptionSource, Entry};

#[cfg(windows)]
mod windows_job;

/// How long the command may take, how much diff it is given, and how long a
/// description it may print.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);
/// How long the output is waited for after the shell exited.
const OUTPUT_GRACE: Duration = Duration::from_secs(2);
const DIFF_LIMIT: usize = 64 * 1024;
const DESCRIPTION_LIMIT: usize = 200;

#[derive(Serialize)]
struct Input<'a> {
    uuid: &'a str,
    trigger: &'a str,
    /// The computed description of explicitly tracked changes.
    description: String,
    added: Vec<&'a str>,
    modified: Vec<&'a str>,
    removed: Vec<&'a str>,
    /// A unified diff of changed unencrypted tracked files, cut at the limit.
    diff: String,
    diff_truncated: bool,
}

/// The command running right now, so a shutdown can end it.
static RUNNING: Mutex<Option<RunningCommand>> = Mutex::new(None);

#[derive(Clone)]
struct RunningCommand {
    pid: u32,
    #[cfg(windows)]
    job: std::sync::Arc<windows_job::Job>,
}

impl RunningCommand {
    fn kill(&self) {
        #[cfg(unix)]
        {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(self.pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        #[cfg(windows)]
        self.job.kill();
    }
}

struct ActiveCommand(RunningCommand);

impl Drop for ActiveCommand {
    fn drop(&mut self) {
        self.0.kill();
        if let Ok(mut running) = RUNNING.lock()
            && running
                .as_ref()
                .is_some_and(|running| running.pid == self.0.pid)
        {
            *running = None;
        }
    }
}

/// Ends the running command and everything it started, if any.
pub(crate) fn abort_running() {
    let running = RUNNING.lock().ok().and_then(|mut running| running.take());
    if let Some(running) = running {
        running.kill();
    }
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
    run_with_limits(store, entry, command, TIMEOUT, OUTPUT_GRACE)
}

fn run_with_limits(
    store: &Store,
    entry: &Entry,
    command: &str,
    timeout: Duration,
    output_grace: Duration,
) -> Result<Option<String>> {
    let input = serde_json::to_vec(&input(store, entry)?)?;
    let mut shell = shell(command);
    shell
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // its own process group: a timeout ends what the shell started too
        shell.process_group(0);
    }
    #[cfg(windows)]
    let (mut child, job) = windows_job::spawn(&mut shell)?;
    #[cfg(not(windows))]
    let mut child = shell.spawn()?;
    let active = ActiveCommand(RunningCommand {
        pid: child.id(),
        #[cfg(windows)]
        job: std::sync::Arc::new(job),
    });
    if let Ok(mut running) = RUNNING.lock() {
        *running = Some(active.0.clone());
    }
    // stdin is written and closed on its own thread: a command that answers
    // before reading everything must not block us
    let mut stdin = child.stdin.take().expect("piped");
    std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let stdout = child.stdout.take().expect("piped");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.take(DIFF_LIMIT as u64 + 1).read_to_end(&mut out);
        let _ = sender.send(out);
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            // the shell and whatever it started; the reader thread ends
            // with the last writer of the pipe, so it is not waited for
            active.0.kill();
            let _ = child.kill();
            let _ = child.wait();
            bail!("took longer than {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    // a descendant that outlived the shell and kept the pipe is not the
    // shell's answer: the output is waited for a moment, not forever
    let Ok(output) = receiver.recv_timeout(output_grace) else {
        active.0.kill();
        bail!("a process it started kept its output open");
    };
    if output.len() > DIFF_LIMIT {
        bail!("description output exceeded {} bytes", DIFF_LIMIT);
    }
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

/// What the command is told: never an excluded path or encrypted contents.
fn input<'a>(store: &Store, entry: &'a Entry) -> Result<Input<'a>> {
    let checkpoint = &entry.checkpoint;
    let encrypted: BTreeSet<&str> = checkpoint
        .tree
        .coverage
        .entries
        .iter()
        .filter(|entry| entry.encrypt)
        .map(|entry| entry.path.as_str())
        .collect();
    let under = |path: &str, roots: &BTreeSet<&str>| {
        roots.iter().any(|root| {
            path == *root
                || path
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let visible =
        |paths: &'a [String]| -> Vec<&'a str> { paths.iter().map(String::as_str).collect() };
    let added = visible(&checkpoint.changes.added);
    let modified = visible(&checkpoint.changes.modified);
    let removed = visible(&checkpoint.changes.removed);
    let description = checkpoint.description.clone();
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
                if under(path, &encrypted) {
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
    fn command_lifecycle_keeps_history() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open_in(temp.path())?;
        let outcome = store.attempt(
            &super::super::tracked::TrackedSet::default(),
            super::super::checkpoint::Draft::new(store::Trigger::Agent),
        )?;
        let super::super::checkpoint::Outcome::Created(entry) = outcome else {
            bail!("test requires an ordinary Git checkpoint");
        };
        assert_eq!(
            run(&store, &entry, "echo named checkpoint")?.as_deref(),
            Some("named checkpoint")
        );
        #[cfg(windows)]
        {
            let started = Instant::now();
            let error = run_with_limits(
                &store,
                &entry,
                "ping -n 30 127.0.0.1",
                Duration::from_secs(1),
                OUTPUT_GRACE,
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("took longer than 1s"),
                "{error:#}"
            );
            assert!(started.elapsed() < Duration::from_secs(10));
        }
        assert!(RUNNING.lock().unwrap().is_none());
        #[cfg(unix)]
        {
            let error = run(&store, &entry, "yes oversized-output").unwrap_err();
            assert!(error.to_string().contains("output exceeded"), "{error:#}");
        }
        assert_eq!(store::resolve_ref("latest", &store.list()?)?, entry.id);
        Ok(())
    }

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
