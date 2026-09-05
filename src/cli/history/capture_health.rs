//! Whether edits are being saved automatically, and what to do when not.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Watcher {
    Running,
    NotDeclared,
}

impl Watcher {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::NotDeclared => "not declared",
        }
    }
}

/// The watcher's state. No watcher service exists in this version, so
/// automatic capture is never running.
pub(crate) fn watcher() -> Watcher {
    Watcher::NotDeclared
}

/// The next step for each state.
pub(crate) fn advice(state: Watcher) -> &'static str {
    match state {
        Watcher::Running => "edits are saved automatically",
        Watcher::NotDeclared => {
            "automatic capture is inactive: edits are saved by `mise history save` and by every mutating bootstrap command"
        }
    }
}

/// Warns when enrollment succeeded but nothing saves edits automatically.
pub(crate) fn report() {
    let state = watcher();
    if state != Watcher::Running {
        warn!("history: {}", advice(state));
    }
}
