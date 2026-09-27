//! Hiding the console Windows hands a Scheduled Task's console program.
//!
//! Task Scheduler starts a console program in a console of its own, so the
//! history watcher would sit behind a terminal window for as long as it runs
//! — one the user can close, which kills the watcher.
//!
//! Nothing marks that invocation as the service: the watcher asks on every
//! start, and the console itself says whether anyone is there to read it.

#[cfg(not(windows))]
pub fn hide_if_unattended() {}

/// Hide this process's console window when the console was created for this
/// process alone, which is what Task Scheduler does and what no shell does.
///
/// A console with another process attached is one somebody is reading through
/// — a shell waiting on `mise dot watch` — and its window is left alone.
///
/// The console itself stays. Giving it up instead would leave this process
/// without one, and Windows hands a console program started by a process that
/// has no console a console of its own: every `git` the watcher runs would
/// then put a window on the desktop, which is the problem rather than a
/// smaller version of it. Keeping a hidden console means children inherit it
/// and stay hidden too, and the standard handles remain what they were.
#[cfg(windows)]
pub fn hide_if_unattended() {
    use windows_sys::Win32::System::Console::{GetConsoleProcessList, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    // SAFETY: no arguments. Null when this process has no console, and then
    // there is no window to hide.
    let window = unsafe { GetConsoleWindow() };
    if window.is_null() {
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
    // SAFETY: a console window handle this process is attached to. Failure
    // leaves the window up, which is the state this started in.
    unsafe { ShowWindow(window, SW_HIDE) };
}
