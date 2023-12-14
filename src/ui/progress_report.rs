use std::time::Duration;

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;

pub trait SingleReport: Send + Sync {
    fn enable_steady_tick(&self) {}
    fn set_prefix(&mut self, prefix: &str);
    fn prefix(&self) -> String;
    fn set_style(&self, _style: ProgressStyle) {}
    fn set_message(&self, _message: String) {}
    fn println(&self, _message: String) {}
    fn warn(&self, _message: String) {}
    fn error(&self, _message: String) {}
    fn finish(&self) {}
    fn finish_with_message(&self, _message: String) {}
}

pub static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template("{prefix}{wide_msg} {spinner:.blue} {elapsed:3.dim.italic}")
        .unwrap()
});

static SUCCESS_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!(
        "{{prefix}}{{wide_msg}} {} {{elapsed:3.dim.italic}}",
        style("✓").bright().green().for_stderr()
    );
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

static ERROR_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!(
        "{{prefix:.red}}{{wide_msg}} {} {{elapsed:3.dim.italic}}",
        style("✗").red().for_stderr()
    );
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

#[derive(Debug)]
pub struct ProgressReport {
    pub pb: ProgressBar,
}

impl ProgressReport {
    pub fn new(pb: ProgressBar) -> ProgressReport {
        ProgressReport { pb }
    }
}

impl SingleReport for ProgressReport {
    fn enable_steady_tick(&self) {
        self.pb.enable_steady_tick(Duration::from_millis(250));
    }

    fn set_prefix(&mut self, prefix: &str) {
        self.pb.set_prefix(prefix.to_string());
    }

    fn prefix(&self) -> String {
        self.pb.prefix()
    }

    fn set_style(&self, style: ProgressStyle) {
        self.pb.set_style(style);
        self.pb
            .set_prefix(console::style("rtx").dim().for_stderr().to_string());
    }
    fn set_message(&self, message: String) {
        self.pb.set_message(message.replace('\r', ""));
    }
    fn println(&self, message: String) {
        self.pb.println(message);
    }
    fn warn(&self, message: String) {
        self.pb
            .println(format!("{} {}", style("[WARN]").yellow(), message));
    }
    fn error(&self, message: String) {
        self.set_message(format!(
            "{} {}",
            style("[ERROR]").red().for_stderr(),
            message
        ));
        self.pb.set_style(ERROR_TEMPLATE.clone());
        self.pb.finish();
    }
    fn finish(&self) {
        self.pb.set_style(SUCCESS_TEMPLATE.clone());
        self.pb.finish()
    }
    fn finish_with_message(&self, message: String) {
        self.pb.set_style(SUCCESS_TEMPLATE.clone());
        self.pb.finish_with_message(message);
    }
}

pub struct QuietReport {
    prefix: String,
}
impl QuietReport {
    pub fn new() -> QuietReport {
        QuietReport {
            prefix: String::new(),
        }
    }
}
impl SingleReport for QuietReport {
    fn set_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.to_string();
    }

    fn prefix(&self) -> String {
        self.prefix.clone()
    }
}
pub struct VerboseReport {
    prefix: String,
}
impl VerboseReport {
    pub fn new() -> VerboseReport {
        VerboseReport {
            prefix: String::new(),
        }
    }
}
impl SingleReport for VerboseReport {
    fn set_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.to_string();
    }

    fn prefix(&self) -> String {
        self.prefix.clone()
    }

    fn set_message(&self, message: String) {
        eprintln!("{}", message);
    }
    fn println(&self, message: String) {
        eprintln!("{}", message);
    }
    fn warn(&self, message: String) {
        eprintln!("{}", message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_report() {
        let mut pr = ProgressReport::new(ProgressBar::new(0));
        pr.set_prefix("prefix");
        assert_eq!(pr.prefix(), "prefix");
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }

    #[test]
    fn test_progress_report_verbose() {
        let mut pr = VerboseReport::new();
        pr.set_prefix("prefix");
        assert_eq!(pr.prefix(), "prefix");
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }

    #[test]
    fn test_progress_report_quiet() {
        let mut pr = QuietReport::new();
        pr.set_prefix("prefix");
        assert_eq!(pr.prefix(), "prefix");
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }
}
