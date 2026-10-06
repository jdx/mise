//! Optional desktop notifications for sync conflicts that newly need a
//! decision (`settings.history.notify`, on by default). Best effort: the
//! notifier is prepared before it is dispatched, then runs and is reaped on a
//! worker thread. A missing desktop or tool is a debug line, and delivery never
//! holds up a capture or a sync.

use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "linux", all(test, target_os = "macos")))]
const LOGO: &[u8] = include_bytes!("../../../docs/public/apple-touch-icon.png");

#[cfg(target_os = "linux")]
fn logo() -> Option<std::path::PathBuf> {
    let path = crate::dirs::CACHE.join("notifications/mise.png");
    match cache_logo(&path) {
        Ok(()) => Some(path),
        Err(err) => {
            debug!("history: could not cache notification logo: {err}");
            None
        }
    }
}

#[cfg(any(target_os = "linux", all(test, target_os = "macos")))]
fn cache_logo(path: &std::path::Path) -> eyre::Result<()> {
    if std::fs::read(path).ok().as_deref() != Some(LOGO) {
        if let Some(parent) = path.parent() {
            crate::file::create_dir_all(parent)?;
        }
        crate::file::write_atomic(path, LOGO)?;
    }
    Ok(())
}

/// Shows a notification with `title` and `body`, if a notifier is available.
pub(crate) fn send(title: &str, body: &str) {
    let command = match notifier(title, body) {
        Ok(command) => command,
        Err(err) => {
            debug!("history: desktop notifier unavailable: {err:#}");
            return;
        }
    };
    if let Err(err) = dispatch(command) {
        debug!("history: could not dispatch notification: {err}");
    }
}

/// Why notifications can never be delivered by this build on this machine,
/// or `None` when a notifier exists. It cannot see macOS permission, which
/// is only learned by asking: see [`send_test`].
pub fn unavailable_reason() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        (!macos::release_signed()).then(|| "unofficial macOS build such as Homebrew".to_string())
    }
    #[cfg(target_os = "linux")]
    {
        crate::file::which_spawnable("notify-send")
            .is_none()
            .then(|| "`notify-send` not found".to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Some("not supported on this platform".to_string())
    }
}

/// One line on whether sync-conflict notifications can reach the user, for
/// `mise doctor` and `mise dot status`. Notifications only announce sync
/// conflicts that pause sharing; a watcher that cannot save is reported by
/// those commands instead.
pub fn summary() -> String {
    if !crate::config::Settings::get().history.notify {
        return "disabled (history.notify = false)".to_string();
    }
    match unavailable_reason() {
        Some(reason) => format!(
            "unavailable ({reason}); sync conflicts are only shown by `mise dot status` and `mise doctor`"
        ),
        None => "available for sync conflicts; confirm delivery with `mise dot notify`".to_string(),
    }
}

/// Sends a notification and waits for the notifier, so that a refusal can be
/// reported. On macOS the first call asks for permission, which can take up
/// to the helper's 30 second prompt timeout.
pub fn send_test() -> eyre::Result<()> {
    let mut command = notifier("mise", "Notifications are working.")?;
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    eyre::bail!("{}", failure_message(output.status.code(), stderr.trim()))
}

