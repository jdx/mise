//! The progress-reporting interface shared by mise's crates. mise's renderers
//! implement [`SingleReport`]; code here only reports through it.

use std::fmt::{Display, Formatter};

use crate::style;

#[derive(Debug, Clone, Copy)]
pub enum ProgressIcon {
    Success,
    Skipped,
    #[allow(dead_code)]
    Warning,
    Error,
}

impl Display for ProgressIcon {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgressIcon::Success => write!(f, "{}", style::egreen("✓").bright()),
            ProgressIcon::Skipped => write!(f, "{}", style::eyellow("⇢").bright()),
            ProgressIcon::Warning => write!(f, "{}", style::eyellow("⚠").bright()),
            ProgressIcon::Error => write!(f, "{}", style::ered("✗").bright()),
        }
    }
}

pub trait SingleReport: Send + Sync + std::fmt::Debug {
    fn println(&self, _message: String) {}
    fn set_message(&self, _message: String) {}

    /// A line of a child process's stdout, as opposed to a phase message a
    /// backend produced itself.
    ///
    /// Reporters that only have room for one status line fold it in and rely on
    /// mise's `CmdLineRunner` replaying the whole stream when the command
    /// fails. Reporters that show it as it arrives say so with
    /// [`Self::shows_process_output`], which suppresses that replay.
    fn set_process_output(&self, message: String) {
        self.set_message(message);
    }

    /// Whether [`Self::set_process_output`] reaches the user immediately.
    fn shows_process_output(&self) -> bool {
        false
    }

    /// Secondary progress that is not a byte transfer — an embedded package
    /// manager's `32/48 pkgs`. Reporters with one status line fold it into the
    /// message; the install renderers show it beside the phase.
    fn set_detail(&self, detail: String) {
        let _ = detail;
    }

    /// Progress through the current operation in whole items rather than
    /// bytes — packages resolved, packages fetched. Drives the bar the same way
    /// `set_length`/`set_position` do, but is never formatted as a size or
    /// folded into a transfer rate.
    fn set_items(&self, done: u64, total: u64) {
        let _ = (done, total);
    }
    fn inc(&self, _delta: u64) {}
    fn set_position(&self, _delta: u64) {}
    fn set_length(&self, _length: u64) {}
    fn abandon(&self) {}
    fn finish(&self) {
        self.finish_with_message(String::new());
    }
    fn finish_with_message(&self, message: String) {
        self.finish_with_icon(message, ProgressIcon::Success);
    }
    fn finish_with_icon(&self, _message: String, _icon: ProgressIcon) {}

    /// Declare how many operations this progress report will have
    /// Each operation will get equal space (1/count)
    /// For example, if there are 3 operations (download, checksum, extract):
    /// - start_operations(3) at the beginning
    ///
    /// Then each set_length() call will allocate 33.33% of the total progress
    fn start_operations(&self, _count: usize) {}

    /// Declare the operations with a relative cost for each, in the order they
    /// run, when the backend can estimate them better than "all equal".
    ///
    /// This only paces a progress display. The numbers are estimates — a
    /// download's share of an install varies with the artifact and the network
    /// — so nothing may depend on them being right.
    fn start_operations_weighted(&self, weights: &[f64]) {
        self.start_operations(weights.len());
    }

    /// Advance to the next operation
    /// Call this before each new stage (after the first one)
    fn next_operation(&self) {}
}
