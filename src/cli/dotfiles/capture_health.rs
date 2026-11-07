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
    /// No watcher is declared: edits are saved by hand with `mise dot save`.
    /// This is a choice, not a fault.
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
            "edits are saved when you run `mise dot save` (or `mise dot watch --once`); to save them automatically, add a `[bootstrap.services.mise-history]` table with `builtin = \"history-watch\"` to your global config and run `mise bootstrap`"
        }
    }
}

/// The error of the running watcher's last capture, when the capture after
/// it has not succeeded yet.
pub(crate) fn failing_capture() -> Option<String> {
    crate::system::history::health::read(&crate::system::history::store::state_dir())?
        .failing_capture()
        .map(str::to_string)
}

/// Warns when the watcher should be saving edits but is not. Saving by hand
/// with `mise dot save` is a supported way to work, so a watcher that was
/// never declared is only mentioned, not warned about.
pub(crate) async fn report() {
    // with history disabled nothing is saved either way, and the caller
    // has already said so
    if !crate::config::Settings::get().history.enabled {
        return;
    }
    match watcher().await {
        Ok(Watcher::Running) => match failing_capture() {
            Some(error)
                if crate::system::history::health::is_stale_watcher_schema_error(&error) =>
            {
                warn!(
                    "history: {}; edits are not being saved (`mise dot status` has the capture error)",
                    crate::system::history::health::STALE_WATCHER_SCHEMA_ADVICE
                )
            }
            Some(error) => warn!(
                "history: watcher running, but its captures are failing: {error}; edits are not being saved (`mise dot status` has the details)"
            ),
            None => {
                info!("history: watcher running; autosave-enabled files are saved automatically")
            }
        },
        Ok(Watcher::NotDeclared) => info!("history: {}", advice(Watcher::NotDeclared)),
        Ok(state) => warn!("history: {}", advice(state)),
        Err(err) => {
            warn!("history: could not determine whether edits are saved automatically: {err:#}")
        }
    }
}
