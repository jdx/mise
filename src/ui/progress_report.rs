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

    /// Start a new sub-step with a given weight (0.0-1.0) relative to total progress
    /// For example, if download is 50% and extract is 50%, call:
    /// - start_substep(0.5) before download
    /// - start_substep(0.5) before extract
    fn start_substep(&self, _weight: f64) {}
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

static HEADER_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let width = match *env::TERM_WIDTH {
        0..=79 => 10,
        80..=99 => 15,
        _ => 20,
    };
    let tmpl = format!(r#"{{prefix}} {{bar:{width}.cyan/blue}} {{pos}}/{{len:2}}"#);
    ProgressStyle::with_template(&tmpl).unwrap()
});

#[derive(Debug)]
pub struct ProgressReport {
    pub pb: ProgressBar,
    report_id: Option<usize>,
    // Track sub-steps: accumulated_progress + (current_weight * current_step_progress)
    substep_base: Mutex<f64>,    // Progress accumulated from completed substeps (0.0-1.0)
    substep_weight: Mutex<f64>,  // Weight of current substep (0.0-1.0)
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
            substep_base: Mutex::new(0.0),
            substep_weight: Mutex::new(1.0), // Default: single step with full weight
        }
    }

    pub fn new_header(prefix: String, length: u64, message: String) -> ProgressReport {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let pad = *LONGEST_PLUGIN_NAME;
        let pb = ProgressBar::new(length)
            .with_style(HEADER_TEMPLATE.clone())
            .with_prefix(pad_prefix(pad, &prefix))
            .with_message(message);
        pb.enable_steady_tick(TICK_INTERVAL);
        ProgressReport {
            pb,
            report_id: None,
            substep_base: Mutex::new(0.0),
            substep_weight: Mutex::new(1.0),
        }
    }

    fn update_terminal_progress(&self) {
        // Update the multi-progress report with this report's progress
        // accounting for substep weights
        if let Some(report_id) = self.report_id {
            if let Some(mpr) = ui::multi_progress_report::MultiProgressReport::try_get() {
                let substep_base = *self.substep_base.lock().unwrap();
                let substep_weight = *self.substep_weight.lock().unwrap();

                let current_step_progress = if let Some(length) = self.pb.length() {
                    if length > 0 {
                        self.pb.position() as f64 / length as f64
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                // Total progress = base + (weight * current_step)
                let total_progress = substep_base + (substep_weight * current_step_progress);
                let total_progress = total_progress.clamp(0.0, 1.0);

                // Convert to position/length for MultiProgressReport (using 100 as the fixed length)
                let position = (total_progress * 100.0) as u64;
                mpr.update_report_progress(report_id, position, 100);
            }
        }
    }
}

impl SingleReport for ProgressReport {
    fn println(&self, message: String) {
        // Suspend the entire MultiProgress to prevent header duplication
        crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
            eprintln!("{message}");
        });
    }
    fn set_message(&self, message: String) {
        self.pb.set_message(message.replace('\r', ""));
    }
    fn inc(&self, delta: u64) {
        self.pb.inc(delta);
        self.update_terminal_progress();
        if Some(self.pb.position()) == self.pb.length() {
            self.pb.set_style(SPIN_TEMPLATE.clone());
            self.pb.enable_steady_tick(TICK_INTERVAL);
        }
    }
    fn set_position(&self, pos: u64) {
        self.pb.set_position(pos);
        self.update_terminal_progress();
        if Some(self.pb.position()) == self.pb.length() {
            self.pb.set_style(SPIN_TEMPLATE.clone());
            self.pb.enable_steady_tick(Duration::from_millis(250));
        }
    }
    fn set_length(&self, length: u64) {
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
        self.pb.finish_and_clear();
        // Mark this report as complete (100%)
        if let Some(report_id) = self.report_id {
            if let Some(mpr) = ui::multi_progress_report::MultiProgressReport::try_get() {
                mpr.update_report_progress(report_id, 100, 100);
            }
        }
    }

    fn start_substep(&self, weight: f64) {
        let weight = weight.clamp(0.0, 1.0);

        // Save progress from completed substeps and start new substep
        let mut substep_base = self.substep_base.lock().unwrap();
        let current_weight = *self.substep_weight.lock().unwrap();

        // Add the completed portion of the previous substep to the base
        if let Some(length) = self.pb.length() {
            if length > 0 {
                let prev_progress = self.pb.position() as f64 / length as f64;
                *substep_base += current_weight * prev_progress;
            }
        }

        // Set new weight for this substep
        *self.substep_weight.lock().unwrap() = weight;

        // Reset the progress bar for this substep
        self.pb.set_position(0);
        self.pb.set_length(100); // Use a standard length
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
