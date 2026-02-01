use std::sync::{Arc, Mutex};

use clx::progress::{self, ProgressJobBuilder, ProgressOutput};

use crate::cli::version::VERSION_PLAIN;
use crate::config::Settings;
use crate::env;
use crate::ui::progress_report::{ProgressReport, QuietReport, SingleReport, VerboseReport};

#[derive(Debug)]
pub struct MultiProgressReport {
    quiet: bool,
    verbose: bool,
    raw: bool,
    has_stderr: bool,
    force_progress: bool,
    total_count: Mutex<usize>,
    completed_count: Mutex<usize>,
    /// Header job for updating progress display
    header_job: Mutex<Option<Arc<progress::ProgressJob>>>,
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
        let force_progress = *env::MISE_FORCE_PROGRESS;

        progress_trace!(
            "MultiProgressReport::new: raw={}, quiet={}, verbose={}, has_stderr={}, force_progress={}",
            settings.raw,
            settings.quiet,
            settings.verbose,
            has_stderr,
            force_progress,
        );

        // Configure clx output mode based on settings
        // MISE_FORCE_PROGRESS=1 forces progress UI even in non-TTY (for debugging)
        let use_progress_ui =
            !settings.raw && !settings.quiet && !settings.verbose && (has_stderr || force_progress);
        if !use_progress_ui {
            progress::set_output(ProgressOutput::Text);
        }

        // Configure OSC progress based on settings
        if !settings.terminal_progress {
            // Disable OSC progress if terminal_progress is disabled
            // clx::osc::configure panics if called more than once (singleton pattern),
            // so we use catch_unwind to safely ignore duplicate calls
            let _ = std::panic::catch_unwind(|| {
                clx::osc::configure(false);
            });
        }

        MultiProgressReport {
            quiet: settings.quiet,
            verbose: settings.verbose,
            raw: settings.raw,
            has_stderr,
            force_progress,
            total_count: Mutex::new(0),
            completed_count: Mutex::new(0),
            header_job: Mutex::new(None),
        }
    }

    /// Check if we should use UI-style progress (not quiet/verbose/raw)
    fn use_progress_ui(&self) -> bool {
        !self.raw && !self.quiet && !self.verbose && (self.has_stderr || self.force_progress)
    }

    pub fn add(&self, prefix: &str) -> Box<dyn SingleReport> {
        self.add_with_options(prefix, false)
    }

    pub fn add_with_options(&self, prefix: &str, dry_run: bool) -> Box<dyn SingleReport> {
        if self.quiet {
            progress_trace!(
                "add_with_options[{}]: creating QuietReport (quiet=true)",
                prefix
            );
            Box::new(QuietReport::new())
        } else if self.use_progress_ui() && !dry_run {
            progress_trace!(
                "add_with_options[{}]: creating ProgressReport with clx",
                prefix
            );
            Box::new(ProgressReport::new(prefix.into()))
        } else {
            progress_trace!(
                "add_with_options[{}]: creating VerboseReport (use_progress_ui={}, dry_run={})",
                prefix,
                self.use_progress_ui(),
                dry_run
            );
            Box::new(VerboseReport::new(prefix.to_string()))
        }
    }

    pub fn init_footer(&self, dry_run: bool, _message: &str, total_count: usize) {
        // Only create header once - check if already initialized
        if self.header_job.lock().unwrap().is_some() {
            return;
        }

        // Set total count for progress tracking
        *self.total_count.lock().unwrap() = total_count;
        progress_trace!("init_footer: total_count={}", total_count);

        // Don't show header when there's only 1 tool - individual progress bar is sufficient
        if total_count <= 1 {
            return;
        }

        // Don't show header in quiet mode
        if self.quiet {
            return;
        }

        // Create header job showing overall progress (only in progress UI mode)
        // Left-aligned, colored header with "mise VERSION by @jdx" and cur/total count
        if self.use_progress_ui() && !dry_run {
            use crate::ui::style;

            // Build colored header text parts
            let mise_text = format!("{}", style::emagenta("mise").bold());
            let version_text = format!("{}", style::edim(&*VERSION_PLAIN));
            let by_text = format!("{}", style::edim("by @jdx"));

            // Template showing: "mise VERSION by @jdx                  [cur/total]"
            let header_body = "{{ mise }} {{ version }} {{ by | flex_fill }} {{ progress }}";

            let job = ProgressJobBuilder::new()
                .body(header_body)
                .prop("mise", &mise_text)
                .prop("version", &version_text)
                .prop("by", &by_text)
                .prop("progress", &format!("[0/{}]", total_count))
                .progress_total(total_count)
                .progress_current(0)
                .start();
            *self.header_job.lock().unwrap() = Some(job);
        }
    }

    pub fn footer_inc(&self, n: usize) {
        if n == 0 {
            return;
        }
        let completed = {
            let mut c = self.completed_count.lock().unwrap();
            *c += n;
            *c
        };
        let total = *self.total_count.lock().unwrap();
        progress_trace!("footer_inc: completed={}, total={}", completed, total);

        // Update header job progress display
        if let Some(job) = self.header_job.lock().unwrap().as_ref() {
            job.prop("progress", &format!("[{}/{}]", completed, total));
            job.progress_current(completed);
        }
    }

    pub fn footer_finish(&self) {
        let total = *self.total_count.lock().unwrap();
        let completed = *self.completed_count.lock().unwrap();

        progress_trace!("footer_finish: completed={}, total={}", completed, total);

        // Stop clx progress
        progress::stop();

        // Reset state for subsequent install operations (e.g., in daemon mode)
        *self.header_job.lock().unwrap() = None;
        *self.completed_count.lock().unwrap() = 0;
        *self.total_count.lock().unwrap() = 0;
    }

    pub fn stop(&self) -> eyre::Result<()> {
        progress::stop_clear();
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
