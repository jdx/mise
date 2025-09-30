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
        ProgressReport { pb }
    }

    pub fn new_header(prefix: String, length: u64, message: String) -> ProgressReport {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let pad = *LONGEST_PLUGIN_NAME;
        let pb = ProgressBar::new(length)
            .with_style(HEADER_TEMPLATE.clone())
            .with_prefix(pad_prefix(pad, &prefix))
            .with_message(message);
        pb.enable_steady_tick(TICK_INTERVAL);
        ProgressReport { pb }
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
        if Some(self.pb.position()) == self.pb.length() {
            self.pb.set_style(SPIN_TEMPLATE.clone());
            self.pb.enable_steady_tick(TICK_INTERVAL);
        }
    }
    fn set_position(&self, pos: u64) {
        self.pb.set_position(pos);
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
    }
    fn abandon(&self) {
        self.pb.abandon();
    }
    fn finish_with_icon(&self, _message: String, _icon: ProgressIcon) {
        self.pb.finish_and_clear();
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
