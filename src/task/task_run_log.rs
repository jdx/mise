//! Keeps the combined stdout and stderr of each task's most recent run on disk, so output
//! that a style hid or truncated (`replacing`, `timed`, `silent`, the prefix and keep-order
//! interleaving) can be read back with `mise tasks log` without running the task again.
//!
//! Lines are recorded after redaction, from the same observers the artifact cache uses, so a
//! log holds exactly what mise could have printed. Only output mise already reads is kept:
//! a `raw` or `interactive` task, or an `interleave` task whose streams mise hands straight to
//! the terminal, writes no log, and a stale log from an earlier captured run is removed.
//!
//! Task output can hold private data that was never marked for redaction, so the directory is
//! owner-only and every log is created `0600`.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

use eyre::Result;

use crate::config::{self, Config};
use crate::dirs;
use crate::env;
use crate::file;
use crate::hash;
use crate::task::Task;

/// The most a log file holds, the notice about dropped output included. Older output is
/// dropped first, so a long run keeps its end.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

/// Room kept for the first-line notice that says how much output was dropped.
const NOTICE_BYTES: usize = 128;

const LINE_BUDGET: usize = MAX_BYTES - NOTICE_BYTES;

/// No command of the run was spawned, so the run says nothing about the task's output.
const NOT_SPAWNED: u8 = 0;
/// A command was spawned with its output handed to the terminal.
const UNCAPTURED: u8 = 1;
/// A command was spawned with its output read by mise.
const CAPTURED: u8 = 2;

/// The output of one task run while it runs.
#[derive(Debug, Default)]
pub(crate) struct TaskRunLog {
    state: AtomicU8,
    tail: Mutex<Tail>,
}

#[derive(Debug, Default)]
struct Tail {
    lines: VecDeque<String>,
    bytes: usize,
    dropped_bytes: usize,
}

impl TaskRunLog {
    /// Records that a command of this run is about to be spawned, and whether mise reads its
    /// output. A run that fails before any spawn (a template, the sandbox, an interruption)
    /// leaves the previous log alone.
    pub(crate) fn mark_spawned(&self, captured: bool) {
        self.state.fetch_max(
            if captured { CAPTURED } else { UNCAPTURED },
            Ordering::Relaxed,
        );
    }

    pub(crate) fn push(&self, line: &str) {
        let mut tail = self.tail.lock().unwrap();
        let line = if line.len() + 1 > LINE_BUDGET {
            // Keep the end of an oversized line, cut on a character boundary.
            let mut start = line.len() + 1 - LINE_BUDGET;
            while !line.is_char_boundary(start) {
                start += 1;
            }
            tail.dropped_bytes += start;
            &line[start..]
        } else {
            line
        };
        tail.bytes += line.len() + 1;
        tail.lines.push_back(line.to_string());
        while tail.bytes > LINE_BUDGET {
            let Some(old) = tail.lines.pop_front() else {
                break;
            };
            tail.bytes -= old.len() + 1;
            tail.dropped_bytes += old.len() + 1;
        }
    }

    /// Replaces the task's log with this run's output, or removes the old log when this run
    /// handed its output to the terminal. Returns the path when a log was written.
    pub(crate) fn finish(&self, task: &Task, config: &Config) -> Option<PathBuf> {
        let path = log_path(task, config);
        match self.state.load(Ordering::Relaxed) {
            NOT_SPAWNED => None,
            UNCAPTURED => {
                if path.exists()
                    && let Err(err) = file::remove_file(&path)
                {
                    debug!("task {}: failed to remove stale log: {err}", task.name);
                }
                None
            }
            _ => match self.write(&path) {
                Ok(()) => Some(path),
                Err(err) => {
                    debug!("task {}: failed to write log: {err}", task.name);
                    None
                }
            },
        }
    }

    fn write(&self, path: &Path) -> Result<()> {
        let dir = path.parent().expect("a log path has a parent");
        create_private_dir(dir)?;
        // `tempfile` creates its files 0600, and the rename keeps that mode.
        let temporary = tempfile::Builder::new().prefix(".tmp-").tempfile_in(dir)?;
        {
            let tail = self.tail.lock().unwrap();
            let mut out = std::io::BufWriter::new(temporary.as_file());
            out.write_all(notice(tail.dropped_bytes).as_bytes())?;
            for line in &tail.lines {
                out.write_all(line.as_bytes())?;
                out.write_all(b"\n")?;
            }
            out.flush()?;
        }
        file::persist_atomic(temporary, path)
    }
}

