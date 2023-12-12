use crate::config::Settings;
use console::style;
use indicatif::MultiProgress;

use crate::ui::progress_report::ProgressReport;

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
}

impl MultiProgressReport {
    pub fn new(settings: &Settings) -> Self {
        let mp = match settings.quiet || settings.verbose || !console::user_attended_stderr() {
            true => None,
            false => Some(MultiProgress::new()),
        };
        MultiProgressReport {
            mp,
            quiet: settings.quiet,
        }
    }
    pub fn add(&self) -> ProgressReport {
        match &self.mp {
            Some(mp) => {
                let mut pr = ProgressReport::new(false, self.quiet);
                pr.pb = Some(mp.add(pr.pb.unwrap()));
                pr
            }
            None => ProgressReport::new(true, self.quiet),
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
    // pub fn clear(&self) {
    //     match &self.mp {
    //         Some(mp) => {
    //             let _ = mp.clear();
    //         },
    //         None => ()
    //     }
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_progress_report() {
        let settings = Settings::default();
        let mpr = MultiProgressReport::new(&settings);
        let pr = mpr.add();
        pr.set_style(indicatif::ProgressStyle::with_template("").unwrap());
        pr.enable_steady_tick();
        pr.finish_with_message("test");
        pr.println("");
        pr.set_message("test");
    }
}
