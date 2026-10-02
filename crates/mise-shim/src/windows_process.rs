//! A minimal Windows process launcher for the native shim.
//!
//! This intentionally stays local instead of pulling mise-util into the tiny shipped shim. It
//! preserves the parent startup data that libuv uses for Node's extra IPC descriptor.

use std::cmp::Ordering;
use std::ffi::{OsStr, OsString, c_void};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::ptr;
use std::slice;

type Handle = *mut c_void;

#[repr(C)]
struct StartupInfoW {
    cb: u32,
    lp_reserved: *mut u16,
    lp_desktop: *mut u16,
    lp_title: *mut u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    lp_reserved2: *mut u8,
    h_std_input: Handle,
    h_std_output: Handle,
    h_std_error: Handle,
}

#[repr(C)]
struct ProcessInformation {
    h_process: Handle,
    h_thread: Handle,
    dw_process_id: u32,
    dw_thread_id: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "CloseHandle"]
    fn close_handle(object: Handle) -> i32;
    #[link_name = "CompareStringOrdinal"]
    fn compare_string_ordinal(
        string1: *const u16,
        count1: i32,
        string2: *const u16,
        count2: i32,
        ignore_case: i32,
    ) -> i32;
    #[link_name = "CreateProcessW"]
    fn create_process_w(
        application_name: *const u16,
        command_line: *mut u16,
        process_attributes: *const c_void,
        thread_attributes: *const c_void,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *const c_void,
        current_directory: *const u16,
        startup_info: *const StartupInfoW,
        process_information: *mut ProcessInformation,
    ) -> i32;
    #[link_name = "GetExitCodeProcess"]
    fn get_exit_code_process(process: Handle, exit_code: *mut u32) -> i32;
    #[link_name = "GetStartupInfoW"]
    fn get_startup_info_w(startup_info: *mut StartupInfoW);
    #[link_name = "GetSystemDirectoryW"]
    fn get_system_directory_w(buffer: *mut u16, size: u32) -> u32;
    #[link_name = "GetWindowsDirectoryW"]
    fn get_windows_directory_w(buffer: *mut u16, size: u32) -> u32;
    #[link_name = "WaitForSingleObject"]
    fn wait_for_single_object(handle: Handle, milliseconds: u32) -> u32;
}

unsafe extern "C" {
    #[link_name = "_close"]
    fn crt_close(fd: i32) -> i32;
}

const INFINITE: u32 = u32::MAX;
const WAIT_OBJECT_0: u32 = 0;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
const CSTR_LESS_THAN: i32 = 1;
const CSTR_EQUAL: i32 = 2;
const CSTR_GREATER_THAN: i32 = 3;
const CRT_FOPEN: u8 = 0x01;
const CRT_FPIPE: u8 = 0x08;

struct InheritedNodeIpc {
    fd: i32,
}

pub(super) fn has_inherited_node_ipc() -> bool {
    inherited_node_ipc_fd().is_some()
}

fn inherited_node_ipc_fd() -> Option<i32> {
    inherited_node_ipc().map(|ipc| ipc.fd)
}

