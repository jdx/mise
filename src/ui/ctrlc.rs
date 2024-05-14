use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use console::Term;
use signal_hook::consts::SIGINT;
use signal_hook::iterator::{Handle, Signals};

#[must_use]
#[derive(Debug)]
pub struct HandleGuard(Handle);

/// ensures cursor is displayed on ctrl-c
pub fn handle_ctrlc() -> eyre::Result<Option<HandleGuard>> {
    static HANDLED: AtomicBool = AtomicBool::new(false);
    let handled = HANDLED.swap(true, Ordering::Relaxed);
    if handled {
        return Ok(None);
    }

    let mut signals = Signals::new([SIGINT])?;
    let handle = HandleGuard(signals.handle());
    thread::spawn(move || {
        if signals.into_iter().next().is_some() {
            let _ = Term::stderr().show_cursor();
            debug!("Ctrl-C pressed, exiting...");
            exit(1);
        }
        HANDLED.store(false, Ordering::Relaxed);
    });
    Ok(Some(handle))
}

impl Drop for HandleGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}
