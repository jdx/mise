//! Giving up the console Windows hands a Scheduled Task's console program.
//!
//! Task Scheduler starts a console program in a console of its own, so the
//! history watcher would sit behind a terminal window for as long as it runs
//! — one the user can close, which kills the watcher.
//!
//! Nothing marks that invocation as the service: the watcher asks on every
//! start, and the console itself says whether anyone is there to read it.

#[cfg(not(windows))]
pub(crate) fn detach_if_unattended() {}

/// Give up this process's console when it was created for this process alone,
/// which is what Task Scheduler does and what no shell does.
///
/// A console with another process attached is one somebody is reading through
/// — a shell waiting on `mise dot watch`, or the `cmd.exe` a service that sets
/// `environment` runs through — and it is left alone. Detaching would in any
/// case free only this process, so the window would stay up either way.
///
/// Windows destroys a console once nothing is attached to it, so giving it up
/// invalidates whatever standard handles pointed at it. Those are replaced,
/// because `println!` panics when a write fails; a handle that was redirected
/// to a pipe or a file is somebody's output and is left connected.
#[cfg(windows)]
pub(crate) fn detach_if_unattended() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{
        FreeConsole, GetConsoleMode, GetConsoleProcessList, GetConsoleWindow, GetStdHandle,
        STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };

    // SAFETY: no arguments. Null when this process has no console, and then
    // there is nothing to give up.
    if unsafe { GetConsoleWindow() }.is_null() {
        return;
    }
    // Only the count matters, so this asks for the smallest list that can tell
    // one process from more: the call returns how many there are either way.
    let mut attached = [0u32; 2];
    // SAFETY: the buffer is valid for the length passed alongside it.
    let count = unsafe { GetConsoleProcessList(attached.as_mut_ptr(), 2) };
    if count != 1 {
        return;
    }
    // SAFETY: no borrowed pointers; `GetConsoleMode` reports whether a handle
    // is a console one, and fails harmlessly for every other kind.
    let is_console = |id: STD_HANDLE| unsafe {
        let mut mode = 0;
        GetConsoleMode(GetStdHandle(id), &mut mode) != 0
    };
    let doomed: Vec<STD_HANDLE> = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
        .into_iter()
        .filter(|id| is_console(*id))
        .collect();
    // Opened before any is installed: a half-applied redirection with the
    // console still attached would leave the very panic this avoids.
    let Ok(replacements) = doomed
        .iter()
        .map(|_| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("NUL")
        })
        .collect::<std::io::Result<Vec<_>>>()
    else {
        return;
    };
    for (id, file) in doomed.into_iter().zip(replacements) {
        // Deliberately leaked: a standard handle outlives every owner in the
        // process, and closing it would strand the slot it was installed in.
        // SAFETY: the handle is open and stays open for the life of the process.
        unsafe { SetStdHandle(id, file.into_raw_handle()) };
    }
    // Console control events cannot reach a process with no console. Ctrl+C
    // and Ctrl+Break come from a terminal this process is not in.
    // SAFETY: no arguments. A failure leaves the console attached, which is
    // the state this started in.
    unsafe { FreeConsole() };
}
