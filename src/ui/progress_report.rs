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
}

impl ProgressReport {
    pub fn new(prefix: String) -> ProgressReport {
        let pb = ProgressBar::new(100)
            .with_style(PROG_TEMPLATE.clone())
            .with_prefix(format!("{} {prefix}", style("rtx").dim().for_stderr()));
        pb.enable_steady_tick(Duration::from_millis(250));
        ProgressReport { prefix, pb }
    }

    fn error_prefix(&self) -> String {
        format!("{} {}", style("rtx").red().for_stderr(), self.prefix)
    }
    fn success_prefix(&self) -> String {
        format!("{} {}", style("rtx").green().for_stderr(), self.prefix)
    }
}

impl SingleReport for ProgressReport {
    fn println(&self, message: String) {
        self.pb.println(message);
    }
    fn warn(&self, message: String) {
        let msg = format!("{} {message}", style("[WARN]").yellow().for_stderr());
        self.pb.println(msg);
    }
    fn error(&self, message: String) {
        let msg = format!("{} {message}", style("[ERROR]").red().for_stderr());
        self.set_message(msg);
        self.pb.set_style(ERROR_TEMPLATE.clone());
        self.pb.set_prefix(self.error_prefix());
        self.pb.finish();
    }
    fn set_message(&self, message: String) {
        self.pb.set_message(message.replace('\r', ""));
    }
    fn finish(&self) {
        self.pb.set_style(SUCCESS_TEMPLATE.clone());
        self.pb.set_prefix(self.success_prefix());
        self.pb.finish()
    }
    fn finish_with_message(&self, message: String) {
        self.pb.set_style(SUCCESS_TEMPLATE.clone());
        self.pb.set_prefix(self.success_prefix());
        self.pb.finish_with_message(message);
    }
}

pub struct QuietReport {
    prefix: String,
}

impl QuietReport {
    pub fn new(prefix: String) -> QuietReport {
        QuietReport { prefix }
    }
}

impl SingleReport for QuietReport {
    fn warn(&self, message: String) {
        warn!("{} {message}", self.prefix);
    }
    fn error(&self, message: String) {
        error!("{} {message}", self.prefix);
    }
}

pub struct VerboseReport {
    prefix: String,
}

impl VerboseReport {
    pub fn new(prefix: String) -> VerboseReport {
        VerboseReport { prefix }
    }
}

impl SingleReport for VerboseReport {
    fn println(&self, message: String) {
        eprintln!("{message}");
    }
    fn warn(&self, message: String) {
        warn!("{} {message}", self.prefix);
    }
    fn error(&self, message: String) {
        error!("{} {message}", self.prefix);
    }
    fn set_message(&self, message: String) {
        eprintln!("{} {message}", self.prefix);
    }
    fn finish(&self) {
        self.finish_with_message(style("done").green().for_stderr().to_string());
    }
    fn finish_with_message(&self, message: String) {
        self.set_message(message);
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
