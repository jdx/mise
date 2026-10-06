//! Reads running processes with `libproc` and `sysctl`.

use std::ffi::{OsStr, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nix::libc;

use super::{Listing, Process};

/// The longest path `proc_pidpath` returns.
const PROC_PIDPATHINFO_MAXSIZE: usize = 4 * libc::MAXPATHLEN as usize;

pub(super) fn list() -> Listing {
    let mut listing = Listing::default();
    for pid in pids() {
        let exe = exe_path(pid);
        if exe.is_none() {
            // Other users' processes, and zombies.
            listing.uninspected += 1;
        }
        let args = command_line_paths(pid);
        if exe.is_none() && args.is_empty() {
            continue;
        }
        listing.processes.push(Process {
            pid: pid as u32,
            name: process_name(pid, exe.as_deref()),
            exe,
            args,
        });
    }
    listing
}

fn pids() -> Vec<libc::pid_t> {
    // `proc_listallpids` takes a buffer size in bytes but returns a number of
    // pids. A null buffer asks for the number needed.
    // SAFETY: a null buffer is allowed.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let Ok(count) = usize::try_from(count) else {
        return vec![];
    };
    // Leave room for processes started since the count was read.
    let mut pids = vec![0 as libc::pid_t; count + 64];
    // SAFETY: the buffer holds `pids.len()` pids and the size passed is its length in bytes.
    let written = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast::<c_void>(),
            (pids.len() * size_of::<libc::pid_t>()) as libc::c_int,
        )
    };
    pids.truncate(usize::try_from(written).unwrap_or(0));
    pids.retain(|pid| *pid > 0);
    pids
}

fn exe_path(pid: libc::pid_t) -> Option<PathBuf> {
    let mut buf = vec![0u8; PROC_PIDPATHINFO_MAXSIZE];
    // SAFETY: the buffer is `buf.len()` bytes long.
    let len =
        unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast::<c_void>(), buf.len() as u32) };
    let len = usize::try_from(len).ok().filter(|len| *len > 0)?;
    Some(PathBuf::from(OsStr::from_bytes(&buf[..len])))
}

fn process_name(pid: libc::pid_t, exe: Option<&std::path::Path>) -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is `buf.len()` bytes long.
    let len = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast::<c_void>(), buf.len() as u32) };
    if let Ok(len) = usize::try_from(len)
        && len > 0
    {
        return String::from_utf8_lossy(&buf[..len]).into_owned();
    }
    exe.and_then(|exe| exe.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string())
}

/// The absolute paths in a process's command line, from `KERN_PROCARGS2`:
/// an `argc`, the executable path, padding NULs, then the NUL-separated
/// arguments followed by the environment.
fn command_line_paths(pid: libc::pid_t) -> Vec<PathBuf> {
    let Some(buf) = procargs2(pid) else {
        return vec![];
    };
    let Some((argc, rest)) = buf.split_first_chunk::<4>() else {
        return vec![];
    };
    let argc = i32::from_ne_bytes(*argc).max(0) as usize;
    let mut fields = rest.split(|b| *b == 0);
    // Skip the executable path and the padding after it.
    fields.next();
    fields
        .skip_while(|field| field.is_empty())
        .take(argc)
        .filter(|arg| arg.first() == Some(&b'/'))
        .map(|arg| PathBuf::from(OsStr::from_bytes(arg)))
        .collect()
}

fn procargs2(pid: libc::pid_t) -> Option<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size = 0usize;
    // SAFETY: a null buffer asks for the size needed.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: the buffer is `size` bytes long.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buf.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(size);
    Some(buf)
}

pub(super) fn start_time(pid: u32) -> Option<SystemTime> {
    // SAFETY: proc_bsdinfo is plain data, for which all zeroes is valid.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is `size` bytes long.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::pid_t,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::from_mut(&mut info).cast::<c_void>(),
            size,
        )
    };
    if written != size {
        return None;
    }
    Some(
        UNIX_EPOCH
            + Duration::from_secs(info.pbi_start_tvsec)
            + Duration::from_micros(info.pbi_start_tvusec),
    )
}