fn inherited_node_ipc() -> Option<InheritedNodeIpc> {
    let fd =
        std::env::var_os("NODE_CHANNEL_FD").and_then(|fd| fd.to_str()?.parse::<usize>().ok())?;
    unsafe {
        let mut startup_info: StartupInfoW = std::mem::zeroed();
        get_startup_info_w(&mut startup_info);
        if startup_info.lp_reserved2.is_null() {
            return None;
        }

        // libuv's table is: u32 count, `count` CRT flags, then `count` HANDLEs. The handle
        // entries use the parent process's pointer width, which may differ from this shim's.
        // Validate every length against cbReserved2 before reading its IPC entry.
        let table = slice::from_raw_parts(
            startup_info.lp_reserved2,
            usize::from(startup_info.cb_reserved2),
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

pub(super) fn status_with_inherited_node_ipc_and_env(
    program: &OsStr,
    args: impl IntoIterator<Item = OsString>,
    env_key: &OsStr,
    env_value: &OsStr,
) -> io::Result<ExitStatus> {
    let mut command_line = command_line(program, args)?;
    command_line.push(0);
    let mut environment = environment_block(env_key, env_value)?;
    let application_name = resolve_application_name(program)?;
    let ipc = inherited_node_ipc();

    unsafe {
        let mut startup_info: StartupInfoW = std::mem::zeroed();
        get_startup_info_w(&mut startup_info);
        let mut process_info: ProcessInformation = std::mem::zeroed();
        if create_process_w(
            application_name.as_ptr(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            CREATE_UNICODE_ENVIRONMENT,
            environment.as_mut_ptr().cast(),
            ptr::null(),
            &startup_info,
            &mut process_info,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }

        // The child now owns a duplicate. Close through the CRT descriptor table, not directly
        // through CloseHandle: FOPEN means the CRT owns this handle and must retire its entry too.
        // A failed close is unexpected after the table validation. Keep waiting for the spawned
        // process before surfacing it: returning early would report a launch failure while the
        // child (and its inherited authority) continued unsupervised.
        let cleanup_error = if let Some(ipc) = ipc
            && crt_close(ipc.fd) == -1
        {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        let _ = close_handle(process_info.h_thread);
        let wait = wait_for_single_object(process_info.h_process, INFINITE);
        if wait != WAIT_OBJECT_0 {
            let err = io::Error::last_os_error();
            let _ = close_handle(process_info.h_process);
            return Err(err);
        }
        let mut code = 0;
        if get_exit_code_process(process_info.h_process, &mut code) == 0 {
            let err = io::Error::last_os_error();
            let _ = close_handle(process_info.h_process);
            return Err(err);
        }
        let _ = close_handle(process_info.h_process);
        match cleanup_error {
            Some(err) => Err(err),
            None => Ok(ExitStatus::from_raw(code)),
        }
    }
}

/// Resolve the shim's `mise` executable with the same no-current-directory search order that
/// `std::process::Command` uses for a bare executable name.
fn resolve_application_name(program: &OsStr) -> io::Result<Vec<u16>> {
    let path = Path::new(program);
    let resolved = if path.components().count() == 1 {
        let mut executable = PathBuf::from(program);
        if executable.extension().is_none() {
            executable.set_extension("exe");
        }
        let mut search_paths = Vec::new();
        if let Ok(mut current_exe) = std::env::current_exe() {
            current_exe.pop();
            search_paths.push(current_exe);
        }
        if let Some(path) = system_directory(get_system_directory_w) {
            search_paths.push(path);
        }
        if let Some(path) = system_directory(get_windows_directory_w) {
            search_paths.push(path);
        }
        if let Some(paths) = std::env::var_os("PATH") {
            search_paths
                .extend(std::env::split_paths(&paths).filter(|path| !path.as_os_str().is_empty()));
        }
        search_paths
            .into_iter()
            .map(|path| path.join(&executable))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "program not found"))?
    } else if path.extension().is_none() {
        let with_exe = path.with_extension("exe");
        if with_exe.is_file() {
            with_exe
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };
    wide_nul(resolved.as_os_str())
}

fn system_directory(
    get_directory: unsafe extern "system" fn(*mut u16, u32) -> u32,
) -> Option<PathBuf> {
    let mut buffer = vec![0; 32_768];
    let length = unsafe { get_directory(buffer.as_mut_ptr(), buffer.len() as u32) };
    (length != 0 && usize::try_from(length).ok()? < buffer.len())
        .then(|| PathBuf::from(OsString::from_wide(&buffer[..length as usize])))
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

/// Build a Unicode environment block with one child-only environment override.
///
/// `CreateProcessW` inherits the current environment only when its environment pointer is null.
/// The shim needs to add `__MISE_SHIM_PATH` without calling `set_var`, which would race with
/// other threads in this process. Start with `vars_os` (including Windows' `=C:` entries), replace
/// an existing spelling of the key case-insensitively, and sort as required by `CreateProcessW`.
fn environment_block(key: &OsStr, value: &OsStr) -> io::Result<Vec<u16>> {
    let mut vars: Vec<_> = std::env::vars_os().collect();
    vars.retain(|(existing, _)| !same_env_key(existing, key));
    vars.push((key.to_os_string(), value.to_os_string()));
    vars.sort_by(|(a, _), (b, _)| compare_environment_keys(a, b));

    let mut block = Vec::new();
    for (key, value) in vars {
        append_wide(&mut block, &key)?;
        block.push('=' as u16);
        append_wide(&mut block, &value)?;
        block.push(0);
    }
    // A Unicode environment block ends with an additional NUL after the last entry.
    block.push(0);
    Ok(block)
}

fn same_env_key(a: &OsStr, b: &OsStr) -> bool {
    let a: Vec<_> = a.encode_wide().collect();
    let b: Vec<_> = b.encode_wide().collect();
    a.len() == b.len()
        && unsafe {
            // The slices remain live for the call, their lengths fit in i32 because Windows
            // environment strings are bounded, and CompareStringOrdinal is read-only.
            compare_string_ordinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1)
                == CSTR_EQUAL
        }
}

fn compare_environment_keys(a: &OsStr, b: &OsStr) -> Ordering {
    let a: Vec<_> = a.encode_wide().collect();
    let b: Vec<_> = b.encode_wide().collect();
    match unsafe {
        // The slices remain live for the call, their lengths fit in i32 because Windows
        // environment strings are bounded, and CompareStringOrdinal is read-only.
        compare_string_ordinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1)
    } {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_EQUAL => Ordering::Equal,
        CSTR_GREATER_THAN => Ordering::Greater,
        result => panic!("CompareStringOrdinal failed while sorting environment: {result}"),
    }
}

fn command_line(program: &OsStr, args: impl IntoIterator<Item = OsString>) -> io::Result<Vec<u16>> {
    let mut command = Vec::new();
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
        if unit == u16::from(b'\\') {
            backslashes += 1;
        } else {
            if unit == u16::from(b'"') {
                command.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes + 1));
            }
            backslashes = 0;
        }
        command.push(unit);
    }
    if quoted {
        command.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
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
