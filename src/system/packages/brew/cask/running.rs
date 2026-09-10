//! Detects whether an installed app bundle currently has live processes.
//!
//! Replacing a bundle under a running app is only safe when nothing will ask
//! the old bundle for more code. Chrome, Electron apps, and anything else that
//! spawns helpers on demand exec them from the bundle path after launch, so a
//! swap leaves every later helper pointing at files that no longer exist. A
//! self-updating app already knows how to move itself between versions without
//! that failure, which is why a live one is left alone.

use std::path::Path;

/// Whether any process on this machine runs an executable inside `app`.
///
/// Reads `ps -axo comm=`, which lists each process's executable path as it was
/// launched. Bundles started through LaunchServices, `open`, or an absolute
/// path all report a path inside the bundle, and so do helpers nested under
/// `Contents/Frameworks`. A listing failure counts as "not running" so a broken
/// `ps` degrades to the previous behaviour instead of blocking every upgrade.
#[cfg(target_os = "macos")]
pub(super) fn app_is_running(app: &Path) -> bool {
    use std::process::{Command, Stdio};

    match Command::new("ps")
        .args(["-axo", "comm="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) => app_has_live_process(app, &output.stdout),
        Err(err) => {
            debug!(
                "brew-cask: could not list processes for {}: {err:#}",
                app.display()
            );
            false
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn app_is_running(_app: &Path) -> bool {
    false
}

/// Matches `ps -axo comm=` output, one executable path per line, against `app`.
///
/// The comparison is by path component, so `Foo.app` does not claim processes
/// from `Foo.app 2` or `Foobar.app`.
pub(super) fn app_has_live_process(app: &Path, ps_output: &[u8]) -> bool {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    ps_output
        .split(|byte| *byte == b'\n')
        .map(<[u8]>::trim_ascii)
        .filter(|line| !line.is_empty())
        .any(|line| Path::new(OsStr::from_bytes(line)).starts_with(app))
}
