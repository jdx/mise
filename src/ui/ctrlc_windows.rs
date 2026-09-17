//! Windows console control handling.
//!
//! Unix gets supervised interruption for free: `tokio::signal::ctrl_c` installs
//! a SIGINT handler, so mise stays alive and decides how its children are torn
//! down. Windows delivers `CTRL_C_EVENT` to every process attached to the
//! console, and with no handler installed the default one ends mise
//! immediately. Anything that does not die on its own is then left holding the
//! console after mise is gone — most visibly `cmd.exe` running a batch file,
//! which stops to ask `Terminate batch job (Y/N)?` and keeps reading stdin
//! while the shell prompt is already back (discussion #13215).
//!
//! The handler here keeps mise alive through the event so the usual shutdown
//! path runs. It is installed by [`exit_on_ctrl_c`], so only commands that opt
//! into mise-supervised interruption change behavior.

use std::sync::LazyLock as Lazy;
use std::sync::atomic::{AtomicBool, Ordering};

use console::Term;
use tokio::sync::Notify;
use windows_sys::Win32::Foundation::{FALSE, TRUE};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};
use windows_sys::core::BOOL;

use crate::cmd::CmdLineRunner;

static EXIT: AtomicBool = AtomicBool::new(true);
static SHOW_CURSOR: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Woken by the console control handler, which runs on a thread of its own and
/// so cannot await anything itself. `notify_one` keeps a permit when nothing is
/// waiting yet, so an event that arrives before [`exit_signal`] is polled is
/// still delivered.
static INTERRUPTED: Lazy<Notify> = Lazy::new(Notify::new);

/// Runs on a thread the OS creates for the event, so it cannot await anything
/// and must return promptly. It only touches atomics and
/// [`Notify::notify_one`], neither of which can block on a lock the rest of
/// mise might be holding; [`exit_signal`] does the actual work.
unsafe extern "system" fn handler(ctrl_type: u32) -> BOOL {
    match ctrl_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => {
            // Recorded before anything is torn down so a child's exit is
            // reported as cancellation rather than as a task failure.
            if EXIT.load(Ordering::Relaxed) || CANCELLED.swap(true, Ordering::Relaxed) {
                SHOULD_EXIT.store(true, Ordering::Relaxed);
            }
            INTERRUPTED.notify_one();
            // Claim the event: the default handler would end mise here, before
            // it could clean up after its children.
            TRUE
        }
        // Closing the window, logging off and shutting down keep their default
        // behavior; the OS gives too little time there to shut down in order.
        _ => FALSE,
    }
}

fn install_handler() {
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    // Built here rather than on first use, which would otherwise be inside the
    // handler thread while the console waits on it.
    Lazy::force(&INTERRUPTED);
    // SAFETY: `handler` is a valid handler function for the lifetime of the
    // process and is never removed.
    if unsafe { SetConsoleCtrlHandler(Some(handler), TRUE) } == FALSE {
        // Without a console there is nothing to interrupt, so this is not worth
        // failing a command over.
        debug!(
            "failed to install console ctrl handler: {}",
            std::io::Error::last_os_error()
        );
    }
}

pub(crate) async fn exit_signal() -> i32 {
    loop {
        INTERRUPTED.notified().await;
        if SHOW_CURSOR.load(Ordering::Relaxed) {
            let _ = Term::stderr().show_cursor();
        }
        vfox::cancel_http_requests();
        if SHOULD_EXIT.load(Ordering::Relaxed) {
            debug!("Ctrl-C pressed, exiting...");
            // Unlike Unix, where the pgroup got the SIGINT that reached mise,
            // nothing is guaranteed to have ended the tree: a console event is
            // advisory and `cmd.exe` in particular stops to ask about it.
            CmdLineRunner::kill_all();
            return 1;
        }
        // The console delivered the event to every child as well, so give them
        // the chance to stop on their own before anything is forced.
        info!(
            "interrupted, waiting for running commands to exit (press Ctrl-C again to stop them)"
        );
    }
}

pub(crate) fn exit_on_ctrl_c(do_exit: bool) {
    EXIT.store(do_exit, Ordering::Relaxed);
    CANCELLED.store(false, Ordering::Relaxed);
    install_handler();
}

/// Returns true if ctrl-c has been received
pub(crate) fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

/// ensures cursor is displayed on ctrl-c
pub(crate) fn show_cursor_after_ctrl_c() {
    SHOW_CURSOR.store(true, Ordering::Relaxed);
}
