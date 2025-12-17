#![allow(unknown_lints)]
#![allow(clippy::literal_string_with_formatting_args)]

use std::time::Duration;
use std::{
    fmt::{Display, Formatter},
    sync::Mutex,
};

use indicatif::{ProgressBar, ProgressStyle};
use std::sync::LazyLock as Lazy;

use crate::ui::style;
use crate::{backend, env, ui};

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
}

static SPIN_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = "{prefix} {wide_msg} {spinner:.blue} {elapsed:>3.dim.italic}";
    ProgressStyle::with_template(tmpl).unwrap()
});

const TICK_INTERVAL: Duration = Duration::from_millis(250);

static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = match *env::TERM_WIDTH {
        0..=89 => "{prefix} {wide_msg} {bar:10.cyan/blue} {percent:>2}%",
        90..=99 => "{prefix} {wide_msg} {bar:15.cyan/blue} {percent:>2}%",
        100..=114 => "{prefix} {wide_msg} {bytes}/{total_bytes:10} {bar:10.cyan/blue}",
        _ => {
            "{prefix} {wide_msg} {bytes}/{total_bytes} ({eta}) {bar:20.cyan/blue} {elapsed:>3.dim.italic}"
        }
    };
    ProgressStyle::with_template(tmpl).unwrap()
});

/// Renders a progress bar with text overlaid on top
/// The text background alternates based on progress:
/// - Filled portion: black text on cyan background
/// - Unfilled portion: dim text on default background
fn render_progress_bar_with_overlay(text: &str, progress: f64, width: usize) -> String {
    use console::Style;

    let progress = progress.clamp(0.0, 1.0);
    let filled_width = (width as f64 * progress) as usize;

    // Strip any existing ANSI codes from text
    let clean_text = console::strip_ansi_codes(text);

    // If text is longer than width, truncate it
    let display_text = if clean_text.chars().count() > width {
        clean_text.chars().take(width - 3).collect::<String>() + "..."
    } else {
        clean_text.to_string()
    };

    let text_len = display_text.chars().count();
    let padding = (width.saturating_sub(text_len)) / 2;

    // Build the bar with text overlay
    let mut result = String::new();

    // Styles for different regions
    let filled_bar_style = Style::new().cyan();
    let filled_text_style = Style::new().black().on_cyan();
    let empty_text_style = Style::new().dim();

    for i in 0..width {
        if i < padding || i >= padding + text_len {
            // No text here, just show the bar
            if i < filled_width {
                result.push_str(&filled_bar_style.apply_to('█').to_string());
            } else {
                result.push('░');
            }
        } else {
            // Text overlay
            let text_idx = i - padding;
            let ch = display_text.chars().nth(text_idx).unwrap();

            if i < filled_width {
                // Filled portion: black text on cyan background
                result.push_str(&filled_text_style.apply_to(ch).to_string());
            } else {
                // Unfilled portion: dim text
                result.push_str(&empty_text_style.apply_to(ch).to_string());
            }
        }
    }

    result
}

static FOOTER_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    // Simple template - we'll update the message with our custom rendered bar
    ProgressStyle::with_template("{wide_msg}").unwrap()
});

#[derive(Debug)]
pub struct ProgressReport {
    pub pb: ProgressBar,
    report_id: Option<usize>,
    total_operations: Mutex<Option<usize>>, // Total operations declared upfront (None if unknown)
    operation_count: Mutex<u32>,            // How many operations have started (1, 2, 3...)
    operation_base: Mutex<u64>, // Base progress for current operation (0, 333333, 666666...)
    operation_length: Mutex<u64>, // Allocated length for current operation
    footer_text: Option<String>, // If set, this is a footer bar with text overlay
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
    let prefix = format!("{} {prefix}", style::edim("mise"));
    pad_prefix(pad, &prefix)
}

impl ProgressReport {
    pub fn new(prefix: String) -> ProgressReport {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let pad = *LONGEST_PLUGIN_NAME;
        let pb = ProgressBar::new(100)
            .with_style(SPIN_TEMPLATE.clone())
            .with_prefix(normal_prefix(pad, &prefix));
        pb.enable_steady_tick(TICK_INTERVAL);

        // Allocate a report ID for multi-progress tracking
        let report_id = ui::multi_progress_report::MultiProgressReport::try_get()
            .map(|mpr| mpr.allocate_report_id());

        ProgressReport {
            pb,
            report_id,
            total_operations: Mutex::new(Some(1)), // Default to 1 operation (100% of progress)
            operation_count: Mutex::new(0),
            operation_base: Mutex::new(0),
            operation_length: Mutex::new(1_000_000), // Full range initially
            footer_text: None,
        }
    }

