//! The switch mise's unit-test harness flips so that code in this crate
//! behaves the way it did under `#[cfg(test)]` when it lived in mise.
//!
//! `cfg(test)` is only set for the crate being tested, so it cannot reach a
//! dependency. mise's test constructor calls [`enable`] instead, before
//! anything reads the environment statics.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static HOME: OnceLock<PathBuf> = OnceLock::new();

/// Run as mise's unit-test harness, with `home` as the home directory.
///
/// Must be called before anything reads [`crate::env::HOME`]. Only the first
/// call takes effect.
pub fn enable(home: PathBuf) {
    let _ = HOME.set(home);
}

/// Whether [`enable`] has been called.
pub fn active() -> bool {
    HOME.get().is_some()
}

/// The home directory the test harness chose, if it is running.
pub fn home() -> Option<&'static Path> {
    HOME.get().map(PathBuf::as_path)
}
