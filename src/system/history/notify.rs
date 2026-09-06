//! Optional desktop notifications for sync conflicts that newly need a
//! decision (`settings.history.notify`, on by default). Best effort: the
//! notifier is started and not waited for, a missing desktop or tool is a
//! debug line, and nothing here ever holds up a capture or a sync.

use std::process::{Command, Stdio};

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
    match command.spawn() {
        Ok(_child) => debug!("history: notified: {title}"),
        Err(err) => debug!("history: could not notify ({title}): {err}"),
    }
}

#[cfg(target_os = "linux")]
fn notifier(title: &str, body: &str) -> Option<Command> {
    let bin = crate::file::which_spawnable("notify-send")?;
    let mut command = Command::new(bin);
    command
        .args(["--app-name", "mise", "--urgency", "normal", "--"])
        .arg(title)
        .arg(body);
    Some(command)
}

#[cfg(target_os = "macos")]
fn notifier(title: &str, body: &str) -> Option<Command> {
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

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn notifier(_title: &str, _body: &str) -> Option<Command> {
    None
}
