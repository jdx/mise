use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use indicatif::{MultiProgress, ProgressBar};

use crate::cli::version::VERSION_PLAIN;
use crate::config::Settings;
use crate::ui::osc::{self, ProgressState};
use crate::ui::progress_report::{ProgressReport, QuietReport, SingleReport, VerboseReport};
use crate::ui::style;

#[derive(Debug)]
pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
    quiet: bool,
    footer: Mutex<Option<Box<dyn SingleReport>>>,
    footer_pb: Mutex<Option<ProgressBar>>, // Reference to footer bar for inserting before it
    // Track overall progress: total expected progress units and current progress per report
    total_count: Mutex<usize>,
    report_progress: Mutex<HashMap<usize, (u64, u64)>>, // report_id -> (position, length)
    next_report_id: Mutex<usize>,
    last_osc_percentage: Mutex<Option<u8>>, // Last OSC percentage sent, to avoid duplicate updates
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
        let has_stderr = console::user_attended_stderr();
        let force_progress = *crate::env::MISE_PROGRESS_TRACE; // Force progress bars when tracing
        progress_trace!(
            "MultiProgressReport::new: raw={}, quiet={}, verbose={}, user_attended_stderr={}, force_progress={}",
            settings.raw,
            settings.quiet,
            settings.verbose,
            has_stderr,
            force_progress
        );
        let mp = match (settings.raw || settings.quiet || settings.verbose || !has_stderr)
            && !force_progress
        {
            true => {
                progress_trace!(
                    "MultiProgressReport::new: mp=None (one of the conditions is true)"
                );
                None
            }
            false => {
                progress_trace!("MultiProgressReport::new: mp=Some(MultiProgress)");
                Some(MultiProgress::new())
            }
        };
        MultiProgressReport {
            mp,
            quiet: settings.quiet,
            footer: Mutex::new(None),
            footer_pb: Mutex::new(None),
            total_count: Mutex::new(0),
            report_progress: Mutex::new(HashMap::new()),
            next_report_id: Mutex::new(0),
            last_osc_percentage: Mutex::new(None),
        }
    }
    pub fn add(&self, prefix: &str) -> Box<dyn SingleReport> {
        self.add_with_options(prefix, false)
    }

    pub fn add_with_options(&self, prefix: &str, dry_run: bool) -> Box<dyn SingleReport> {
        match &self.mp {
            _ if self.quiet => {
                progress_trace!(
                    "add_with_options[{}]: creating QuietReport (quiet=true)",
                    prefix
                );
                Box::new(QuietReport::new())
            }
            Some(mp) if !dry_run => {
                progress_trace!(
                    "add_with_options[{}]: creating ProgressReport with MultiProgress",
                    prefix
                );
                let mut pr = ProgressReport::new(prefix.into());
                // Insert before footer if it exists, otherwise just add
                let footer_pb = self.footer_pb.lock().unwrap();
                pr.pb = if let Some(footer) = footer_pb.as_ref() {
                    mp.insert_before(footer, pr.pb)
                } else {
                    mp.add(pr.pb)
                };
                Box::new(pr)
            }
            _ => {
                progress_trace!(
                    "add_with_options[{}]: creating VerboseReport (mp={:?}, dry_run={})",
                    prefix,
                    self.mp.is_some(),
                    dry_run
                );
                Box::new(VerboseReport::new(prefix.to_string()))
            }
        }
    }
    pub fn init_footer(&self, dry_run: bool, message: &str, total_count: usize) {
        let mut footer = self.footer.lock().unwrap();
        if let Some(_footer) = footer.as_ref() {
            return;
        }

        // Set total count for overall progress tracking
        *self.total_count.lock().unwrap() = total_count;
        progress_trace!(
            "init_footer: total_count={}, total_units={}",
            total_count,
            total_count * 1_000_000
        );

        // Don't show footer when there's only 1 tool - individual progress bar is sufficient
        if total_count == 1 {
            return;
        }

        // Initialize OSC progress if enabled
        if Settings::get().terminal_progress {
            osc::set_progress(ProgressState::Normal, 0);
            progress_trace!("init_footer: initialized OSC progress at 0%");
        }

        let version = &*VERSION_PLAIN;
        let version_text = format!("{version} by @jdx");
        *footer = Some(match &self.mp {
            _ if self.quiet => return,
            Some(mp) if !dry_run => {
                // Footer text with "mise" prefix for progress bar overlay
                let footer_text = format!(
                    "{} {}",
                    style::emagenta("mise").bold(),
                    style::edim(&version_text),
                );
                // Footer length is total_count * 1,000,000 to show progress with high granularity
                let footer_length = (total_count * 1_000_000) as u64;
                let mut footer_bar =
                    ProgressReport::new_footer(footer_text, footer_length, message.to_string());
                // Add footer to the end (it will be the last bar initially)
                footer_bar.pb = mp.add(footer_bar.pb);
                // Store reference to footer bar for inserting other bars before it
                *self.footer_pb.lock().unwrap() = Some(footer_bar.pb.clone());
                // Set initial message AFTER adding to MultiProgress to prevent ghost output
                footer_bar.set_position(0);
                Box::new(footer_bar)
            }
            _ => {
                // Don't include "mise" prefix for VerboseReport since logger already adds it
                let verbose = VerboseReport::new(version_text);
                verbose.set_message(message.to_string());
                Box::new(verbose)
            }
        });
    }
    pub fn footer_inc(&self, n: usize) {
        if n == 0 {
            return;
        }
        if let Some(f) = &*self.footer.lock().unwrap() {
            f.inc(n as u64);
        }
    }
    pub fn footer_finish(&self) {
        if let Some(f) = &*self.footer.lock().unwrap() {
            f.finish();
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
        progress_trace!("allocate_report_id: allocated report_id={}", id);
        id
    }

    /// Update a report's progress and recalculate overall progress
    pub fn update_report_progress(&self, report_id: usize, position: u64, length: u64) {
        progress_trace!(
            "update_report_progress: report_id={}, position={}, length={}",
            report_id,
            position,
            length
        );
        let mut progress = self.report_progress.lock().unwrap();
        progress.insert(report_id, (position, length));
        drop(progress); // Release lock before calling update_overall_progress
        self.update_overall_progress();
    }

    /// Calculate and send overall progress update to terminal
    /// Each report gets equal weight (1/total_count)
    /// Reports use 0-1,000,000 scale internally
    fn update_overall_progress(&self) {
        let total_count = *self.total_count.lock().unwrap();
        if total_count == 0 {
            progress_trace!("update_overall_progress: skipping, total_count=0");
            return;
        }

        let progress = self.report_progress.lock().unwrap();

        // Calculate weighted progress: each report contributes equally (1/N)
        // Reports provide position/length in 0-1,000,000 range
        let weight_per_report = 1.0 / total_count as f64;
        let mut total_progress = 0.0f64;

        progress_trace!(
            "update_overall_progress: total_count={}, weight_per_report={:.3}, num_reports={}",
            total_count,
            weight_per_report,
            progress.len()
        );

        for (report_id, (position, length)) in progress.iter() {
            let report_progress = if *length > 0 {
                (*position as f64 / *length as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let weighted_progress = weight_per_report * report_progress;
            total_progress += weighted_progress;

            progress_trace!(
                "  report_id={}: pos={}, len={}, progress={:.3}, weighted={:.3}",
                report_id,
                position,
                length,
                report_progress,
                weighted_progress
            );
        }

        total_progress = total_progress.clamp(0.0, 1.0);
        progress_trace!(
            "update_overall_progress: total_progress={:.3}",
            total_progress
        );

        // Update footer bar - convert to units for display
        let footer_units = (total_progress * (total_count * 1_000_000) as f64).round() as u64;
        if let Some(f) = &*self.footer.lock().unwrap() {
            f.set_position(footer_units);
        }

        // Update terminal OSC progress - only if percentage changed
        if Settings::get().terminal_progress {
            let overall_percentage = (total_progress * 100.0).clamp(0.0, 100.0) as u8;
            let mut last_pct = self.last_osc_percentage.lock().unwrap();

            if *last_pct != Some(overall_percentage) {
                progress_trace!(
                    "update_overall_progress: OSC progress={}%",
                    overall_percentage
                );
                osc::set_progress(ProgressState::Normal, overall_percentage);
                *last_pct = Some(overall_percentage);
            }
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
