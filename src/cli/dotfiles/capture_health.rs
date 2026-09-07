//! Whether edits are being saved automatically, and what to do when not.

use eyre::Result;
use serde::Serialize;

use crate::config::Config;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Watcher {
    /// A watcher holds the store's watch lock.
    Running,
    /// `[bootstrap.services]` declares the built-in watcher, but none runs.
    DeclaredNotRunning,
    NotDeclared,
}

impl Watcher {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::DeclaredNotRunning => "declared but not running",
            Self::NotDeclared => "not declared",
        }
    }
}

/// The watcher's state for the current store and configuration.
pub(crate) async fn watcher() -> Result<Watcher> {
    let running = crate::system::history::watch::runtime::is_running(
        &crate::system::history::store::state_dir(),
    );
    if running {
        return Ok(Watcher::Running);
    }
    let config = Config::get().await?;
    let declared = crate::system::services_common::compose_user_declarations(&config)?
        .values()
        .any(|(declaration, _)| {
            declaration.builtin.as_deref() == Some("history-watch")
                && declaration.state != crate::system::services_common::ServiceState::Absent
        });
    Ok(if declared {
        Watcher::DeclaredNotRunning
    } else {
        Watcher::NotDeclared
    })
}

/// The next step for each state.
pub(crate) fn advice(state: Watcher) -> &'static str {
    match state {
        Watcher::Running => "edits are saved automatically",
        Watcher::DeclaredNotRunning => {
            "the history watcher is declared but not running: run `mise bootstrap services apply`"
        }
        Watcher::NotDeclared => {
            "automatic capture is inactive: declare `[bootstrap.services.mise-history] builtin = \"history-watch\"` and run `mise bootstrap`; until then edits are saved by `mise bootstrap dotfiles save` or `mise bootstrap dotfiles watch --once`"
        }
    }
}

/// Warns when enrollment succeeded but nothing saves edits automatically.
pub(crate) async fn report() {
    match watcher().await {
        Ok(Watcher::Running) => {
            info!("history: watcher running; autosave-enabled files are saved automatically")
        }
        Ok(state) => warn!("history: {}", advice(state)),
        Err(err) => {
            warn!("history: could not determine whether edits are saved automatically: {err:#}")
        }
    }
}
