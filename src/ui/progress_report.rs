use crate::config::Config;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;
use std::time::Duration;

pub trait SingleReport: Send + Sync {
    fn println(&self, _message: String) {}
    fn warn(&self, _message: String);
    fn error(&self, _message: String);
    fn set_message(&self, _message: String) {}
    fn finish(&self) {}
    fn finish_with_message(&self, _message: String) {}
}

static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template("{prefix} {wide_msg} {spinner:.blue} {elapsed:3.dim.italic}")
        .unwrap()
});

static SUCCESS_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!(
        "{{prefix}} {} {{wide_msg}}",
        style("✓").bright().green().for_stderr()
    );
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

static ERROR_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!("{{prefix}} {} {{wide_msg}}", style("✗").red().for_stderr());
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

#[derive(Debug)]
pub struct ProgressReport {
    pub pb: ProgressBar,
    prefix: String,
    pad: usize,
}

static LONGEST_PLUGIN_NAME: Lazy<usize> = Lazy::new(|| {
    Config::get()
        .list_plugins()
        .into_iter()
        .map(|p| p.name().len() + 12)
        .max()
        .unwrap_or_default()
        .max(20)
        .min(40)
});

fn pad_prefix(w: usize, s: &str) -> String {
    console::pad_str(s, w, console::Alignment::Left, None).to_string()
}
fn normal_prefix(pad: usize, prefix: &str) -> String {
    let prefix = format!("{} {prefix}", style("rtx").dim().for_stderr());
    pad_prefix(pad, &prefix)
}
fn error_prefix(pad: usize, prefix: &str) -> String {
    let prefix = format!("{} {prefix}", style("rtx").red().for_stderr());
    pad_prefix(pad, &prefix)
}
fn warn_prefix(pad: usize, prefix: &str) -> String {
    let prefix = format!("{} {prefix}", style("rtx").yellow().for_stderr());
    pad_prefix(pad, &prefix)
}
fn success_prefix(pad: usize, prefix: &str) -> String {
    let prefix = format!("{} {prefix}", style("rtx").green().for_stderr());
    pad_prefix(pad, &prefix)
}

impl ProgressReport {
    pub fn new(prefix: String) -> ProgressReport {
        let pad = *LONGEST_PLUGIN_NAME + 2;
        let pb = ProgressBar::new(100)
            .with_style(PROG_TEMPLATE.clone())
            .with_prefix(normal_prefix(pad, &prefix));
        pb.enable_steady_tick(Duration::from_millis(250));
        ProgressReport { prefix, pb, pad }
    }
}

impl SingleReport for ProgressReport {
    fn println(&self, message: String) {
        self.pb.println(message);
    }
    fn warn(&self, message: String) {
        let msg = format!("{} {message}", style("[WARN]").yellow().for_stderr());
        self.pb.set_prefix(warn_prefix(self.pad, &self.prefix));
        self.pb.println(msg);
    }
    fn error(&self, message: String) {
        let msg = format!("{} {message}", style("[ERROR]").red().for_stderr());
        self.set_message(msg);
        self.pb.set_style(ERROR_TEMPLATE.clone());
        self.pb.set_prefix(error_prefix(self.pad - 2, &self.prefix));
        self.pb.finish();
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

pub struct QuietReport {
    prefix: String,
    pad: usize,
}

impl QuietReport {
    pub fn new(prefix: String) -> QuietReport {
        QuietReport {
            prefix,
            pad: *LONGEST_PLUGIN_NAME + 2,
        }
    }
}

impl SingleReport for QuietReport {
    fn warn(&self, message: String) {
        let prefix = warn_prefix(self.pad - 2, &self.prefix);
        let x = style("⚠").yellow().for_stderr();
        warn!("{prefix} {x} {message}");
    }
    fn error(&self, message: String) {
        let prefix = error_prefix(self.pad - 2, &self.prefix);
        let x = style("✗").red().for_stderr();
        error!("{prefix} {x} {message}");
    }
}

pub struct VerboseReport {
    prefix: String,
    pad: usize,
}

impl VerboseReport {
    pub fn new(prefix: String) -> VerboseReport {
        VerboseReport {
            prefix,
            pad: *LONGEST_PLUGIN_NAME + 2,
        }
    }
}

impl SingleReport for VerboseReport {
    fn println(&self, message: String) {
        eprintln!("{message}");
    }
    fn warn(&self, message: String) {
        let prefix = warn_prefix(self.pad - 2, &self.prefix);
        let x = style("⚠").yellow().for_stderr();
        warn!("{prefix} {x} {message}");
    }
    fn error(&self, message: String) {
        let prefix = error_prefix(self.pad - 2, &self.prefix);
        let x = style("✗").red().for_stderr();
        error!("{prefix} {x} {message}");
    }
    fn set_message(&self, message: String) {
        let prefix = normal_prefix(self.pad, &self.prefix);
        eprintln!("{prefix} {message}");
    }
    fn finish(&self) {
        self.finish_with_message(style("done").green().for_stderr().to_string());
    }
    fn finish_with_message(&self, message: String) {
        let prefix = success_prefix(self.pad - 2, &self.prefix);
        let ico = style("✓").bright().green().for_stderr();
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
        let pr = QuietReport::new("PREFIX".to_string());
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }
}
