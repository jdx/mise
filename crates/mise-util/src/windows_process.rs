//! Windows process helpers for preserving startup data supplied by a parent process.
//!
//! libuv stores its CRT file-descriptor table in `STARTUPINFO.lpReserved2`. The normal
//! `std::process::Command` path constructs a new `STARTUPINFO`, which is right for ordinary
//! launches but loses an inherited Node.js IPC descriptor on a shim dispatch.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::ptr;
use std::slice;

use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, GetStartupInfoW, INFINITE, PROCESS_INFORMATION,
    STARTUPINFOW, WaitForSingleObject,
};

unsafe extern "C" {
    #[link_name = "_close"]
    fn crt_close(fd: i32) -> i32;
}

const CRT_FOPEN: u8 = 0x01;
const CRT_FPIPE: u8 = 0x08;

struct InheritedNodeIpc {
    fd: i32,
}

/// Whether Node passed a usable IPC descriptor in the inherited CRT table.
///
/// libuv supplies a CRT table for every Node child, not just for IPC. Node identifies its IPC
/// entry separately with `NODE_CHANNEL_FD`, so checking both avoids replacing normal process
/// launching for ordinary Node spawns.
pub fn has_inherited_node_ipc() -> bool {
    inherited_node_ipc_fd().is_some()
}

fn inherited_node_ipc_fd() -> Option<i32> {
    inherited_node_ipc().map(|ipc| ipc.fd)
}

fn inherited_node_ipc() -> Option<InheritedNodeIpc> {
    let fd =
        std::env::var_os("NODE_CHANNEL_FD").and_then(|fd| fd.to_str()?.parse::<usize>().ok())?;
    unsafe {
        let mut startup_info: STARTUPINFOW = std::mem::zeroed();
        GetStartupInfoW(&mut startup_info);
        if startup_info.lpReserved2.is_null() {
            return None;
        }

        // libuv's table is: u32 count, `count` CRT flags, then `count` HANDLEs. The handle
        // entries use the parent process's pointer width, which may differ from mise's.
        // Validate every length against cbReserved2 before reading its IPC entry.
        let table = slice::from_raw_parts(
            startup_info.lpReserved2,
            usize::from(startup_info.cbReserved2),
        );
        inherited_node_ipc_fd_from_table(table, fd).map(|fd| InheritedNodeIpc { fd })
    }
}

fn inherited_node_ipc_fd_from_table(table: &[u8], fd: usize) -> Option<i32> {
    let count = usize::try_from(u32::from_ne_bytes(table.get(..4)?.try_into().ok()?)).ok()?;
    if count > 256 || fd >= count {
        return None;
    }
    let handle_width = crt_handle_width(table, count)?;

    let flags = table[4 + fd];
    if flags & (CRT_FOPEN | CRT_FPIPE) != (CRT_FOPEN | CRT_FPIPE) {
        return None;
    }
    let handle_offset = 4 + count + fd.checked_mul(handle_width)?;
    let handle = table.get(handle_offset..handle_offset + handle_width)?;
    let valid_handle = match handle_width {
        4 => {
            let handle = u32::from_ne_bytes(handle.try_into().ok()?);
            handle != 0 && handle != u32::MAX
        }
        8 => {
            let handle = u64::from_ne_bytes(handle.try_into().ok()?);
            handle != 0 && handle != u64::MAX
        }
        _ => unreachable!(),
    };
    if valid_handle {
        i32::try_from(fd).ok()
    } else {
        None
    }
}

fn crt_handle_width(table: &[u8], count: usize) -> Option<usize> {
    let handles_start = 4usize.checked_add(count)?;
    let handle_bytes = table.len().checked_sub(handles_start)?;
    let width = handle_bytes.checked_div(count)?;
    if !matches!(width, 4 | 8) || handle_bytes != count.checked_mul(width)? {
        return None;
    }
    Some(width)
}

