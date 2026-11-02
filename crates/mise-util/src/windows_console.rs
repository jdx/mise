//! Hiding the console Windows hands a Scheduled Task's console program.
//!
//! Task Scheduler starts a console program in a console of its own, so a
//! watcher can leave a terminal window open for as long as it runs. Closing
//! that window kills the watcher.
//!
//! Unattended commands check their console on every start rather than relying
//! on a service marker. An interactive shell shares its console with mise and
//! is left alone; only a private console is hidden or replaced.

use std::io;

#[cfg(not(windows))]
pub fn hide_if_unattended() -> io::Result<()> {
    Ok(())
}

// Match ALLOC_CONSOLE_OPTIONS from consoleapi3.h; windows-sys does not yet
// provide these bindings.
#[cfg(windows)]
#[repr(C)]
struct AllocationOptions {
    mode: i32,
    use_show_window: i32,
    show_window: u16,
}

#[cfg(windows)]
fn redirected_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::System::Console::{GetConsoleMode, WriteConsoleW};

    let mut mode = 0;
    // GetConsoleMode needs read access. A zero-character write also recognizes
    // write-only console output buffers without changing their contents.
    // SAFETY: Windows validates the borrowed handle; the buffers are valid,
    // the write length is zero, and the reserved argument is null.
    let console = unsafe {
        GetConsoleMode(handle, &mut mode) != 0
            || WriteConsoleW(
                handle,
                std::ptr::from_ref(&0_u16).cast(),
                0,
                &mut mode,
                std::ptr::null(),
            ) != 0
    };
    (!console).then_some(handle)
}

