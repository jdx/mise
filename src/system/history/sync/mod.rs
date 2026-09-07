//! Sharing configuration through one setup repository: the origin
//! (`origin`), the versioned marker (`format`), the portable branch layout
//! (`layout`), network commands with the user's git configuration
//! (`network`), what is shareable now (`share`), the per-path transition
//! table (`reconcile`), publication from mise's bare repository
//! (`publish`), machine recovery refs (`backup`), explicit application
//! (`apply`), durable sync state (`state`), privacy filtering (`privacy`),
//! and the orchestrating `sync` run.

use eyre::{Result, bail};

pub(crate) mod apply;
pub(crate) mod backup;
pub(crate) mod format;
pub(crate) mod layout;
pub(crate) mod machines;
pub(crate) mod network;
pub(crate) mod origin;
mod preflight;
pub(crate) mod privacy;
pub(crate) mod publish;
pub(crate) mod reconcile;
pub(crate) mod run;
pub(crate) mod share;
pub(crate) mod state;

/// `settings.history.sync`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyncMode {
    /// Publish and fetch; any conflict pauses publication and incoming application for the setup.
    Sync,
    /// Download only: never publish, never change live files.
    FetchOnly,
    /// Publish and fetch; apply only on `mise bootstrap dotfiles pull`.
    Manual,
}

impl SyncMode {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "sync" => Ok(Self::Sync),
            "fetch-only" => Ok(Self::FetchOnly),
            "manual" => Ok(Self::Manual),
            other => bail!("unknown history.sync mode {other:?}; use sync, fetch-only, or manual"),
        }
    }

    pub(crate) fn current() -> Result<Self> {
        Self::parse(&crate::config::Settings::get().history.sync)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::FetchOnly => "fetch-only",
            Self::Manual => "manual",
        }
    }

    pub(crate) fn publishes(self) -> bool {
        self != Self::FetchOnly
    }

    /// What the mode does and does not do, disclosed when connecting.
    pub(crate) fn disclosure(self) -> &'static str {
        match self {
            Self::Sync => {
                "sync: this machine publishes its shared files and configuration and fetches the repository. Any conflict pauses publication and incoming application for the entire setup; local history, fetching, and eligible machine backups continue. Once all conflicts are resolved, `mise bootstrap dotfiles pull` applies the complete incoming batch with a protective checkpoint and recovery journal. Applying never runs `mise bootstrap`, installs or removes packages, or renders templates; when incoming configuration changes declarations, `mise bootstrap dotfiles status` says to run `mise bootstrap`."
            }
            Self::FetchOnly => {
                "fetch-only: this machine only downloads the repository and other machines' recovery refs. Nothing is published, no live file changes."
            }
            Self::Manual => {
                "manual: this machine publishes and fetches automatically; incoming changes are applied only by `mise bootstrap dotfiles pull`."
            }
        }
    }
}
