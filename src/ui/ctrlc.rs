use crate::exit;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::cmd::CmdLineRunner;
use console::Term;

static EXIT: AtomicBool = AtomicBool::new(true);
static SHOW_CURSOR: AtomicBool = AtomicBool::new(false);
// static HANDLERS: OnceCell<Vec<Box<dyn Fn() + Send + Sync + 'static>>> = OnceCell::new();

pub fn init() {
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.unwrap();
            if SHOW_CURSOR.load(Ordering::Relaxed) {
                let _ = Term::stderr().show_cursor();
            }
            CmdLineRunner::kill_all(nix::sys::signal::SIGINT);
            if EXIT.load(Ordering::Relaxed) {
                debug!("Ctrl-C pressed, exiting...");
                exit(1);
            }
        }
    });
}

pub fn exit_on_ctrl_c(do_exit: bool) {
    EXIT.store(do_exit, Ordering::Relaxed);
}

/// ensures cursor is displayed on ctrl-c
pub fn show_cursor_after_ctrl_c() {
    SHOW_CURSOR.store(true, Ordering::Relaxed);
}
