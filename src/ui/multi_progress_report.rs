use std::sync::{Arc, Mutex, Weak};

use indicatif::MultiProgress;

use crate::config::Settings;
use crate::ui::progress_report::{ProgressReport, QuietReport, SingleReport, VerboseReport};

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
}

static INSTANCE: Mutex<Option<Weak<MultiProgressReport>>> = Mutex::new(None);

impl MultiProgressReport {
    pub fn try_get() -> Option<Arc<Self>> {
        match &*INSTANCE.lock().unwrap() {
            Some(w) => w.upgrade(),
            None => None,
        }
    }
    pub fn get() -> Arc<Self> {
        let mut mutex = INSTANCE.lock().unwrap();
        if let Some(w) = &*mutex {
            if let Some(mpr) = w.upgrade() {
                return mpr;
            }
        }

        let mpr = Arc::new(Self::new());
        *mutex = Some(Arc::downgrade(&mpr));
        mpr
    }
    fn new() -> Self {
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
            _ if self.quiet => Box::new(QuietReport::new()),
            Some(mp) => {
                let mut pr = ProgressReport::new(prefix.into());
                pr.pb = mp.add(pr.pb);
                Box::new(pr)
            }
            None => Box::new(VerboseReport::new(prefix.to_string())),
        }
    }
    pub fn suspend_if_active<F: FnOnce() -> R, R>(f: F) -> R {
        match Self::try_get() {
            Some(mpr) => mpr.suspend(f),
            None => f(),
        }
    }
    pub fn suspend<F: FnOnce() -> R, R>(&self, f: F) -> R {
        match &self.mp {
            Some(mp) => mp.suspend(f),
            None => f(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_progress_report() {
        let mpr = MultiProgressReport::get();
        let pr = mpr.add("PREFIX");
        pr.finish_with_message("test".into());
        pr.println("".into());
        pr.set_message("test".into());
    }
}
