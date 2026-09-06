//! Optional desktop notifications for sync conflicts that newly need a
//! decision (`settings.history.notify`, on by default). Best effort: the
//! notifier runs and is reaped on a worker thread, a missing desktop or tool is a
//! debug line, and nothing here ever holds up a capture or a sync.

use std::process::{Command, Stdio};

#[cfg(any(target_os = "linux", target_os = "macos"))]
const LOGO: &[u8] = include_bytes!("../../../docs/public/apple-touch-icon.png");

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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
    let mut command = match notifier(title, body) {
        Some(command) => command,
        None => {
            debug!("history: no desktop notifier on this platform; not notifying: {title}");
            return;
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match dispatch(command) {
        Ok(_worker) => debug!("history: notification dispatched: {title}"),
        Err(err) => debug!("history: could not notify ({title}): {err}"),
    }
}

fn dispatch(
    mut command: Command,
) -> std::io::Result<std::thread::JoinHandle<std::io::Result<std::process::ExitStatus>>> {
    // Start the process inside the thread, so a thread-creation failure cannot
    // strand an unreaped child. Waiting here never blocks the watcher.
    std::thread::Builder::new()
        .name("mise-notification".into())
        .spawn(move || {
            let result = command.status();
            match &result {
                Ok(status) if status.success() => debug!("history: notifier completed"),
                Ok(status) => debug!("history: notifier exited with {status}"),
                Err(err) => debug!("history: could not run notifier: {err}"),
            }
            result
        })
}

#[cfg(target_os = "linux")]
fn notifier(title: &str, body: &str) -> Option<Command> {
    let bin = crate::file::which_spawnable("notify-send")?;
    Some(linux_notification(&bin, title, body, logo().as_deref()))
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
fn notifier(title: &str, body: &str) -> Option<Command> {
    if let Some(bin) = crate::file::which_spawnable("terminal-notifier") {
        let mut command = terminal_notification(&bin, title, body);
        if let Some(icon) = logo() {
            // Modern macOS fixes the sender icon to its application bundle;
            // contentImage displays the logo without impersonating another app.
            command.arg("-contentImage").arg(icon);
        }
        return Some(command);
    }
    let bin = crate::file::which_spawnable("osascript")?;
    let escape = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    let mut command = Command::new(bin);
    command.arg("-e").arg(format!(
        "display notification \"{}\" with title \"{}\"",
        escape(body),
        escape(title)
    ));
    Some(command)
}

#[cfg(target_os = "macos")]
fn terminal_notification(bin: &std::path::Path, title: &str, body: &str) -> Command {
    let mut command = Command::new(bin);
    // terminal-notifier reads values through NSUserDefaults. Escape the first
    // character so leading brackets/quotes are not parsed as property lists.
    command.args([
        "-title",
        &format!("\\{title}"),
        "-message",
        &format!("\\{body}"),
    ]);
    command
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn notifier(_title: &str, _body: &str) -> Option<Command> {
    None
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
        assert!(
            dispatch(Command::new(missing))
                .unwrap()
                .join()
                .unwrap()
                .is_err()
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

    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_notification_preserves_literal_text() {
        let command = terminal_notification(
            std::path::Path::new("terminal-notifier"),
            "[mise]",
            "\"file\"; $(touch unwanted)",
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "-title",
                "\\[mise]",
                "-message",
                "\\\"file\"; $(touch unwanted)"
            ]
        );
    }
}
