#![allow(unknown_lints)]

use std::sync::{Arc, Mutex};

use clx::progress::{ProgressJob, ProgressJobBuilder, ProgressStatus};
use std::sync::LazyLock as Lazy;

use crate::ui::style;
use crate::{backend, ui};

pub(crate) use mise_util::progress::{ProgressIcon, SingleReport};

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

/// clx-based progress report implementation
#[derive(Debug)]
pub(crate) struct ProgressReport {
    job: Arc<ProgressJob>,
    /// The phase and any secondary detail, folded into one `message` prop:
    /// this row has a single status cell.
    phase: Mutex<String>,
    detail: Mutex<String>,
}

impl ProgressReport {
    pub(crate) fn new(prefix: String) -> ProgressReport {
        Self::new_with_pad(prefix, *LONGEST_PLUGIN_NAME)
    }

    pub(crate) fn new_with_pad(prefix: String, pad: usize) -> ProgressReport {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let formatted_prefix = normal_prefix(pad, &prefix);

        // Template: prefix + message + optional bytes/progress bar + spinner on right
        // Use flex_fill to pad message and push progress bar to right edge
        // Use "arc" spinner style instead of default mini_dot
        // clx's bytes() function shows actual byte values for the current operation
        // while clx handles the multi-operation mapping internally for OSC progress
        // Use bytes(total=false, hide_complete=true) to show only current bytes and hide on completion
        let body = "{{ prefix }} {{ message | flex_fill }} {% if total %}{{ bytes(total=false, hide_complete=true) }} {{ eta(hide_complete=true) }} {{ progress_bar(width=20, hide_complete=true) }} {% endif %}{{ spinner(name=\"arc\") }}";

        let job = ProgressJobBuilder::new()
            .body(body)
            .prop("prefix", &formatted_prefix)
            .prop("message", "")
            .start();

        ProgressReport {
            job,
            phase: Mutex::new(String::new()),
            detail: Mutex::new(String::new()),
        }
    }

    fn render_message(&self) {
        let phase = self.phase.lock().unwrap();
        let detail = self.detail.lock().unwrap();
        let message = if detail.is_empty() {
            phase.clone()
        } else {
            format!("{phase}  {detail}")
        };
        self.job.prop("message", &message);
    }
}

impl SingleReport for ProgressReport {
    fn println(&self, message: String) {
        self.job.println(&message);
    }

    fn set_message(&self, message: String) {
        *self.phase.lock().unwrap() = message.replace('\r', "");
        self.render_message();
    }

    fn set_detail(&self, detail: String) {
        *self.detail.lock().unwrap() = detail.replace('\r', "");
        self.render_message();
    }

    fn inc(&self, delta: u64) {
        self.job.increment(delta as usize);
    }

    fn set_position(&self, pos: u64) {
        self.job.progress_current(pos as usize);
    }

    fn set_length(&self, length: u64) {
        self.job.progress_total(length as usize);
    }

    fn abandon(&self) {
        self.job.set_status(ProgressStatus::Hide);
    }

    fn finish_with_icon(&self, _message: String, icon: ProgressIcon) {
        // Set status based on icon
        match icon {
            ProgressIcon::Success => self.job.set_status(ProgressStatus::Done),
            ProgressIcon::Error => self.job.set_status(ProgressStatus::Failed),
            ProgressIcon::Warning => self.job.set_status(ProgressStatus::Warn),
            ProgressIcon::Skipped => self.job.set_status(ProgressStatus::Done),
        }
    }

    fn start_operations(&self, count: usize) {
        self.job.start_operations(count);
    }

    fn next_operation(&self) {
        self.job.next_operation();
    }
}

#[derive(Debug)]
pub(crate) struct QuietReport {}

impl QuietReport {
    pub(crate) fn new() -> QuietReport {
        QuietReport {}
    }
}

impl SingleReport for QuietReport {}

#[derive(Debug)]
pub(crate) struct VerboseReport {
    prefix: String,
    prev_message: Mutex<String>,
    prev_detail: Mutex<String>,
    pad: usize,
    total_operations: Mutex<Option<usize>>,
    current_operation: Mutex<usize>,
}

impl VerboseReport {
    pub(crate) fn new(prefix: String) -> VerboseReport {
        Self::new_with_pad(prefix, *LONGEST_PLUGIN_NAME)
    }

    pub(crate) fn new_with_pad(prefix: String, pad: usize) -> VerboseReport {
        VerboseReport {
            prefix,
            prev_message: Mutex::new("".to_string()),
            prev_detail: Mutex::new("".to_string()),
            pad,
            total_operations: Mutex::new(None),
            current_operation: Mutex::new(0),
        }
    }
}

impl SingleReport for VerboseReport {
    fn println(&self, message: String) {
        safe_eprintln!("{message}");
    }
    fn set_process_output(&self, message: String) {
        // Not set_message: its dedup collapses repeated phase text, which would
        // silently drop a child's repeated stdout lines now that
        // `shows_process_output` suppresses the failure replay.
        let prefix = pad_prefix(self.pad, &self.prefix);
        log::info!("{prefix} {message}");
    }
    fn set_detail(&self, detail: String) {
        // One line per change, folded with the phase, the way this reporter
        // printed the combined message before phase and detail were split.
        if detail.trim().is_empty() {
            return;
        }
        let phase = self.prev_message.lock().unwrap().clone();
        let mut prev = self.prev_detail.lock().unwrap();
        if *prev == detail {
            return;
        }
        let prefix = pad_prefix(self.pad, &self.prefix);
        log::info!("{prefix} {phase} {detail}");
        *prev = detail;
    }
    fn shows_process_output(&self) -> bool {
        true
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
        if let Some(total) = *self.total_operations.lock().unwrap() {
            let mut current = self.current_operation.lock().unwrap();
            // A backend may step past its last declared operation to say "all
            // declared work is done"; the counter stays at the last one.
            *current = (*current + 1).min(total);
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
