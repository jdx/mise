use crate::config::Settings;
use console::style;
use indicatif::{MultiProgress, ProgressBar};

use crate::ui::progress_report::{ProgressReport, QuietReport, SingleReport, VerboseReport};

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
}

impl MultiProgressReport {
    pub fn new() -> Self {
        let settings = Settings::get();
        let mp = match settings.quiet || settings.verbose || !console::user_attended_stderr() {
            true => None,
            false => Some(MultiProgress::new()),
        };
        MultiProgressReport {
            mp,
            quiet: settings.quiet,
        }
    }
    pub fn add(&self) -> Box<dyn SingleReport> {
        match &self.mp {
            _ if self.quiet => Box::new(QuietReport::new()),
            Some(mp) => Box::new(ProgressReport::new(mp.add(ProgressBar::new(0)))),
            None => Box::new(VerboseReport::new()),
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
        let pr = mpr.add();
        pr.set_style(indicatif::ProgressStyle::with_template("").unwrap());
        pr.enable_steady_tick();
        pr.finish_with_message("test".into());
        pr.println("".into());
        pr.set_message("test".into());
    }
}
