use std::sync::atomic::{AtomicBool, Ordering};

use console::Term;
use mise_util::cmd::CmdLineRunner;

static EXIT: AtomicBool = AtomicBool::new(true);
static SHOW_CURSOR: AtomicBool = AtomicBool::new(false);
// static HANDLERS: OnceCell<Vec<Box<dyn Fn() + Send + Sync + 'static>>> = OnceCell::new();

/// 128 + SIGINT, the conventional status for a process interrupted by Ctrl-C.
/// Shells use it to tell an interrupt from an ordinary failure.
const INTERRUPTED_EXIT_CODE: i32 = 130;

pub async fn exit_signal() -> i32 {
    if crate::in_tests() {
        return std::future::pending().await;
    }
    mise_util::cmd::track_sigint();
    loop {
        tokio::signal::ctrl_c().await.unwrap();
        if SHOW_CURSOR.load(Ordering::Relaxed) {
            let _ = Term::stderr().show_cursor();
        }
        // Record the first task-mode interrupt before signalling children so
        // their exit handlers can distinguish cancellation from task failure.
        let should_exit = EXIT.load(Ordering::Relaxed) || mise_util::cancel::mark();
        crate::run_interrupt_hook();
        CmdLineRunner::kill_all(nix::sys::signal::SIGINT);
        if should_exit {
            log::debug!("Ctrl-C pressed, exiting...");
            return INTERRUPTED_EXIT_CODE;
        }
    }
}

pub fn exit_on_ctrl_c(do_exit: bool) {
    if crate::in_tests() {
        return;
    }
    EXIT.store(do_exit, Ordering::Relaxed);
    mise_util::cancel::reset();
}

/// Returns true if ctrl-c has been received
pub fn is_cancelled() -> bool {
    if crate::in_tests() {
        return false;
    }
    mise_util::cancel::is_cancelled()
}

/// ensures cursor is displayed on ctrl-c
pub fn show_cursor_after_ctrl_c() {
    if crate::in_tests() {
        return;
    }
    SHOW_CURSOR.store(true, Ordering::Relaxed);
}
