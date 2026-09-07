use eyre::{Result, bail};

use crate::system::history::sync::run::{self, SyncRequest};

/// Publish, fetch, and record what is pending now
///
/// Fetches the setup branch and other machines' recovery refs, publishes
/// this machine's shared files and configuration (leased on the branch
/// head; a rejection fetches again and retries), uploads eligible
/// checkpoints, and records incoming changes to apply and conflicts to
/// decide. Live files are never changed here: `mise bootstrap dotfiles pull` does
/// that. In `fetch-only` mode nothing is published.
///
/// The history watcher does this on its own in `sync` and `fetch-only` mode
/// (`settings.history.sync`); this command is for right now.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesSync {
    /// Fetch without publishing
    #[usage(long)]
    fetch_only: bool,

    /// Warn instead of failing when the origin is unreachable
    #[usage(long)]
    best_effort: bool,
}

impl DotfilesSync {
    pub(crate) async fn run(self) -> Result<()> {
        match self.sync().await {
            Ok(()) => Ok(()),
            Err(err) if self.best_effort => {
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
        let outcome = run::sync(&store, &tracked, &SyncRequest::new(self.fetch_only))?;
        crate::system::history::sync::origin::report(&outcome);
        Ok(())
    }
}
