use std::borrow::Cow;
use std::time::Duration;

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;

#[derive(Debug)]
pub struct ProgressReport {
    pub pb: Option<ProgressBar>,
    prefix: String,
}

pub static PROG_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template("{prefix}{wide_msg} {spinner:.blue} {elapsed:3.dim.italic}")
        .unwrap()
});

pub static SUCCESS_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!(
        "{{prefix}}{{wide_msg}} {} {{elapsed:3.dim.italic}}",
        style("✓").bright().green().for_stderr()
    );
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

pub static ERROR_TEMPLATE: Lazy<ProgressStyle> = Lazy::new(|| {
    let tmpl = format!(
        "{{prefix:.red}}{{wide_msg}} {} {{elapsed:3.dim.italic}}",
        style("✗").red().for_stderr()
    );
    ProgressStyle::with_template(tmpl.as_str()).unwrap()
});

impl ProgressReport {
    pub fn new(verbose: bool) -> ProgressReport {
        let pb = match verbose {
            true => None,
            false => Some(ProgressBar::new(0)),
        };
        ProgressReport {
            pb,
            prefix: String::new(),
        }
    }

    pub fn enable_steady_tick(&self) {
        match &self.pb {
            Some(pb) => pb.enable_steady_tick(Duration::from_millis(250)),
            None => (),
        }
    }

    pub fn set_prefix(&mut self, prefix: impl Into<Cow<'static, str>>) {
        match &self.pb {
            Some(pb) => pb.set_prefix(prefix),
            None => {
                self.prefix = prefix.into().to_string();
            }
        }
    }

    pub fn prefix(&self) -> String {
        match &self.pb {
            Some(pb) => pb.prefix(),
            None => self.prefix.clone(),
        }
    }

    pub fn set_style(&self, style: ProgressStyle) {
        match &self.pb {
            Some(pb) => {
                pb.set_style(style);
                pb.set_prefix(console::style("rtx").dim().for_stderr().to_string());
            }
            None => (),
        }
    }
    pub fn set_message<S: AsRef<str>>(&self, message: S) {
        match &self.pb {
            Some(pb) => pb.set_message(message.as_ref().replace('\r', "")),
            None => eprintln!("{}{}", self.prefix, message.as_ref()),
        }
    }
    pub fn println<S: AsRef<str>>(&self, message: S) {
        match &self.pb {
            Some(pb) => pb.println(message),
            None => eprintln!("{}", message.as_ref()),
        }
    }
    pub fn warn<S: AsRef<str>>(&self, message: S) {
        match &self.pb {
            Some(pb) => pb.println(format!("{} {}", style("[WARN]").yellow(), message.as_ref())),
            None => eprintln!("{}{}", self.prefix, message.as_ref()),
        }
    }
    pub fn error(&self) {
        match &self.pb {
            Some(pb) => {
                pb.set_style(ERROR_TEMPLATE.clone());
                pb.finish()
            }
            None => (),
        }
    }
    pub fn finish(&self) {
        match &self.pb {
            Some(pb) => {
                pb.set_style(SUCCESS_TEMPLATE.clone());
                pb.finish()
            }
            None => (),
        }
    }
    pub fn finish_with_message(&self, message: impl Into<Cow<'static, str>>) {
        match &self.pb {
            Some(pb) => {
                pb.set_style(SUCCESS_TEMPLATE.clone());
                pb.finish_with_message(message);
            }
            None => eprintln!("{}{}", self.prefix, message.into()),
        }
    }
    // pub fn clear(&self) {
    //     match &self.pb {
    //         Some(pb) => pb.finish_and_clear(),
    //         None => (),
    //     }
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_report() {
        let mut pr = ProgressReport::new(false);
        pr.set_prefix("prefix");
        assert_eq!(pr.prefix(), "prefix");
        pr.set_message("message");
        pr.finish_with_message("message");
    }

    #[test]
    fn test_progress_report_verbose() {
        let mut pr = ProgressReport::new(true);
        pr.set_prefix("prefix");
        assert_eq!(pr.prefix(), "prefix");
        pr.set_message("message");
        pr.finish_with_message("message");
    }
}