fn notice(dropped_bytes: usize) -> String {
    if dropped_bytes == 0 {
        return String::new();
    }
    format!(
        "[mise: {} of earlier output dropped to keep the log under {}]\n",
        bytesize::ByteSize::b(dropped_bytes as u64),
        bytesize::ByteSize::b(MAX_BYTES as u64)
    )
}

fn create_private_dir(dir: &Path) -> Result<()> {
    file::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Where the last run of `task` is logged: one file per task name per project. A global task
/// is the same task in every project, so its key also holds the project it runs in.
pub fn log_path(task: &Task, config: &Config) -> PathBuf {
    let root = task
        .config_root
        .clone()
        .unwrap_or_else(|| task.config_source.clone());
    let project = is_global_task(task).then(|| {
        config
            .project_root
            .clone()
            .or_else(|| dirs::CWD.clone())
            .unwrap_or_default()
    });
    dirs::STATE.join("task-logs").join(format!(
        "{}.log",
        log_key(&root, project.as_deref(), &task.name)
    ))
}

fn is_global_task(task: &Task) -> bool {
    config::is_global_config(&task.config_source)
        || config::is_global_task_include_path(&task.config_source)
        || task.config_root.as_deref() == Some(env::MISE_GLOBAL_CONFIG_ROOT.as_path())
}

fn log_key(root: &Path, project: Option<&Path>, name: &str) -> String {
    let key = match project {
        Some(project) => format!("{}\0{}\0{name}", root.display(), project.display()),
        None => format!("{}\0{name}", root.display()),
    };
    hash::hash_blake3_to_str(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(lines: &[&str]) -> TaskRunLog {
        let log = TaskRunLog::default();
        log.mark_spawned(true);
        for line in lines {
            log.push(line);
        }
        log
    }

    #[test]
    fn tail_keeps_the_newest_lines_under_the_cap() {
        let log = TaskRunLog::default();
        let line = "x".repeat(1024);
        for _ in 0..(MAX_BYTES / 1024 + 10) {
            log.push(&line);
        }
        log.push("last");
        let tail = log.tail.lock().unwrap();
        assert!(tail.bytes <= LINE_BUDGET);
        assert!(tail.dropped_bytes > 0);
        assert_eq!(tail.lines.back().map(String::as_str), Some("last"));
    }

    #[test]
    fn an_oversized_line_keeps_a_utf8_suffix_under_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("task.log");
        // a multi-byte character makes the cut land inside one unless it moves to a boundary
        let line = format!("start{}end", "é".repeat(MAX_BYTES));
        let log = captured(&[&line]);
        log.write(&path).unwrap();
        let written = std::fs::read(&path).unwrap();
        assert!(written.len() <= MAX_BYTES, "{} bytes", written.len());
        let written = String::from_utf8(written).expect("valid UTF-8");
        assert!(written.starts_with("[mise: "));
        assert!(written.ends_with("éend\n"));
        assert!(!written.contains("start"));
    }

    #[test]
    fn a_run_that_never_spawned_keeps_the_previous_log() {
        let log = TaskRunLog::default();
        assert_eq!(log.state.load(Ordering::Relaxed), NOT_SPAWNED);
        log.mark_spawned(true);
        log.mark_spawned(false);
        assert_eq!(log.state.load(Ordering::Relaxed), CAPTURED);
    }

    #[cfg(unix)]
    #[test]
    fn logs_are_private_to_their_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("task-logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = logs.join("task.log");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        captured(&["secret-ish output"]).write(&path).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&logs), 0o700);
        assert_eq!(mode(&path), 0o600);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "secret-ish output\n"
        );
    }

    #[test]
    fn a_global_task_has_one_log_per_project() {
        let root = Path::new("/home/me");
        let a = log_key(root, Some(Path::new("/src/a")), "fmt");
        let b = log_key(root, Some(Path::new("/src/b")), "fmt");
        assert_ne!(a, b);
        assert_eq!(a, log_key(root, Some(Path::new("/src/a")), "fmt"));
        assert_ne!(a, log_key(root, None, "fmt"));
    }
}