    pub fn new_footer(footer_text: String, length: u64, _message: String) -> ProgressReport {
        ui::ctrlc::show_cursor_after_ctrl_c();
        // Footer shows text inside the progress bar with custom overlay rendering
        let pb = ProgressBar::new(length).with_style(FOOTER_TEMPLATE.clone());
        // Don't enable steady tick for footer - it doesn't use a spinner template
        // and the tick causes unnecessary redraws

        // Don't set initial message here - it will be set after adding to MultiProgress
        // to prevent ghost output before the bar is part of the managed display

        ProgressReport {
            pb,
            report_id: None,
            total_operations: Mutex::new(None),
            operation_count: Mutex::new(0),
            operation_base: Mutex::new(0),
            operation_length: Mutex::new(length),
            footer_text: Some(footer_text),
        }
    }

    fn update_footer_display(&self) {
        // Update footer bar with custom text overlay rendering
        if let Some(footer_text) = &self.footer_text {
            let pos = self.pb.position();
            let len = self.pb.length().unwrap_or(1);
            let progress = if len > 0 {
                pos as f64 / len as f64
            } else {
                0.0
            };
            let width = *env::TERM_WIDTH;
            let rendered = render_progress_bar_with_overlay(footer_text, progress, width);
            self.pb.set_message(rendered);
        }
    }

