use crate::config::Settings;
use console::style;
use indicatif::MultiProgress;

use crate::ui::progress_report::{ProgressReport, QuietReport, SingleReport, VerboseReport};

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
}

impl MultiProgressReport {
    pub fn new() -> Self {
        let settings = Settings::get();
        let mp = match settings.raw
            || settings.quiet
            || settings.verbose
            || !console::user_attended_stderr()
        {
            true => None,
            false => Some(MultiProgress::new()),
        };
        MultiProgressReport {
            mp,
            quiet: settings.quiet,
        }
    }
    pub fn add(&self, prefix: &str) -> Box<dyn SingleReport> {
        match &self.mp {
            _ if self.quiet => Box::new(QuietReport::new(prefix.to_string())),
            Some(mp) => {
                let mut pr = ProgressReport::new(prefix.into());
                pr.pb = mp.add(pr.pb);
                Box::new(pr)
            }
            None => Box::new(VerboseReport::new(prefix.to_string())),
        }
    }
    pub fn suspend<F: FnOnce() -> R, R>(&self, f: F) -> R {
        match &self.mp {
            Some(mp) => mp.suspend(f),
            None => f(),
        }
    }
    pub fn warn(&self, message: String) {
        match &self.mp {
            Some(pb) => {
                let _ = pb.println(format!(
                    "{} {}",
                    style("[WARN]").yellow().for_stderr(),
                    message
                ));
            }
            None if !self.quiet => warn!("{}", message),
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_progress_report() {
        let mpr = MultiProgressReport::new();
        let pr = mpr.add("PREFIX");
        pr.finish_with_message("test".into());
        pr.println("".into());
        pr.set_message("test".into());
    }
}