/// Hide a console created for this process alone, keeping an inheritable
/// console so that programs the service starts do not open their own windows.
///
/// A console with another process attached is one somebody is reading through,
/// such as a shell waiting on `mise dot watch`, and its window is left alone.
///
/// Giving up the console without replacing it would make ordinary console
/// children, including `git`, open windows of their own. A hidden or windowless
/// console lets them inherit it instead. File and pipe standard handles are
/// preserved; console handles invalidated by replacement are redirected to NUL.
///
/// Windows Terminal hosts a pseudoconsole: `GetConsoleWindow` names its
/// message window, not the visible terminal. On Windows 11 24H2 and later,
/// replace that private pseudoconsole with a windowless console instead.
/// Classic consoles, and Windows versions without that API, retain the
/// existing window-hiding behavior.
///
/// Changing consoles resets Windows control handlers. Call this before
/// registering signal listeners, including Tokio's process-wide handler.
#[cfg(windows)]
pub fn hide_if_unattended() -> io::Result<()> {
    use std::fs::OpenOptions;
    use std::os::windows::io::IntoRawHandle;

    use windows_sys::Win32::System::Console::{
        FreeConsole, GetConsoleCP, GetConsoleOutputCP, GetConsoleProcessList, GetConsoleWindow,
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleCP,
        SetConsoleOutputCP, SetStdHandle,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, SW_HIDE, ShowWindow};

    // SAFETY: no arguments. Null means this process has no console to hide.
    let window = unsafe { GetConsoleWindow() };
    // Only the count matters: the call returns the total even when this
    // two-entry buffer cannot hold every attached process.
    let mut attached = [0; 2];
    // SAFETY: attached is writable for the stated number of entries.
    if window.is_null() || unsafe { GetConsoleProcessList(attached.as_mut_ptr(), 2) } != 1 {
        return Ok(());
    }
    let mut class = [0; 64];
    // SAFETY: window is this process's console window; class is writable.
    let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    // Keep the API optional: importing it directly would prevent mise from
    // starting on Windows versions older than 11 24H2.
    // SAFETY: kernel32 remains loaded for the process's lifetime.
    let module = unsafe { GetModuleHandleW(windows_sys::core::w!("kernel32.dll")) };
    let allocate = if module.is_null() {
        None
    } else {
        // SAFETY: the module is live and the function name is NUL-terminated.
        unsafe { GetProcAddress(module, c"AllocConsoleWithOptions".as_ptr().cast()) }
    };
    let Some(allocate) = allocate.filter(|_| {
        class[..length as usize]
            .iter()
            .copied()
            .eq("PseudoConsoleWindow".encode_utf16())
    }) else {
        // Classic consoles and systems without the API retain the existing behavior.
        // SAFETY: window belongs to this process's private console. Failure
        // leaves the existing window visible.
        unsafe { ShowWindow(window, SW_HIDE) };
        return Ok(());
    };

    // A Terminal HWND is only a message window. Replace its console with an
    // inheritable windowless one, so ordinary children (including git) stay hidden.
    // SAFETY: the export has this documented signature and AllocationOptions C layout.
    type Allocate = unsafe extern "system" fn(*const AllocationOptions, *mut i32) -> i32;
    let allocate =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Allocate>(allocate) };
    // SAFETY: this process is still attached to its original console.
    let (input_cp, output_cp) = unsafe { (GetConsoleCP(), GetConsoleOutputCP()) };
    let handles = [STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, STD_INPUT_HANDLE].map(|id| {
        // Preserve files/pipes; only console handles are invalidated by FreeConsole.
        // SAFETY: id is one of the three standard-handle identifiers.
        let handle = unsafe { GetStdHandle(id) };
        (id, redirected_handle(handle))
    });
    // Open NUL before detaching so an open error leaves the original console
    // available for diagnostics.
    let null = handles
        .iter()
        .any(|(_, handle)| handle.is_none())
        .then(|| OpenOptions::new().read(true).write(true).open("NUL"))
        .transpose()?;
    // SAFETY: the sole-process guard excludes a shared shell's console.
    if unsafe { FreeConsole() } == 0 {
        return Err(io::Error::last_os_error());
    }
    let options = AllocationOptions {
        mode: 2, // ALLOC_CONSOLE_MODE_NO_WINDOW
        use_show_window: 0,
        show_window: 0,
    };
    let mut result = 0;
    // SAFETY: both buffers have the Windows API's documented layout.
    let status = unsafe { allocate(&options, &mut result) };
    // Standard handles live until process exit. Transfer NUL ownership before
    // installing any handle; restore redirected stderr first for error reporting.
    let null = null.map_or(std::ptr::null_mut(), IntoRawHandle::into_raw_handle);
    for (id, handle) in handles {
        // SAFETY: files/pipes remain valid, and NUL replaces old console handles.
        if unsafe { SetStdHandle(id, handle.unwrap_or(null)) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // ALLOC_CONSOLE_RESULT_NEW_CONSOLE is 1. Without a replacement console,
    // ordinary children would open windows again, so a failed allocation must
    // stop the service rather than continue detached.
    if status < 0 || result != 1 {
        return Err(io::Error::other(format!(
            "AllocConsoleWithOptions: HRESULT 0x{:08X}, result {result}",
            status as u32
        )));
    }
    // Allocation resets code pages to OEM defaults. Preserve the original encoding.
    // SAFETY: these code pages came from the original console.
    if unsafe { SetConsoleCP(input_cp) } == 0 || unsafe { SetConsoleOutputCP(output_cp) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(windows, test))]
mod tests {
    #[test]
    fn allocation_options_match_the_windows_abi() {
        use super::AllocationOptions;
        assert_eq!(std::mem::size_of::<AllocationOptions>(), 12);
        assert_eq!(std::mem::align_of::<AllocationOptions>(), 4);
        assert_eq!(std::mem::offset_of!(AllocationOptions, show_window), 8);
    }

    #[test]
    fn console_handles_are_not_redirected_handles() {
        use std::fs::OpenOptions;
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        use windows_sys::Win32::System::Console::{
            GetConsoleCP, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        };
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

        const CHILD: &str = "MISE_TEST_CONSOLE_HANDLES";
        if std::env::var_os(CHILD).is_none() {
            // A separate windowless console avoids depending on the test runner's
            // console or changing the console used by other tests.
            let name = concat!(
                module_path!(),
                "::console_handles_are_not_redirected_handles"
            )
            .split_once("::")
            .unwrap()
            .1;
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name])
                .env(CHILD, "1")
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        // SAFETY: no arguments; the child must have its own windowless console.
        assert_ne!(unsafe { GetConsoleCP() }, 0);
        for read in [false, true] {
            let console = OpenOptions::new()
                .read(read)
                .write(true)
                .open("CONOUT$")
                .unwrap();
            assert!(
                super::redirected_handle(console.as_raw_handle()).is_none(),
                "console output must be replaced even without read access"
            );
            let null = OpenOptions::new()
                .read(read)
                .write(true)
                .open("NUL")
                .unwrap();
            assert_eq!(
                super::redirected_handle(null.as_raw_handle()),
                Some(null.as_raw_handle())
            );
        }
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        assert_eq!(
            super::redirected_handle(file.as_raw_handle()),
            Some(file.as_raw_handle())
        );
        for id in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: these are the pipes captured by the parent test process.
            let pipe = unsafe { GetStdHandle(id) };
            assert_eq!(super::redirected_handle(pipe), Some(pipe));
        }
    }
}
