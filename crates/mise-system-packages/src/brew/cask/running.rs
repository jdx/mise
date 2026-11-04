use super::*;

/// Whether any process runs an executable inside `app`. Reads `ps -axo comm=`,
/// which reports each executable path as launched, so helpers nested under
/// `Contents/Frameworks` count. A failed listing reports not running.
#[cfg(target_os = "macos")]
pub(super) fn app_is_running(app: &Path) -> bool {
    use std::process::{Command, Stdio};

    match Command::new("ps")
        .args(["-axo", "comm="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => app_has_live_process(app, &output.stdout),
        Ok(output) => {
            debug!(
                "brew-cask: process listing for {} exited with {}",
                app.display(),
                output.status
            );
            false
        }
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

/// Matches one executable path per line against `app` by path component, so
/// `Foo.app` does not claim `Foo.app 2`. Lines that are not UTF-8 are ignored.
#[cfg(any(target_os = "macos", test))]
pub(super) fn app_has_live_process(app: &Path, ps_output: &[u8]) -> bool {
    ps_output
        .split(|byte| *byte == b'\n')
        .filter_map(|line| std::str::from_utf8(line.trim_ascii()).ok())
        .filter(|line| !line.is_empty())
        .any(|line| Path::new(line).starts_with(app))
}
