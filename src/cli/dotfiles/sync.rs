use eyre::{Result, bail};

use crate::system::history::sync::run::{self, SyncRequest};

/// Push saved checkpoints and fetch incoming changes
///
/// Fetches the origin branch and pushes your local history to it. If the push
/// is rejected, mise fetches again and reconciles the two histories without
/// rewriting either. Incoming changes and conflicts are recorded for
/// `mise dot pull`; this command never changes your files. With `--fetch-only`,
/// or in `fetch-only` mode, nothing is pushed.
///
/// The watcher does this on its own in `sync` and `fetch-only` mode (the
/// `history.sync` setting). See https://mise.jdx.dev/dotfiles/sync.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot sync", help = "Push and fetch now"),
    example("mise dot sync --fetch-only", help = "Fetch without pushing")
)]
pub(crate) struct DotfilesSync {
    /// Fetch without pushing
    #[usage(long)]
    fetch_only: bool,

    /// Allow pushing older unencrypted versions of files that are now encrypted
    #[usage(long)]
    allow_plaintext_history: bool,

    /// Warn instead of failing when the origin is unreachable
    #[usage(long)]
    best_effort: bool,
}

impl DotfilesSync {
    pub(crate) async fn run(self) -> Result<()> {
        match self.sync().await {
            Ok(()) => Ok(()),
            Err(err) if self.best_effort && is_network_failure(&err) => {
                warn!("history sync: {err:#}");
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    async fn sync(&self) -> Result<()> {
        if !crate::config::Settings::get().history.enabled {
            bail!("history is disabled (history.enabled = false)");
        }
        let (store, tracked, _) = super::history::open().await?;
        if let Some(reason) = store.unavailable() {
            bail!("cannot synchronize: {reason}");
        }
        let mut request = SyncRequest::new(self.fetch_only);
        request.allow_plaintext_history = self.allow_plaintext_history;
        let outcome = run::sync(&store, &tracked, &request)?;
        crate::system::history::sync::origin::report(&outcome);
        Ok(())
    }
}

fn is_network_failure(error: &eyre::Report) -> bool {
    error
        .downcast_ref::<crate::system::history::sync::network::NetworkError>()
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_effort_does_not_hide_policy_or_configuration_errors() {
        assert!(!is_network_failure(&eyre::eyre!(
            "invalid encryption policy"
        )));
        assert!(!is_network_failure(&eyre::eyre!("history is disabled")));
        let transport = eyre::Report::new(crate::system::history::sync::network::NetworkError(
            "origin unavailable".into(),
        ));
        assert!(is_network_failure(&transport.wrap_err("synchronizing")));
    }
}
