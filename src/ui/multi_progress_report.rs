use std::sync::{Arc, Mutex};

use indicatif::MultiProgress;

use crate::ui::progress_report::{ProgressReport, QuietReport, SingleReport, VerboseReport};
use crate::ui::style;
use crate::{cli::version::VERSION_PLAIN, config::Settings};

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
    header: Mutex<Option<Box<dyn SingleReport>>>,
}

static INSTANCE: Mutex<Option<Arc<MultiProgressReport>>> = Mutex::new(None);

impl MultiProgressReport {
    pub fn try_get() -> Option<Arc<Self>> {
        INSTANCE.lock().unwrap().as_ref().cloned()
    }
    pub fn get() -> Arc<Self> {
        let mut guard = INSTANCE.lock().unwrap();
        if let Some(existing) = guard.as_ref() {
            return existing.clone();
        }
        let mpr = Arc::new(Self::new());
        *guard = Some(mpr.clone());
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
            header: Mutex::new(None),
        }
    }
    pub fn add(&self, prefix: &str) -> Box<dyn SingleReport> {
        self.add_with_options(prefix, false)
    }

    pub fn add_with_options(&self, prefix: &str, dry_run: bool) -> Box<dyn SingleReport> {
        match &self.mp {
            _ if self.quiet => Box::new(QuietReport::new()),
            Some(mp) if !dry_run => {
                let mut pr = ProgressReport::new(prefix.into());
                // Always use add() to append progress bars
                // The header should already be at position 0 if it exists
                pr.pb = mp.add(pr.pb);
                Box::new(pr)
            }
            _ => Box::new(VerboseReport::new(prefix.to_string())),
        }
    }
    pub fn init_header(&self, dry_run: bool, message: &str, total_tools: usize) {
        let mut hdr = self.header.lock().unwrap();
        if let Some(_hdr) = hdr.as_ref() {
            return;
        }
        let version = &*VERSION_PLAIN;
        let prefix = format!(
            "{} {}",
            style::emagenta("mise").bold(),
            style::edim(format!("{version} by @jdx –")),
        );
        *hdr = Some(match &self.mp {
            _ if self.quiet => return,
            Some(mp) if !dry_run => {
                let mut header =
                    ProgressReport::new_header(prefix, total_tools as u64, message.to_string());
                header.pb = mp.add(header.pb);
                Box::new(header)
            }
            _ => {
                let header = VerboseReport::new(prefix);
                header.set_message(message.to_string());
                Box::new(header)
            }
        });
    }
    pub fn header_inc(&self, n: usize) {
        if n == 0 {
            return;
        }
        if let Some(h) = &*self.header.lock().unwrap() {
            h.inc(n as u64);
        }
    }
    pub fn header_finish(&self) {
        if let Some(h) = &*self.header.lock().unwrap() {
            h.finish();
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
    pub fn stop(&self) -> eyre::Result<()> {
        if let Some(mp) = &self.mp {
            mp.clear()?;
        }
        Ok(())
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
