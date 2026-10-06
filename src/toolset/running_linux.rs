//! Reads running processes from `/proc`.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Listing, Process};

pub(super) fn list() -> Listing {
    let mut listing = Listing::default();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return listing;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse().ok()) else {
            continue;
        };
        let proc_dir = entry.path();
        let exe = match std::fs::read_link(proc_dir.join("exe")) {
            Ok(exe) => Some(exe),
            Err(err) => {
                if err.kind() == std::io::ErrorKind::PermissionDenied {
                    listing.uninspected += 1;
                }
                None
            }
        };
        let args = command_line_paths(&proc_dir);
        if exe.is_none() && args.is_empty() {
            continue;
        }
        listing.processes.push(Process {
            pid,
            name: process_name(&proc_dir),
            exe,
            args,
        });
    }
    listing
}

/// The absolute paths in a process's command line.
fn command_line_paths(proc_dir: &Path) -> Vec<PathBuf> {
    let Ok(cmdline) = std::fs::read(proc_dir.join("cmdline")) else {
        return vec![];
    };
    cmdline
        .split(|b| *b == 0)
        .filter(|arg| arg.first() == Some(&b'/'))
        .map(|arg| PathBuf::from(OsStr::from_bytes(arg)))
        .collect()
}

fn process_name(proc_dir: &Path) -> String {
    std::fs::read_to_string(proc_dir.join("comm"))
        .map(|comm| comm.trim_end().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Converts a process's start time, which `/proc` counts in clock ticks since
/// boot, to the wall-clock time links record their changes in.
pub(super) fn start_time(pid: u32) -> Option<SystemTime> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let boot: u64 = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    let ticks_per_second = nix::unistd::sysconf(nix::unistd::SysconfVar::CLK_TCK)
        .ok()
        .flatten()
        .and_then(|ticks| u64::try_from(ticks).ok())
        .filter(|ticks| *ticks > 0)?;
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name in field 2 may contain spaces and parentheses, so count
    // from the last `)`. The start time is field 22.
    let ticks: u64 = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()?;
    Some(
        UNIX_EPOCH
            + Duration::from_secs(boot)
            + Duration::from_millis(ticks * 1000 / ticks_per_second),
    )
}
