use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use indicatif::MultiProgress;

use crate::config::Settings;
use crate::ui::osc::{self, ProgressState};
use crate::ui::progress_report::{ProgressReport, QuietReport, SingleReport, VerboseReport};
use crate::ui::style;
use crate::cli::version::VERSION_PLAIN;

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
    header: Mutex<Option<Box<dyn SingleReport>>>,
    // Track overall progress: total expected progress units and current progress per report
    total_count: Mutex<usize>,
    report_progress: Mutex<HashMap<usize, (u64, u64)>>, // report_id -> (position, length)
    next_report_id: Mutex<usize>,
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
            total_count: Mutex::new(0),
            report_progress: Mutex::new(HashMap::new()),
            next_report_id: Mutex::new(0),
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
    pub fn init_header(&self, dry_run: bool, message: &str, total_count: usize) {
        let mut hdr = self.header.lock().unwrap();
        if let Some(_hdr) = hdr.as_ref() {
            return;
        }

        // Set total count for overall progress tracking
        *self.total_count.lock().unwrap() = total_count;

        // Initialize OSC progress if enabled
        if Settings::get().terminal_progress {
            osc::set_progress(ProgressState::Progress, 0);
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
                // Header length is total_count * 100 to show progress instead of just count
                let header_length = (total_count * 100) as u64;
                let mut header =
                    ProgressReport::new_header(prefix, header_length, message.to_string());
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
        // Clear terminal progress when finished
        if Settings::get().terminal_progress {
            osc::clear_progress();
        }
    }

    /// Allocate a new report ID for progress tracking
    pub fn allocate_report_id(&self) -> usize {
        let mut next_id = self.next_report_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        id
    }

    /// Update a report's progress and recalculate overall progress
    pub fn update_report_progress(&self, report_id: usize, position: u64, length: u64) {
        let mut progress = self.report_progress.lock().unwrap();
        progress.insert(report_id, (position, length));
        drop(progress); // Release lock before calling update_overall_progress
        self.update_overall_progress();
    }

    /// Calculate and send overall progress update to terminal
    fn update_overall_progress(&self) {
        let total_count = *self.total_count.lock().unwrap();
        if total_count == 0 {
            return;
        }

        let progress = self.report_progress.lock().unwrap();

        // Calculate total progress: each report contributes 100 units
        let total_units = total_count * 100;
        let mut current_units = 0u64;

        for (_report_id, (position, length)) in progress.iter() {
            if *length > 0 {
                // Calculate percentage for this report (0-100)
                let report_percentage = ((*position as f64 / *length as f64) * 100.0) as u64;
                current_units += report_percentage.min(100);
            }
        }

        // Update header bar with overall progress
        if let Some(h) = &*self.header.lock().unwrap() {
            h.set_position(current_units);
        }

        // Update terminal OSC progress
        if Settings::get().terminal_progress {
            let overall_percentage = ((current_units as f64 / total_units as f64) * 100.0)
                .clamp(0.0, 100.0) as u8;
            osc::set_progress(ProgressState::Progress, overall_percentage);
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
        if Settings::get().terminal_progress {
            osc::clear_progress();
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
