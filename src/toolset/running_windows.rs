//! Reads running processes with the Toolhelp and process APIs.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Wdk::System::Threading::NtQueryInformationProcess;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, FILETIME, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

use super::{Listing, Process};

/// `PROCESSINFOCLASS::ProcessCommandLineInformation`, which Windows 8.1 and
/// later answer to a handle opened with `PROCESS_QUERY_LIMITED_INFORMATION`.
const PROCESS_COMMAND_LINE_INFORMATION: i32 = 60;

/// Seconds between 1601-01-01, which `FILETIME` counts from, and 1970-01-01.
const FILETIME_UNIX_EPOCH_SECS: u64 = 11_644_473_600;

struct Handle(HANDLE);

impl Handle {
    fn open(pid: u32) -> Option<Self> {
        // SAFETY: plain call; a null handle means failure.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenProcess or snapshot.
        unsafe { CloseHandle(self.0) };
    }
}

pub(super) fn list() -> Listing {
    let mut listing = Listing::default();
    // SAFETY: plain call.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return listing;
    }
    let snapshot = Handle(snapshot);
    // SAFETY: PROCESSENTRY32W is plain data, for which all zeroes is valid.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: `entry.dwSize` is set.
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while more {
        let pid = entry.th32ProcessID;
        let name = wide_to_string(&entry.szExeFile);
        // SAFETY: `entry.dwSize` is still set.
        more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        // The idle and system processes have no image.
        if pid == 0 || pid == 4 {
            continue;
        }
        let Some(handle) = Handle::open(pid) else {
            // SAFETY: plain call.
            if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
                listing.uninspected += 1;
            }
            continue;
        };
        let exe = image_path(&handle);
        let args = command_line(&handle)
            .map(|line| absolute_paths(&line))
            .unwrap_or_default();
        if exe.is_none() && args.is_empty() {
            continue;
        }
        listing.processes.push(Process {
            pid,
            name,
            exe,
            args,
        });
    }
    listing
}

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|c| *c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}

fn image_path(handle: &Handle) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32768];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` UTF-16 units.
    let ok = unsafe { QueryFullProcessImageNameW(handle.0, 0, buf.as_mut_ptr(), &mut len) };
    (ok != 0).then(|| PathBuf::from(OsString::from_wide(&buf[..len as usize])))
}

/// The command line of the process, as the string it was started with.
fn command_line(handle: &Handle) -> Option<String> {
    // The answer is a UNICODE_STRING followed by the characters it points at.
    let mut buf = vec![0u8; 64 * 1024];
    let mut needed = 0u32;
    // SAFETY: `buf` is `buf.len()` bytes long.
    let status = unsafe {
        NtQueryInformationProcess(
            handle.0,
            PROCESS_COMMAND_LINE_INFORMATION,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut needed,
        )
    };
    if status < 0 {
        return None;
    }
    // UNICODE_STRING { Length: u16, MaximumLength: u16, Buffer: *u16 }, with
    // Buffer pointing into `buf` after the header.
    let length = u16::from_ne_bytes([buf[0], buf[1]]) as usize / 2;
    let data = buf.as_ptr() as usize;
    let pointer_at = size_of::<usize>();
    let pointer = usize::from_ne_bytes(
        buf[pointer_at..pointer_at + size_of::<usize>()]
            .try_into()
            .ok()?,
    );
    let offset = pointer.checked_sub(data)?;
    let bytes = buf.get(offset..offset + length * 2)?;
    let wide = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_ne_bytes(*c))
        .collect::<Vec<_>>();
    Some(String::from_utf16_lossy(&wide))
}

/// The absolute paths among the arguments of a Windows command line, split by
/// the rules `CommandLineToArgvW` uses: double quotes group words, `2n`
/// backslashes before a quote are `n` backslashes and toggle the quote, and
/// `2n+1` backslashes before a quote are `n` backslashes and a literal quote.
/// Backslashes anywhere else are literal.
pub(super) fn absolute_paths(line: &str) -> Vec<PathBuf> {
    let mut args = vec![];
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let mut backslashes = 1;
                while chars.next_if_eq(&'\\').is_some() {
                    backslashes += 1;
                }
                any = true;
                if chars.next_if_eq(&'"').is_some() {
                    current.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if backslashes % 2 == 1 {
                        current.push('"');
                    } else {
                        quoted = !quoted;
                    }
                } else {
                    current.extend(std::iter::repeat_n('\\', backslashes));
                }
            }
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    args.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            c => {
                current.push(c);
                any = true;
            }
        }
    }
    if any {
        args.push(current);
    }
    args.into_iter()
        .map(PathBuf::from)
        .filter(|arg| arg.is_absolute())
        .collect()
}

pub(super) fn start_time(pid: u32) -> Option<SystemTime> {
    let handle = Handle::open(pid)?;
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: all four out-pointers are valid.
    let ok =
        unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }
    let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    let since_1601 = Duration::from_nanos(ticks * 100);
    since_1601
        .checked_sub(Duration::from_secs(FILETIME_UNIX_EPOCH_SECS))
        .map(|since_epoch| UNIX_EPOCH + since_epoch)
}
