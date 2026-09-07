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
pub(crate) mod onboard;
pub(crate) mod origin;
mod preflight;
pub(crate) mod privacy;
pub(crate) mod publish;
pub(crate) mod reconcile;
pub(crate) mod run;
pub(crate) mod share;
pub(crate) mod state;

/// `settings.history.sync`: what the watcher does on its own. Explicit
/// commands (`sync`, `pull`) work the same in every mode except that
/// `fetch-only` never publishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyncMode {
    /// The watcher publishes after saves, fetches periodically, and applies
    /// incoming changes once the complete setup is free of conflicts.
    Sync,
    /// The watcher fetches periodically; nothing is ever published and no
    /// live file changes without `pull`.
    FetchOnly,
    /// No automatic network activity: `sync` and `pull` on request only.
    Manual,
}

/// What a mode does in the background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Automatic {
    pub publish: bool,
    pub fetch: bool,
    pub apply: bool,
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

    pub(crate) fn automatic(self) -> Automatic {
        match self {
            Self::Sync => Automatic {
                publish: true,
                fetch: true,
                apply: true,
            },
            Self::FetchOnly => Automatic {
                publish: false,
                fetch: true,
                apply: false,
            },
            Self::Manual => Automatic {
                publish: false,
                fetch: false,
                apply: false,
            },
        }
    }

    /// What the mode does and does not do, disclosed when connecting.
    pub(crate) fn disclosure(self) -> &'static str {
        match self {
            Self::Sync => {
                "sync: the watcher publishes saved changes and fetches periodically. Any conflict pauses publication and incoming application for the entire setup; local history, fetching, and eligible machine backups continue. Incoming changes are preflighted together and applied with a protective checkpoint and recovery journal. Applying never runs `mise bootstrap` or renders templates. Run `mise bootstrap` when the new declarations need to be applied."
            }
            Self::FetchOnly => {
                "fetch-only: the watcher only downloads the repository and other machines' recovery refs. Nothing is ever published, and no live file changes unless you run `mise bootstrap dotfiles pull`."
            }
            Self::Manual => {
                "manual: no automatic network activity. `mise bootstrap dotfiles sync` publishes and fetches, and `mise bootstrap dotfiles pull` applies, when you run them."
            }
        }
    }
}
