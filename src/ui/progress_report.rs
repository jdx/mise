use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;

pub trait SingleReport: Send + Sync {
    fn set_message(&self, _message: String) {}
    fn println(&self, _message: String) {}
    fn warn(&self, _message: String) {}
    fn error(&self, _message: String) {}
    fn finish(&self) {}
    fn finish_with_message(&self, _message: String) {}
}

pub static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template("{prefix} {wide_msg} {spinner:.blue} {elapsed:3.dim.italic}")
        .unwrap()
});

static SUCCESS_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!(
        "{{prefix}} {{wide_msg}} {} {{elapsed:3.dim.italic}}",
        style("✓").bright().green().for_stderr()
    );
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

static ERROR_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!(
        "{{prefix:.red}} {{wide_msg}} {} {{elapsed:3.dim.italic}}",
        style("✗").red().for_stderr()
    );
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

#[derive(Debug)]
pub struct ProgressReport {
    pb: ProgressBar,
}

impl ProgressReport {
    pub fn new(pb: ProgressBar) -> ProgressReport {
        ProgressReport { pb }
    }
}

impl SingleReport for ProgressReport {
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

pub struct QuietReport {}

impl QuietReport {
    pub fn new() -> QuietReport {
        QuietReport {}
    }
}

impl SingleReport for QuietReport {}

pub struct VerboseReport {}

impl VerboseReport {
    pub fn new() -> VerboseReport {
        VerboseReport {}
    }
}

impl SingleReport for VerboseReport {
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
        let pr = ProgressReport::new(ProgressBar::new(0));
        pr.set_message("message".into());
        pr.finish_with_message("message".into());
    }

    #[test]
    fn test_progress_report_verbose() {
        let pr = VerboseReport::new();
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
