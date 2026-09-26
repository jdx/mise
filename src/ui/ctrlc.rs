use std::sync::atomic::{AtomicBool, Ordering};

use crate::cmd::CmdLineRunner;
use console::Term;

static EXIT: AtomicBool = AtomicBool::new(true);
static SHOW_CURSOR: AtomicBool = AtomicBool::new(false);
// static HANDLERS: OnceCell<Vec<Box<dyn Fn() + Send + Sync + 'static>>> = OnceCell::new();

pub(crate) async fn exit_signal() -> i32 {
    loop {
        tokio::signal::ctrl_c().await.unwrap();
        if SHOW_CURSOR.load(Ordering::Relaxed) {
            let _ = Term::stderr().show_cursor();
        }
        // Record the first task-mode interrupt before signalling children so
        // their exit handlers can distinguish cancellation from task failure.
        let should_exit = EXIT.load(Ordering::Relaxed) || mise_util::cancel::mark();
        vfox::cancel_http_requests();
        CmdLineRunner::kill_all(nix::sys::signal::SIGINT);
        if should_exit {
            debug!("Ctrl-C pressed, exiting...");
            return 1;
        }
    }
}

pub(crate) fn exit_on_ctrl_c(do_exit: bool) {
    EXIT.store(do_exit, Ordering::Relaxed);
    mise_util::cancel::reset();
}

/// Returns true if ctrl-c has been received
pub(crate) fn is_cancelled() -> bool {
    mise_util::cancel::is_cancelled()
}

/// ensures cursor is displayed on ctrl-c
pub(crate) fn show_cursor_after_ctrl_c() {
    SHOW_CURSOR.store(true, Ordering::Relaxed);
}