/// The macOS helper's exit codes 3 and 4 mean the permission prompt was
/// refused or unanswered; `notify-send` gives them no such meaning.
fn failure_message(code: Option<i32>, stderr: &str) -> String {
    let status = match code {
        Some(3) if cfg!(target_os = "macos") => {
            return "notification permission was denied. Allow notifications for mise in System Settings > Notifications".into();
        }
        Some(4) if cfg!(target_os = "macos") => {
            return "the notification permission prompt timed out. Run the command again and answer the prompt".into();
        }
        Some(code) => format!("the notifier exited with status {code}"),
        None => "the notifier was terminated by a signal".into(),
    };
    if stderr.is_empty() {
        status
    } else {
        format!("{status}: {stderr}")
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn warn_if_release_signing_unavailable() {
    if crate::config::Settings::get().history.notify && !macos::release_signed() {
        warn!(
            "macOS desktop notifications are unavailable in unofficial builds such as Homebrew; `mise dot status` and `mise doctor` still report setup conflicts"
        );
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn warn_if_release_signing_unavailable() {}

fn dispatch(
    mut command: Command,
) -> std::io::Result<std::thread::JoinHandle<std::io::Result<std::process::ExitStatus>>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
    let worker = std::thread::Builder::new()
        .name("mise-notification".into())
        .spawn(move || {
            // A short-lived `mise dot sync` may exit as soon as
            // dispatch returns. Confirm that the helper process exists first;
            // it can finish independently if this worker then goes away.
            let mut child = match command.spawn() {
                Ok(child) => {
                    let _ = started_tx.send(Ok(()));
                    child
                }
                Err(err) => {
                    let notice = std::io::Error::new(err.kind(), err.to_string());
                    let _ = started_tx.send(Err(notice));
                    return Err(err);
                }
            };
            let result = child.wait();
            match &result {
                Ok(status) if status.success() => debug!("history: notifier completed"),
                Ok(status) => debug!("history: notifier exited with {status}"),
                Err(err) => debug!("history: could not run notifier: {err}"),
            }
            result
        })?;
    match started_rx.recv() {
        Ok(Ok(())) => Ok(worker),
        Ok(Err(err)) => {
            let _ = worker.join();
            Err(err)
        }
        Err(err) => {
            let _ = worker.join();
            Err(std::io::Error::other(format!(
                "notification worker stopped before starting the helper: {err}"
            )))
        }
    }
}

#[cfg(target_os = "linux")]
fn notifier(title: &str, body: &str) -> eyre::Result<Command> {
    let Some(bin) = crate::file::which_spawnable("notify-send") else {
        eyre::bail!("`notify-send` not found");
    };
    Ok(linux_notification(&bin, title, body, logo().as_deref()))
}

#[cfg(any(target_os = "linux", all(test, target_os = "macos")))]
fn linux_notification(
    bin: &std::path::Path,
    title: &str,
    body: &str,
    icon: Option<&std::path::Path>,
) -> Command {
    let mut command = Command::new(bin);
    command.args(["--app-name", "mise", "--urgency", "normal"]);
    if let Some(icon) = icon {
        command.arg("--icon").arg(icon);
    }
    command.arg("--").arg(title).arg(body);
    command
}

#[cfg(target_os = "macos")]
fn notifier(title: &str, body: &str) -> eyre::Result<Command> {
    macos::notification(title, body)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn notifier(_title: &str, _body: &str) -> eyre::Result<Command> {
    eyre::bail!("not supported on this platform")
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn notification_worker_reaps_the_child_and_reports_spawn_errors() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 7"]);
        let status = dispatch(command).unwrap().join().unwrap().unwrap();
        assert_eq!(status.code(), Some(7));
        let missing = tempfile::tempdir().unwrap().path().join("missing-notifier");
        assert!(dispatch(Command::new(missing)).is_err());
    }

    #[test]
    fn failure_messages_name_the_permission_exit_codes_on_macos_only() {
        let denied = failure_message(Some(3), "");
        let timed_out = failure_message(Some(4), "");
        if cfg!(target_os = "macos") {
            assert!(denied.contains("denied"));
            assert!(timed_out.contains("timed out"));
        } else {
            assert!(denied.contains("status 3"));
            assert!(timed_out.contains("status 4"));
        }
        assert_eq!(
            failure_message(Some(9), ""),
            "the notifier exited with status 9"
        );
        assert!(failure_message(None, "").contains("signal"));
    }

    #[test]
    fn failure_messages_keep_the_notifier_explanation() {
        assert_eq!(
            failure_message(Some(1), "Cannot autolaunch D-Bus"),
            "the notifier exited with status 1: Cannot autolaunch D-Bus"
        );
    }

    #[test]
    fn notification_logo_is_cached_and_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notifications/mise.png");
        cache_logo(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), LOGO);
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        cache_logo(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        std::fs::write(&path, b"stale").unwrap();
        cache_logo(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), LOGO);
    }

    #[test]
    fn notification_logo_cache_failure_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        std::fs::write(&parent, b"").unwrap();
        assert!(cache_logo(&parent.join("mise.png")).is_err());
    }

    #[test]
    fn linux_notification_logo_is_optional_and_paths_are_literal() {
        let bin = std::path::Path::new("notify-send");
        let icon = std::path::Path::new("/cache with spaces/mise.png");
        let with_logo = linux_notification(bin, "--title", "body", Some(icon));
        assert_eq!(
            with_logo.get_args().collect::<Vec<_>>(),
            [
                "--app-name",
                "mise",
                "--urgency",
                "normal",
                "--icon",
                "/cache with spaces/mise.png",
                "--",
                "--title",
                "body"
            ]
        );
        let without_logo = linux_notification(bin, "--title", "body", None);
        assert_eq!(
            without_logo.get_args().collect::<Vec<_>>(),
            [
                "--app-name",
                "mise",
                "--urgency",
                "normal",
                "--",
                "--title",
                "body"
            ]
        );
    }
}
