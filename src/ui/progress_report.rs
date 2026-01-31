#![allow(unknown_lints)]
#![allow(clippy::literal_string_with_formatting_args)]

use std::{
    fmt::{Display, Formatter},
    sync::{Arc, Mutex},
};

use clx::progress::{ProgressJob, ProgressJobBuilder, ProgressStatus};
use std::sync::LazyLock as Lazy;

use crate::ui::style;
use crate::{backend, ui};

#[derive(Debug, Clone, Copy)]
pub enum ProgressIcon {
    Success,
    Skipped,
    Warning,
    Error,
}

impl Display for ProgressIcon {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgressIcon::Success => write!(f, "{}", style::egreen("✓").bright()),
            ProgressIcon::Skipped => write!(f, "{}", style::eyellow("⇢").bright()),
            ProgressIcon::Warning => write!(f, "{}", style::eyellow("⚠").bright()),
            ProgressIcon::Error => write!(f, "{}", style::ered("✗").bright()),
        }
    }
}

pub trait SingleReport: Send + Sync + std::fmt::Debug {
    fn println(&self, _message: String) {}
    fn set_message(&self, _message: String) {}
    fn inc(&self, _delta: u64) {}
    fn set_position(&self, _delta: u64) {}
    fn set_length(&self, _length: u64) {}
    fn abandon(&self) {}
    fn finish(&self) {
        self.finish_with_message(String::new());
    }
    fn finish_with_message(&self, message: String) {
        self.finish_with_icon(message, ProgressIcon::Success);
    }
    fn finish_with_icon(&self, _message: String, _icon: ProgressIcon) {}

    /// Declare how many operations this progress report will have
    /// Each operation will get equal space (1/count)
    /// For example, if there are 3 operations (download, checksum, extract):
    /// - start_operations(3) at the beginning
    ///
    /// Then each set_length() call will allocate 33.33% of the total progress
    fn start_operations(&self, _count: usize) {}

    /// Advance to the next operation
    /// Call this before each new stage (after the first one)
    fn next_operation(&self) {}
}

static LONGEST_PLUGIN_NAME: Lazy<usize> = Lazy::new(|| {
    backend::list()
        .into_iter()
        .map(|p| p.id().len())
        .max()
        .unwrap_or_default()
        .clamp(15, 35)
});

fn pad_prefix(w: usize, s: &str) -> String {
    console::pad_str(s, w, console::Alignment::Left, None).to_string()
}

fn normal_prefix(pad: usize, prefix: &str) -> String {
    pad_prefix(pad, prefix)
}

/// Progress state for tracking multi-operation progress
#[derive(Debug)]
struct ProgressState {
    total_operations: Option<usize>,
    current_operation: usize,
    operation_count: u32,
    operation_base: u64,
    operation_length: u64,
    position: u64,
    length: Option<u64>,
}

impl Default for ProgressState {
    fn default() -> Self {
        Self {
            total_operations: None,
            current_operation: 0,
            operation_count: 0,
            operation_base: 0,
            operation_length: 1_000_000,
            position: 0,
            length: None,
        }
    }
}

/// clx-based progress report implementation
#[derive(Debug)]
pub struct ProgressReport {
    job: Arc<ProgressJob>,
    state: Mutex<ProgressState>,
}

impl ProgressReport {
    pub fn new(prefix: String) -> ProgressReport {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let pad = *LONGEST_PLUGIN_NAME;
        let formatted_prefix = normal_prefix(pad, &prefix);

        // Template: prefix + message + optional progress bar + spinner on right
        // Use flex_fill to pad message and push progress bar to right edge
        // Use "dot" spinner style instead of default mini_dot
        let body = "{{ prefix }} {{ message | flex_fill }} {% if total %}{{ eta(hide_complete=true) }} {{ progress_bar(width=20, hide_complete=true) }} {% endif %}{{ spinner(name=\"arc\") }}";

        let job = ProgressJobBuilder::new()
            .body(body)
            .prop("prefix", &formatted_prefix)
            .prop("message", "")
            .start();

        ProgressReport {
            job,
            state: Mutex::new(ProgressState::default()),
        }
    }

