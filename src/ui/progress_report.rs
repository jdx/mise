use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;

use crate::ui::style;
use crate::{forge, ui};

pub trait SingleReport: Send + Sync {
    fn println(&self, _message: String) {}
    fn set_message(&self, _message: String) {}
    fn finish(&self) {}
    fn finish_with_message(&self, _message: String) {}
}

static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template("{prefix} {wide_msg} {spinner:.blue} {elapsed:3.dim.italic}")
        .unwrap()
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
    forge::list()
        .into_iter()
        .map(|p| p.name().len() + 10)
        .max()
        .unwrap_or_default()
        .max(15)
        .min(35)
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
        ui::handle_ctrlc();
        let pad = *LONGEST_PLUGIN_NAME;
        let pb = ProgressBar::new(100)
            .with_style(PROG_TEMPLATE.clone())
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
    pad: usize,
}

impl VerboseReport {
    pub fn new(prefix: String) -> VerboseReport {
        VerboseReport {
            prefix,
            pad: *LONGEST_PLUGIN_NAME,
        }
    }
}

impl SingleReport for VerboseReport {
    fn println(&self, message: String) {
        eprintln!("{message}");
    }
    fn set_message(&self, message: String) {
        // let prefix = normal_prefix(self.pad, &self.prefix);
        // eprintln!("{prefix} {message}");
        eprintln!("{message}");
    }
    fn finish(&self) {
        self.finish_with_message(style::egreen("done").to_string());
    }
    fn finish_with_message(&self, message: String) {
        let prefix = success_prefix(self.pad - 2, &self.prefix);
        let ico = style::egreen("✓").bright();
        eprintln!("{prefix} {ico} {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_report() {
        let pr = ProgressReport::new("foo".into());
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }

    #[test]
    fn test_progress_report_verbose() {
        let pr = VerboseReport::new("PREFIX".to_string());
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }

    #[test]
    fn test_progress_report_quiet() {
        let pr = QuietReport::new();
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }
}
