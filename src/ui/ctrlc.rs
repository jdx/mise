use crate::exit;
use once_cell::sync::OnceCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crate::cmd::CmdLineRunner;
use console::Term;
use signal_hook::consts::SIGINT;
use signal_hook::iterator::{Handle, Signals};

static EXIT: AtomicBool = AtomicBool::new(true);
static SHOW_CURSOR: AtomicBool = AtomicBool::new(false);
// static HANDLERS: OnceCell<Vec<Box<dyn Fn() + Send + Sync + 'static>>> = OnceCell::new();

pub fn init() -> eyre::Result<()> {
    static HANDLE: OnceCell<Handle> = OnceCell::new();
    HANDLE.get_or_try_init(|| {
        let mut signals = Signals::new([SIGINT])?;
        let handle = signals.handle();
        thread::spawn(move || {
            if let Some(_signal) = signals.into_iter().next() {
                if SHOW_CURSOR.load(Ordering::Relaxed) {
                    let _ = Term::stderr().show_cursor();
                }
                // for handler in HANDLERS.get().unwrap_or(&Vec::new()) {
                //     handler();
                // }
                CmdLineRunner::kill_all(nix::sys::signal::SIGINT);
                if EXIT.swap(true, Ordering::Relaxed) {
                    debug!("Ctrl-C pressed, exiting...");
                    exit(1);
                } else {
                    warn!("Ctrl-C pressed, please wait for tasks to finish or press Ctrl-C again to force exit");
                }
            }
        });
        Ok::<_, eyre::Error>(handle)
    })?;
    Ok(())
}

// pub fn add_handler(func: impl Fn() + Send + Sync + 'static) {
//     let mut handlers = HANDLERS.get_or_init(Vec::new);
//     handlers.push(Box::new(func));
// }

pub fn exit_on_ctrl_c(do_exit: bool) {
    EXIT.store(do_exit, Ordering::Relaxed);
}

/// ensures cursor is displayed on ctrl-c
pub fn show_cursor_after_ctrl_c() {
    SHOW_CURSOR.store(true, Ordering::Relaxed);
}
