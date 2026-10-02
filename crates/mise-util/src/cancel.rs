//! Whether the user has interrupted mise with Ctrl-C. mise's signal handlers set
//! it; code that loops or retries checks it to stop early.

use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);
static SIGNALLED: AtomicBool = AtomicBool::new(false);

/// Whether Ctrl-C has been received since the last [`reset`].
pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed) || SIGNALLED.load(Ordering::Relaxed)
}

/// Note that a SIGINT has arrived, straight from the signal handler.
///
/// The async Ctrl-C handler only runs once the runtime gets to it, by which
/// time a task that took the same SIGINT may already have exited and be
/// reported as a failure. This makes [`is_cancelled`] true right away without
/// touching [`mark`], whose result tells a second Ctrl-C from the first. It
/// only stores to an atomic, so it is async-signal-safe.
pub fn note_signal() {
    SIGNALLED.store(true, Ordering::Relaxed);
}

/// Record an interrupt, returning whether one had already been recorded.
pub fn mark() -> bool {
    CANCELLED.swap(true, Ordering::Relaxed)
}

/// Forget any recorded interrupt.
pub fn reset() {
    CANCELLED.store(false, Ordering::Relaxed);
    SIGNALLED.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_signal_cancels_without_counting_as_a_second_interrupt() {
        reset();
        assert!(!is_cancelled());
        note_signal();
        assert!(is_cancelled());
        // The async handler's first `mark` must still see a first Ctrl-C.
        assert!(!mark());
        assert!(mark());
        reset();
        assert!(!is_cancelled());
    }
}
