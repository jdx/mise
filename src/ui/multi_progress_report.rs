use std::sync::{Arc, Mutex};

use indicatif::MultiProgress;

use crate::ui::progress_report::{
    HeaderReport, ProgressReport, QuietReport, SingleReport, VerboseReport,
};
use crate::ui::style;
use crate::{cli::version::VERSION_PLAIN, config::Settings};

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
    header: Mutex<Option<HeaderReport>>,
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
    pub fn init_header(&self, label: &str, total_tools: usize) {
        if self.mp.is_none() || self.quiet {
            return;
        }
        let mut hdr = self.header.lock().unwrap();
        match (&self.mp, hdr.as_ref()) {
            (Some(mp), None) => {
                let version = &*VERSION_PLAIN;
                let prefix = format!(
                    "{} {} {}",
                    style::emagenta("mise").bold(),
                    style::edim(format!("{version} by @jdx –")),
                    style::eblue(label),
                );
                let mut header = HeaderReport::new(prefix);
                header.pb = mp.add(header.pb);
                header.set_length(total_tools as u64);
                *hdr = Some(header);
            }
            (_, Some(_h)) => {
                // header already initialized; do not change total
            }
            _ => {}
        }
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
            h.finish_with_message("installed".to_string());
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
