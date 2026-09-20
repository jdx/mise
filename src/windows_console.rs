//! Giving up the console Windows hands a Scheduled Task's console program.
//!
//! Task Scheduler starts a console program in a console of its own, so the
//! history watcher would sit behind a terminal window for as long as it runs
//! — one the user can close, which kills the watcher. The `history-watch`
//! service asks for that console to be given up; nothing else does, so a
//! watcher started by hand keeps the terminal it was started from.

/// The flag the service passes. Matched before the argument parser runs so
/// the window goes away as early as an invocation can make it.
pub(crate) const FLAG: &str = "--hide-console";

/// Whether these arguments ask for the console to be given up.
pub(crate) fn requested(args: &[String]) -> bool {
    args.iter().any(|arg| arg == FLAG)
}

#[cfg(not(windows))]
pub(crate) fn detach() {}

/// Detach from this process's console, which Windows destroys once no
/// process is attached to it. A console shared with the shell that started
/// mise keeps its window: only this process leaves.
#[cfg(windows)]
pub(crate) fn detach() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{
        FreeConsole, GetConsoleWindow, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        SetStdHandle,
    };

    // SAFETY: no arguments. Null when this process has no console, and then
    // there is nothing to give up.
    if unsafe { GetConsoleWindow() }.is_null() {
        return;
    }
    // Detaching invalidates the standard handles, and `println!` panics when
    // a write fails, so point them at `NUL` first: every later write then
    // succeeds and goes nowhere, which is where a service's output went
    // anyway. All three are opened before any is installed — a half-applied
    // redirection with the console still attached would leave the very panic
    // this avoids.
    let nul = || {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("NUL")
    };
    let (Ok(stdin), Ok(stdout), Ok(stderr)) = (nul(), nul(), nul()) else {
        return;
    };
    for (id, file) in [
        (STD_INPUT_HANDLE, stdin),
        (STD_OUTPUT_HANDLE, stdout),
        (STD_ERROR_HANDLE, stderr),
    ] {
        // Deliberately leaked: a standard handle outlives every owner in the
        // process, and closing it would strand the slot it was installed in.
        // SAFETY: the handle is open and stays open for the life of the process.
        unsafe { SetStdHandle(id, file.into_raw_handle()) };
    }
    // Console control events cannot reach a process with no console. The
    // watcher's Ctrl+C and Ctrl+Break handlers are for the terminal it was
    // started from; a service is ended by the service manager instead.
    // SAFETY: no arguments. A failure leaves the console attached, which is
    // the state this started in.
    unsafe { FreeConsole() };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_flag_asks_for_it() {
        assert!(requested(&[
            "mise".into(),
            "dot".into(),
            "watch".into(),
            FLAG.into()
        ]));
        assert!(!requested(&["mise".into(), "dot".into(), "watch".into()]));
        // a value that happens to read like the flag is not the flag
        assert!(!requested(&[
            "mise".into(),
            "dot".into(),
            "save".into(),
            format!("~/{FLAG}")
        ]));
    }
}
