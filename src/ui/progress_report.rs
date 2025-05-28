#![allow(unknown_lints)]
#![allow(clippy::literal_string_with_formatting_args)]

use std::sync::Mutex;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use std::sync::LazyLock as Lazy;

use crate::ui::style;
use crate::{backend, env, ui};

pub trait SingleReport: Send + Sync {
    fn println(&self, _message: String) {}
    fn set_message(&self, _message: String) {}
    fn inc(&self, _delta: u64) {}
    fn set_position(&self, _delta: u64) {}
    fn set_length(&self, _length: u64) {}
    fn abandon(&self) {}
    fn finish(&self) {}
    fn finish_with_message(&self, _message: String) {}
}

static SPIN_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = "{prefix} {wide_msg} {spinner:.blue} {elapsed:>3.dim.italic}";
    ProgressStyle::with_template(tmpl).unwrap()
});

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

static SUCCESS_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!("{{prefix}} {} {{wide_msg}}", style::egreen("✓").bright());
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

#[derive(Debug)]
pub struct ProgressReport {
    pub pb: ProgressBar,
    prefix: String,
    pad: usize,
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

fn success_prefix(pad: usize, prefix: &str) -> String {
    let prefix = format!("{} {prefix}", style::egreen("mise"));
    pad_prefix(pad, &prefix)
}

impl ProgressReport {
    pub fn new(prefix: String) -> ProgressReport {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let pad = *LONGEST_PLUGIN_NAME;
        let pb = ProgressBar::new(100)
            .with_style(SPIN_TEMPLATE.clone())
            .with_prefix(normal_prefix(pad, &prefix));
        pb.enable_steady_tick(Duration::from_millis(250));
        ProgressReport { prefix, pb, pad }
    }
}

impl SingleReport for ProgressReport {
    fn println(&self, message: String) {
        self.pb.suspend(|| {
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
            self.pb.enable_steady_tick(Duration::from_millis(250));
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
    fn finish(&self) {
        self.pb.set_style(SUCCESS_TEMPLATE.clone());
        self.pb
            .set_prefix(success_prefix(self.pad - 2, &self.prefix));
        self.pb.finish()
    }
    fn finish_with_message(&self, message: String) {
        self.pb.set_style(SUCCESS_TEMPLATE.clone());
        self.pb
            .set_prefix(success_prefix(self.pad - 2, &self.prefix));
        self.pb.finish_with_message(message);
    }
}

pub struct QuietReport {}

impl QuietReport {
    pub fn new() -> QuietReport {
        QuietReport {}
    }
}

impl SingleReport for QuietReport {}

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
    fn finish_with_message(&self, message: String) {
        let prefix = pad_prefix(self.pad - 2, &self.prefix);
        let ico = style::egreen("✓").bright();
        log::info!("{prefix} {ico} {message}");
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
