//! Whether the user has interrupted mise with Ctrl-C. mise's signal handlers set
//! it; code that loops or retries checks it to stop early.

use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Whether Ctrl-C has been received since the last [`reset`].
pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

/// Record an interrupt, returning whether one had already been recorded.
pub fn mark() -> bool {
    CANCELLED.swap(true, Ordering::Relaxed)
}

/// Forget any recorded interrupt.
pub fn reset() {
    CANCELLED.store(false, Ordering::Relaxed);
}
