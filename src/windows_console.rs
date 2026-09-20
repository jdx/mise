//! Giving up the console Windows hands a Scheduled Task's console program.
//!
//! Task Scheduler starts a console program in a console of its own, so the
//! history watcher would sit behind a terminal window for as long as it runs
//! — one the user can close, which kills the watcher. The `history-watch`
//! service asks for that console to be given up; nothing else does, so a
//! watcher started by hand keeps the terminal it was started from.

/// The flag the service passes.
pub(crate) const FLAG: &str = "--hide-console";

/// Whether these arguments are the watcher asking for its console to be given
/// up, read before the parser runs so the window goes as early as it can.
///
/// The flag alone is not enough to go on here: at this point nothing has
/// decided which command is running, and mise passes arguments through to
/// tasks and child processes. `mise run build -- --hide-console` must reach
/// the task with its terminal intact, so this matches the shape the service
/// registers — `watch` under `dotfiles`, introduced by mise or by
/// `mise bootstrap` — and nothing else. Being wrong in the other direction
/// only costs the head start: `dot watch` gives the console up again once the
/// flag has been parsed.
pub(crate) fn requested(args: &[String]) -> bool {
    let Some(flag) = args.iter().position(|arg| arg == FLAG) else {
        return false;
    };
    // everything past `--` is somebody else's argument, whatever it looks like
    if args[..flag].iter().any(|arg| arg == "--") {
        return false;
    }
    let (Some(watch), Some(group)) = (flag.checked_sub(1), flag.checked_sub(2)) else {
        return false;
    };
    if args[watch] != "watch" || !matches!(args[group].as_str(), "dot" | "dotfiles") {
        return false;
    }
    // `mise dot watch` or `mise bootstrap dotfiles watch`, never the tasks a
    // `mise run dot watch` would name
    group == 1 || args.get(group - 1).is_some_and(|arg| arg == "bootstrap")
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

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn the_watcher_asks_for_it() {
        // what the service registers, and the same command by its other names
        assert!(requested(&args(&format!("mise dot watch {FLAG}"))));
        assert!(requested(&args(&format!("mise dotfiles watch {FLAG}"))));
        assert!(requested(&args(&format!(
            "mise bootstrap dotfiles watch {FLAG}"
        ))));
    }

    #[test]
    fn nothing_else_does() {
        assert!(!requested(&args("mise dot watch")));
        // a task's arguments are not mise's, however they are spelled
        assert!(!requested(&args(&format!("mise run build -- {FLAG}"))));
        assert!(!requested(&args(&format!("mise run build {FLAG}"))));
        // tasks named `dot` and `watch` are not the `dot watch` command
        assert!(!requested(&args(&format!("mise run dot watch {FLAG}"))));
        // a value that happens to read like the flag is not the flag
        assert!(!requested(&args(&format!("mise dot save ~/{FLAG}"))));
    }
}
