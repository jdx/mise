//! Install-wide progress: one model, two renderers.
//!
//! [`crate::ui::text_install_progress`] appends snapshots for captured stderr and
//! CI; [`crate::ui::tty_install_progress`] redraws a live region for an
//! interactive terminal. Both read the same [`State`], so they cannot disagree
//! about what is done, what is running, or how far along it is.
use std::sync::Arc;

use super::multi_progress_report::MultiProgressReport;
use super::progress_report::SingleReport;
use super::text_install_progress::{Action, State, TextInstallProgress};
use super::tty_install_progress::TtyInstallProgress;

/// One tool's reporter inside an install session.
pub(crate) trait ToolProgress: SingleReport {
    /// The resolved `tool@version`, known only after resolution.
    fn set_prefix(&self, prefix: String);

    /// The worker's result. Authoritative over anything the backend reported:
    /// a backend may finish its report before its postinstall work returns.
    fn complete(&self, error: Option<&str>);

    /// A handle backends can drive through [`SingleReport`].
    fn reporter(&self) -> Box<dyn SingleReport>;
}

/// The whole install: which tools exist, what each is waiting on, and how it ended.
pub(crate) trait InstallProgress: Send + Sync {
    /// Begin one tool's work. `None` for a request that was not part of this
    /// session, so the caller falls back to the standard reporter rather than
    /// panicking in the scheduler.
    fn start_tool(&self, key: &str) -> Option<Box<dyn ToolProgress>>;

    /// A tool the scheduler is holding until these dependencies finish.
    fn set_waiting(&self, key: &str, dependencies: Vec<String>);

    /// Tools that never reached a worker (blocked by a failed dependency, or
    /// refused before scheduling) and the final summary.
    fn finish(&mut self, failures: Vec<(String, String)>);
}

/// Pick the renderer for this terminal, or `None` when the existing per-tool
/// reporters should be used unchanged (`--verbose`, `--raw`, `--quiet`).
pub(crate) fn install_progress(
    mpr: &Arc<MultiProgressReport>,
    tools: impl Iterator<Item = (String, String)>,
) -> Option<Box<dyn InstallProgress>> {
    progress_for(mpr, Action::Install, tools)
}

/// The same session for `prune`, `uninstall` and `upgrade`'s old versions.
/// Hundreds of `remove …` rows kept in a live region were what made a large
/// prune unreadable; here each finished removal is one permanent line.
pub(crate) fn removal_progress(
    mpr: &Arc<MultiProgressReport>,
    tools: impl Iterator<Item = (String, String)>,
) -> Option<Box<dyn InstallProgress>> {
    progress_for(mpr, Action::Remove, tools)
}

fn progress_for(
    mpr: &Arc<MultiProgressReport>,
    action: Action,
    tools: impl Iterator<Item = (String, String)>,
) -> Option<Box<dyn InstallProgress>> {
    let state = State::for_action(action, tools);
    if state.is_empty() {
        return None;
    }
    if mpr.use_tty_install_output() {
        Some(Box::new(TtyInstallProgress::new(state)))
    } else if mpr.use_text_install_output() {
        Some(Box::new(TextInstallProgress::new(state)))
    } else {
        None
    }
}