/// Run a command while retaining the caller's `STARTUPINFO`, including libuv's CRT descriptor
/// table. Call this only when [`has_inherited_node_ipc`] is true; regular commands should keep
/// using the standard process runner.
pub fn status_with_inherited_node_ipc(
    program: &OsStr,
    args: impl IntoIterator<Item = OsString>,
) -> io::Result<ExitStatus> {
    let mut command_line = command_line(program, args)?;
    command_line.push(0);
    let mut application_name = wide_nul(program)?;
    let ipc = inherited_node_ipc();

    unsafe {
        let mut startup_info: STARTUPINFOW = std::mem::zeroed();
        GetStartupInfoW(&mut startup_info);
        let mut process_info: PROCESS_INFORMATION = std::mem::zeroed();
        if CreateProcessW(
            application_name.as_mut_ptr(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            0,
            ptr::null(),
            ptr::null(),
            &startup_info,
            &mut process_info,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }

        // The child now has its own inherited copy. Close through the CRT descriptor table, not
        // directly through CloseHandle: FOPEN means the CRT owns this handle and must retire its
        // entry too. A failed close is unexpected after validation. Keep waiting for the spawned
        // process before surfacing it: returning early would report a launch failure while the
        // child (and its inherited authority) continued unsupervised.
        let cleanup_error = if let Some(ipc) = ipc
            && crt_close(ipc.fd) == -1
        {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        // The process handle remains needed for the wait. The primary thread does not.
        let _ = CloseHandle(process_info.hThread);
        let wait = WaitForSingleObject(process_info.hProcess, INFINITE);
        if wait != WAIT_OBJECT_0 {
            let err = io::Error::last_os_error();
            let _ = CloseHandle(process_info.hProcess);
            return Err(err);
        }

        let mut code = 0;
        if GetExitCodeProcess(process_info.hProcess, &mut code) == 0 {
            let err = io::Error::last_os_error();
            let _ = CloseHandle(process_info.hProcess);
            return Err(err);
        }
        let _ = CloseHandle(process_info.hProcess);
        match cleanup_error {
            Some(err) => Err(err),
            None => Ok(ExitStatus::from_raw(code)),
        }
    }
}

fn wide_nul(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut wide: Vec<_> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "command arguments may not contain NUL bytes",
        ));
    }
    wide.push(0);
    Ok(wide)
}

fn command_line(program: &OsStr, args: impl IntoIterator<Item = OsString>) -> io::Result<Vec<u16>> {
    let mut command = Vec::new();
    // This is the same argv0 spelling std uses. Windows paths cannot contain `"`.
    command.push('"' as u16);
    append_wide(&mut command, program)?;
    command.push('"' as u16);
    for arg in args {
        command.push(' ' as u16);
        append_argument(&mut command, &arg)?;
    }
    Ok(command)
}

fn append_wide(command: &mut Vec<u16>, value: &OsStr) -> io::Result<()> {
    let wide: Vec<_> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "command arguments may not contain NUL bytes",
        ));
    }
    command.extend(wide);
    Ok(())
}

/// Quote an argument with the MSVCRT rules used by `std::process::Command`.
fn append_argument(command: &mut Vec<u16>, value: &OsStr) -> io::Result<()> {
    let wide: Vec<_> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "command arguments may not contain NUL bytes",
        ));
    }
    let quoted = wide.is_empty()
        || wide
            .iter()
            .any(|unit| *unit == u16::from(b' ') || *unit == u16::from(b'\t'));
    if quoted {
        command.push('"' as u16);
    }

    let mut backslashes = 0;
    for unit in wide {
        if unit == b'\\' as u16 {
            backslashes += 1;
        } else {
            if unit == b'"' as u16 {
                command.extend(std::iter::repeat_n(b'\\' as u16, backslashes + 1));
            }
            backslashes = 0;
        }
        command.push(unit);
    }
    if quoted {
        command.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
        command.push('"' as u16);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CRT_FOPEN, CRT_FPIPE, crt_handle_width, inherited_node_ipc_fd_from_table};

    #[test]
    fn detects_parent_crt_handle_width() {
        let count = 3;
        for width in [4, 8] {
            let table = vec![0; 4 + count + count * width];
            assert_eq!(crt_handle_width(&table, count), Some(width));
        }
    }

    #[test]
    fn accepts_valid_ipc_from_either_parent_handle_width() {
        let count = 3;
        let fd = 2;
        for width in [4, 8] {
            let mut table = vec![0; 4 + count + count * width];
            table[..4].copy_from_slice(&(count as u32).to_ne_bytes());
            table[4 + fd] = CRT_FOPEN | CRT_FPIPE;
            let handle = 4 + count + fd * width;
            table[handle..handle + width].copy_from_slice(&1u64.to_ne_bytes()[..width]);
            assert_eq!(
                inherited_node_ipc_fd_from_table(&table, fd),
                Some(fd as i32)
            );
        }
    }
}