    fn update_terminal_progress(&self) {
        let state = self.state.lock().unwrap();

        // If no length set, we're spinning - report minimal progress
        if state.length.is_none() {
            return;
        }

        let pb_len = state.length.unwrap();
        let pb_progress = if pb_len > 0 {
            (state.position as f64 / pb_len as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Map to allocated range
        let mapped_position =
            state.operation_base + (pb_progress * state.operation_length as f64) as u64;

        // Update clx progress job
        self.job.progress_current(mapped_position as usize);
        self.job.progress_total(1_000_000);
    }
}

impl SingleReport for ProgressReport {
    fn println(&self, message: String) {
        self.job.println(&message);
    }

    fn set_message(&self, message: String) {
        let state = self.state.lock().unwrap();
        let formatted = if let Some(total) = state.total_operations {
            format!("[{}/{}] {}", state.current_operation, total, message)
        } else {
            message
        };
        drop(state);
        self.job.prop("message", &formatted.replace('\r', ""));
    }

    fn inc(&self, delta: u64) {
        {
            let mut state = self.state.lock().unwrap();
            state.position += delta;
        }
        self.update_terminal_progress();

        // Check if we've completed the current operation
        let state = self.state.lock().unwrap();
        if state.length.is_some() && state.position >= state.length.unwrap() {
            drop(state);
            // Reset to spinning state
            self.job.progress_current(0);
            self.job.progress_total(0);
        }
    }

    fn set_position(&self, pos: u64) {
        {
            let mut state = self.state.lock().unwrap();
            state.position = pos;
        }
        self.update_terminal_progress();

        // Check if we've completed the current operation
        let state = self.state.lock().unwrap();
        if state.length.is_some() && state.position >= state.length.unwrap() {
            drop(state);
            // Reset to spinning state
            self.job.progress_current(0);
            self.job.progress_total(0);
        }
    }

    fn set_length(&self, length: u64) {
        let mut state = self.state.lock().unwrap();

        // Increment operation count
        state.operation_count += 1;
        let count = state.operation_count;

        // When starting a new operation (count > 1), complete the previous operation first
        if count > 1 {
            let completed_position = state.operation_base + state.operation_length;
            state.operation_base = completed_position;

            // Report completion of previous operation
            self.job.progress_current(completed_position as usize);
            self.job.progress_total(1_000_000);
        }

        // Calculate allocation for this operation
        let total = state.total_operations.unwrap_or(1).max(1);
        let per_operation = 1_000_000 / total as u64;
        state.operation_length = per_operation;

        // Reset position for new operation
        state.position = 0;
        state.length = Some(length);

        drop(state);
        self.update_terminal_progress();
    }

    fn abandon(&self) {
        self.job.set_status(ProgressStatus::Hide);
    }

    fn finish_with_icon(&self, _message: String, icon: ProgressIcon) {
        // Mark this report as complete (100%)
        // Set total first, then current, because progress_current clamps to total
        self.job.progress_total(1_000_000);
        self.job.progress_current(1_000_000);

        // Set status based on icon
        match icon {
            ProgressIcon::Success => self.job.set_status(ProgressStatus::Done),
            ProgressIcon::Error => self.job.set_status(ProgressStatus::Failed),
            ProgressIcon::Warning => self.job.set_status(ProgressStatus::Warn),
            ProgressIcon::Skipped => self.job.set_status(ProgressStatus::Done),
        }
    }

    fn start_operations(&self, count: usize) {
        let mut state = self.state.lock().unwrap();
        state.total_operations = Some(count.max(1));
        state.current_operation = 1;
    }

    fn next_operation(&self) {
        let mut state = self.state.lock().unwrap();
        if state.total_operations.is_some() {
            state.current_operation += 1;
        }
    }
}

#[derive(Debug)]
pub struct QuietReport {}

impl QuietReport {
    pub fn new() -> QuietReport {
        QuietReport {}
    }
}

impl SingleReport for QuietReport {}

#[derive(Debug)]
pub struct VerboseReport {
    prefix: String,
    prev_message: Mutex<String>,
    pad: usize,
    total_operations: Mutex<Option<usize>>,
    current_operation: Mutex<usize>,
}

impl VerboseReport {
    pub fn new(prefix: String) -> VerboseReport {
        VerboseReport {
            prefix,
            prev_message: Mutex::new("".to_string()),
            pad: *LONGEST_PLUGIN_NAME,
            total_operations: Mutex::new(None),
            current_operation: Mutex::new(0),
        }
    }
}

impl SingleReport for VerboseReport {
    fn println(&self, message: String) {
        eprintln!("{message}");
    }
    fn set_message(&self, message: String) {
        let mut prev_message = self.prev_message.lock().unwrap();
        if *prev_message == message {
            return;
        }
        let total = *self.total_operations.lock().unwrap();
        let current = *self.current_operation.lock().unwrap();
        let formatted = if let Some(total) = total {
            format!("[{}/{}] {}", current, total, message)
        } else {
            message.clone()
        };
        let prefix = pad_prefix(self.pad, &self.prefix);
        log::info!("{prefix} {formatted}");
        *prev_message = message;
    }
    fn finish(&self) {
        self.finish_with_message(style::egreen("done").to_string());
    }
    fn finish_with_icon(&self, message: String, icon: ProgressIcon) {
        let prefix = pad_prefix(self.pad - 2, &self.prefix);
        log::info!("{prefix} {icon} {message}");
    }
    fn start_operations(&self, count: usize) {
        *self.total_operations.lock().unwrap() = Some(count.max(1));
        *self.current_operation.lock().unwrap() = 1;
    }
    fn next_operation(&self) {
        let total = *self.total_operations.lock().unwrap();
        if total.is_some() {
            let mut current = self.current_operation.lock().unwrap();
            *current += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;

    #[tokio::test]
    async fn test_progress_report() {
        let _config = Config::get().await.unwrap();
        let pr = ProgressReport::new("foo".into());
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }

    #[tokio::test]
    async fn test_progress_report_verbose() {
        let _config = Config::get().await.unwrap();
        let pr = VerboseReport::new("PREFIX".to_string());
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }

    #[tokio::test]
    async fn test_progress_report_quiet() {
        let _config = Config::get().await.unwrap();
        let pr = QuietReport::new();
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }
}