    fn update_terminal_progress(&self) {
        // Map progress bar position to allocated range to prevent backwards progress
        if let Some(report_id) = self.report_id
            && let Some(mpr) = ui::multi_progress_report::MultiProgressReport::try_get()
        {
            // Check if we're spinning (no length set yet)
            if self.pb.length().is_none() {
                // During spinning, report minimal progress to show activity
                progress_trace!(
                    "update_terminal_progress[{}]: spinning, reporting 1%",
                    report_id
                );
                mpr.update_report_progress(report_id, 10_000, 1_000_000); // 1%
                return;
            }

            let base = *self.operation_base.lock().unwrap();
            let allocated_length = *self.operation_length.lock().unwrap();

            // Get progress bar state (position/length in bytes)
            let pb_pos = self.pb.position();
            let pb_len = self.pb.length().unwrap(); // Safe because we checked above

            // Calculate progress as 0.0-1.0
            let pb_progress = if pb_len > 0 {
                (pb_pos as f64 / pb_len as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };

            // Map to allocated range (base to base+allocated_length)
            let mapped_position = base + (pb_progress * allocated_length as f64) as u64;

            progress_trace!(
                "update_terminal_progress[{}]: pb=({}/{}) {:.1}%, base={}, alloc={}, mapped={}",
                report_id,
                pb_pos,
                pb_len,
                pb_progress * 100.0,
                base,
                allocated_length,
                mapped_position
            );

            // Always report against fixed 1,000,000 scale
            mpr.update_report_progress(report_id, mapped_position, 1_000_000);
        }
    }
}

impl SingleReport for ProgressReport {
    fn println(&self, message: String) {
        // Suspend the entire MultiProgress to prevent footer duplication
        crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
            eprintln!("{message}");
        });
    }
    fn set_message(&self, message: String) {
        self.pb.set_message(message.replace('\r', ""));
    }
    fn inc(&self, delta: u64) {
        self.pb.inc(delta);
        progress_trace!(
            "inc[{:?}]: delta={}, new_pos={}",
            self.report_id,
            delta,
            self.pb.position()
        );
        self.update_terminal_progress();
        if Some(self.pb.position()) == self.pb.length() {
            self.pb.set_style(SPIN_TEMPLATE.clone());
            self.pb.enable_steady_tick(TICK_INTERVAL);
        }
    }
    fn set_position(&self, pos: u64) {
        self.pb.set_position(pos);
        progress_trace!("set_position[{:?}]: pos={}", self.report_id, pos);
        self.update_terminal_progress();
        self.update_footer_display();
        if Some(self.pb.position()) == self.pb.length() {
            self.pb.set_style(SPIN_TEMPLATE.clone());
            self.pb.enable_steady_tick(Duration::from_millis(250));
        }
    }
    fn set_length(&self, length: u64) {
        // Atomically update operation count and base together to prevent race conditions
        let mut op_count = self.operation_count.lock().unwrap();
        *op_count += 1;
        let count = *op_count;

        // When starting a new operation (count > 1), complete the previous operation first
        let (base, per_operation) = if count > 1 {
            let mut base_guard = self.operation_base.lock().unwrap();
            let prev_allocated = *self.operation_length.lock().unwrap();
            let prev_base = *base_guard;
            let completed_position = prev_base + prev_allocated;

            progress_trace!(
                "set_length[{:?}]: completing op {}, moving base {} -> {}",
                self.report_id,
                count - 1,
                prev_base,
                completed_position
            );

            // Report completion of previous operation
            if let Some(report_id) = self.report_id
                && let Some(mpr) = ui::multi_progress_report::MultiProgressReport::try_get()
            {
                mpr.update_report_progress(report_id, completed_position, 1_000_000);
            }

            // New operation starts where previous ended
            *base_guard = completed_position;

            // Calculate allocation with the new base
            let total_ops = self.total_operations.lock().unwrap();
            let total = (*total_ops).unwrap_or(1).max(1); // Ensure at least 1 to prevent division by zero
            let per_operation = 1_000_000 / total as u64;

            (completed_position, per_operation)
        } else {
            // First operation
            let total_ops = self.total_operations.lock().unwrap();
            let total = (*total_ops).unwrap_or(1).max(1); // Ensure at least 1 to prevent division by zero
            let base = *self.operation_base.lock().unwrap();
            let per_operation = 1_000_000 / total as u64;

            (base, per_operation)
        };

        drop(op_count); // Release operation_count lock

        *self.operation_length.lock().unwrap() = per_operation;

        let total = self.total_operations.lock().unwrap().unwrap_or(1).max(1);
        progress_trace!(
            "set_length[{:?}]: op={}/{}, base={}, allocated={}, pb_length={}",
            self.report_id,
            count,
            total,
            base,
            per_operation,
            length
        );

        self.pb.set_position(0);
        self.pb.set_style(PROG_TEMPLATE.clone());
        self.pb.disable_steady_tick();
        self.pb.set_length(length);
        self.update_terminal_progress();
    }
    fn abandon(&self) {
        self.pb.abandon();
    }
    fn finish_with_icon(&self, _message: String, _icon: ProgressIcon) {
        progress_trace!("finish_with_icon[{:?}]", self.report_id);
        // For footer bars with text overlay, use finish_with_message to clear it
        if self.footer_text.is_some() {
            self.pb.finish_with_message("");
        } else {
            self.pb.finish_and_clear();
        }
        // Mark this report as complete (100%) using fixed 0-1,000,000 range
        if let Some(report_id) = self.report_id
            && let Some(mpr) = ui::multi_progress_report::MultiProgressReport::try_get()
        {
            progress_trace!("finish_with_icon[{}]: marking as 100% complete", report_id);
            mpr.update_report_progress(report_id, 1_000_000, 1_000_000);
        }
    }

    fn start_operations(&self, count: usize) {
        progress_trace!(
            "start_operations[{:?}]: declaring {} operations",
            self.report_id,
            count
        );
        *self.total_operations.lock().unwrap() = Some(count.max(1));
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
}

impl VerboseReport {
    pub fn new(prefix: String) -> VerboseReport {
        VerboseReport {
            prefix,
            prev_message: Mutex::new("".to_string()),
            pad: *LONGEST_PLUGIN_NAME,
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
        let prefix = pad_prefix(self.pad, &self.prefix);
        log::info!("{prefix} {message}");
        *prev_message = message.clone();
    }
    fn finish(&self) {
        self.finish_with_message(style::egreen("done").to_string());
    }
    fn finish_with_icon(&self, message: String, icon: ProgressIcon) {
        let prefix = pad_prefix(self.pad - 2, &self.prefix);
        log::info!("{prefix} {icon} {message}");
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
