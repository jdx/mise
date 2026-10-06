use eyre::Result;

use crate::system::history::notify;

/// Send a test desktop notification
///
/// Checks that `history.notify` can reach you. On macOS the first
/// notification asks for permission, so run this once to be prompted now
/// instead of at the first sync conflict. Fails with the reason when this
/// build or machine cannot show notifications, such as an unofficial macOS
/// build (Homebrew), a missing `notify-send` on Linux, denied permission, or
/// `history.notify = false`.
///
/// Notifications are only sent when conflicts pause sharing for the setup.
/// Other problems, such as a watcher that cannot save, show in `mise doctor`
/// and `mise dot status`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, example(r###"mise dot notify"###))]
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
