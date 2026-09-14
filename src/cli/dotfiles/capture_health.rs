//! Whether edits are being saved automatically, and what to do when not.

use eyre::Result;
use serde::Serialize;

use crate::config::Config;
use crate::system::services_common::ServiceState;
use crate::system::user_services::{self, UserServiceRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Watcher {
    /// A watcher holds the store's watch lock.
    Running,
    /// `[bootstrap.services]` declares the built-in watcher and its service
    /// is running, but nothing holds this store's watch lock: the process
    /// is an older mise's watcher (the history locks moved into the state
    /// directory in 2026.9.5) or one started with a different
    /// `MISE_STATE_DIR`. Applying the declaration again restarts it.
    ServiceNotWatching,
    /// `[bootstrap.services]` declares the built-in watcher, but none runs.
    DeclaredNotRunning,
    NotDeclared,
}

impl Watcher {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::ServiceNotWatching => "running but not watching this store",
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
                && declaration.state != ServiceState::Absent
        });
    if !declared {
        return Ok(Watcher::NotDeclared);
    }
    // The service manager may be running a process that is not watching
    // this store, which `mise bootstrap services apply` restarts. Telling
    // those two apart is what keeps the advice from repeating a step the
    // user already took.
    for request in declared_watchers(&config) {
        match user_services::is_process_running(&request).await {
            Ok(true) => return Ok(Watcher::ServiceNotWatching),
            Ok(false) => {}
            // A service manager that cannot be asked right now leaves the
            // diagnosis where it was before the probe existed, rather than
            // taking the whole report down with it.
            Err(err) => debug!(
                "history: could not ask the service manager about {}: {err:#}",
                request.name
            ),
        }
    }
    Ok(Watcher::DeclaredNotRunning)
}

/// Every declared watcher that should be watching this store but is not,
/// as service requests. Only the service-manager probe needs these; whether
/// a watcher is declared at all is read from the declarations themselves,
/// which say so even when no request can be built. More than one service
/// may name the builtin, and any of them running is a process that should
/// be watching. The predicate is the one an apply restarts by, so this
/// never reports a state the advice would not repair.
fn declared_watchers(config: &Config) -> Vec<UserServiceRequest> {
    user_services::requests_from_config(config)
        .unwrap_or_default()
        .into_iter()
        .filter(user_services::stale_history_watcher)
        .collect()
}

/// The next step for each state.
pub(crate) fn advice(state: Watcher) -> &'static str {
    match state {
        Watcher::Running => "edits are saved automatically",
        Watcher::ServiceNotWatching => {
            "the history service is running but is not watching this store (its process predates this mise version, or uses a different MISE_STATE_DIR): restart it with `mise bootstrap services apply`"
        }
        Watcher::DeclaredNotRunning => {
            "the history watcher is declared but not running: run `mise bootstrap services apply`"
        }
        Watcher::NotDeclared => {
            "automatic capture is inactive: declare `[bootstrap.services.mise-history] builtin = \"history-watch\"` and run `mise bootstrap`; until then edits are saved by `mise dot save` or `mise dot watch --once`"
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
