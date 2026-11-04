//! Keeps the combined stdout and stderr of each task's most recent run on disk, so output
//! that a style hid or truncated (`replacing`, `timed`, `silent`, the prefix and keep-order
//! interleaving) can be read back with `mise tasks log` without running the task again.
//!
//! Lines are recorded after redaction, from the same observers the artifact cache uses, so a
//! log holds exactly what mise could have printed. Only output mise already reads is kept:
//! a `raw` or `interactive` task, or an `interleave` task whose streams mise hands straight to
//! the terminal, writes no log, and a stale log from an earlier captured run is removed.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::dirs;
use crate::file;
use crate::hash;
use crate::task::Task;

/// The most a log keeps. Older lines are dropped first, so a long run keeps its end.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

/// The output of one task run while it runs.
#[derive(Debug, Default)]
pub(crate) struct TaskRunLog {
    captured: AtomicBool,
    tail: Mutex<Tail>,
}

#[derive(Debug, Default)]
struct Tail {
    lines: VecDeque<String>,
    bytes: usize,
    dropped_lines: usize,
}

impl TaskRunLog {
    /// Records that mise reads at least one of this run's streams, so the log is complete.
    pub(crate) fn mark_captured(&self) {
        self.captured.store(true, Ordering::Relaxed);
    }

    pub(crate) fn push(&self, line: &str) {
        let mut tail = self.tail.lock().unwrap();
        tail.bytes += line.len() + 1;
        tail.lines.push_back(line.to_string());
        while tail.bytes > MAX_BYTES && tail.lines.len() > 1 {
            if let Some(old) = tail.lines.pop_front() {
                tail.bytes -= old.len() + 1;
                tail.dropped_lines += 1;
            }
        }
    }

    /// Replaces the task's log with this run's output, or removes the old log when this run
    /// was not captured. Returns the path when a log was written.
    pub(crate) fn finish(&self, task: &Task) -> Option<PathBuf> {
        let path = log_path(task);
        if !self.captured.load(Ordering::Relaxed) {
            if path.exists()
                && let Err(err) = file::remove_file(&path)
            {
                debug!("task {}: failed to remove stale log: {err}", task.name);
            }
            return None;
        }
        let tail = self.tail.lock().unwrap();
        let mut contents = String::with_capacity(tail.bytes + 80);
        if tail.dropped_lines > 0 {
            contents.push_str(&format!(
                "[mise: {} earlier lines dropped to keep the log under {}]\n",
                tail.dropped_lines,
                bytesize::ByteSize::b(MAX_BYTES as u64)
            ));
        }
        for line in &tail.lines {
            contents.push_str(line);
            contents.push('\n');
        }
        let written = path
            .parent()
            .map_or(Ok(()), file::create_dir_all)
            .and_then(|()| file::write_atomic(&path, contents));
        match written {
            Ok(()) => Some(path),
            Err(err) => {
                debug!("task {}: failed to write log: {err}", task.name);
                None
            }
        }
    }
}

/// Where the last run of `task` is logged: one file per task name per project.
pub fn log_path(task: &Task) -> PathBuf {
    let root = task
        .config_root
        .clone()
        .unwrap_or_else(|| task.config_source.clone());
    let id = hash::hash_blake3_to_str(&format!("{}\0{}", root.display(), task.name));
    dirs::STATE.join("task-logs").join(format!("{id}.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_newest_lines_under_the_cap() {
        let log = TaskRunLog::default();
        let line = "x".repeat(1024);
        for _ in 0..(MAX_BYTES / 1024 + 10) {
            log.push(&line);
        }
        log.push("last");
        let tail = log.tail.lock().unwrap();
        assert!(tail.bytes <= MAX_BYTES);
        assert!(tail.dropped_lines > 0);
        assert_eq!(tail.lines.back().map(String::as_str), Some("last"));
    }

    #[test]
    fn a_single_oversized_line_is_kept() {
        let log = TaskRunLog::default();
        log.push(&"y".repeat(MAX_BYTES + 1));
        assert_eq!(log.tail.lock().unwrap().lines.len(), 1);
    }
}
