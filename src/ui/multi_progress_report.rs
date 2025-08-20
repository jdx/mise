use std::sync::{Arc, Mutex};

use indicatif::MultiProgress;

use crate::ui::progress_report::{ProgressReport, QuietReport, SingleReport, VerboseReport};
use crate::ui::style;
use crate::{cli::version::VERSION_PLAIN, config::Settings};

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
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
                pr.pb = mp.add(pr.pb);
                Box::new(pr)
            }
            _ => Box::new(VerboseReport::new(prefix.to_string())),
        }
    }
    pub fn init_header(&self, message: &str, _total_tools: usize) {
        if self.quiet {
            return;
        }

        // Only print header for non-tty mode
        // In TTY mode with progress bars, the header gets buried and doesn't display properly
        if self.mp.is_none() {
            let version = &*VERSION_PLAIN;
            let icon = if message.contains("(dry-run)") {
                style::eyellow("○")
            } else {
                style::egreen("✓").bright()
            };
            eprintln!(
                "{} {} {} {}",
                style::emagenta("mise").bold(),
                style::edim(format!("{version} by @jdx –")),
                icon,
                message
            );
        }
        // Don't create a header progress bar for TTY mode
    }
    pub fn header_inc(&self, _n: usize) {
        // No header in TTY mode, so nothing to increment
    }
    pub fn header_finish(&self) {
        // No header to finish - it was already printed in non-TTY mode
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
