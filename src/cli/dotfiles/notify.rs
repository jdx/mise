use eyre::Result;

use crate::system::history::notify;

/// Send a test desktop notification
///
/// Checks that `history.notify` can reach you. On macOS the first notification
/// asks for permission, so run this once to be prompted now instead of at the
/// first sync conflict. Fails with the reason when this build or machine cannot
/// show notifications: a macOS build that is not mise's signed release (such as
/// Homebrew's), a missing `notify-send` on Linux, denied permission, or
/// `history.notify = false`.
///
/// mise notifies you only when a sync conflict pauses syncing. Other problems,
/// such as a watcher that cannot save, appear in `mise doctor` and
/// `mise dot status`.
#[derive(Debug, usage_rs::Args)]
#[usage(example("mise dot notify", help = "Send a test notification"))]
pub(crate) struct DotfilesNotify {}

impl DotfilesNotify {
    pub(crate) async fn run(self) -> Result<()> {
        if !crate::config::Settings::get().history.notify {
            eyre::bail!(
                "desktop notifications are disabled (history.notify = false); enable the setting to test them"
            );
        }
        if let Some(reason) = notify::unavailable_reason() {
            eyre::bail!("desktop notifications are unavailable: {reason}");
        }
        notify::send_test()?;
        miseprintln!("sent a test notification");
        Ok(())
    }
}
