//! Bottle tag selection.
//!
//! On macOS, Homebrew builds bottles per OS version (`arm64_sequoia`, ...). A
//! bottle built for an older macOS runs on a newer one, so we pick the newest
//! tag that is <= the host version — the same logic brew uses — falling back
//! to the version-independent `all` tag. Linux bottles have a single
//! per-architecture tag (`x86_64_linux`, `arm64_linux`).

use std::collections::HashMap;
use std::sync::LazyLock as Lazy;

use crate::cmd::cmd;

use super::api::BottleFile;

/// macOS major version -> bottle tag suffix, newest first
const MACOS_VERSIONS: &[(u32, &str)] = &[
    (26, "tahoe"),
    (15, "sequoia"),
    (14, "sonoma"),
    (13, "ventura"),
    (12, "monterey"),
    (11, "big_sur"),
];

static MACOS_MAJOR: Lazy<u32> = Lazy::new(|| {
    let major = cmd("sw_vers", ["-productVersion"])
        .read()
        .ok()
        .and_then(|v| v.trim().split('.').next()?.parse().ok());
    if major.is_none() {
        // without the OS version every versioned tag is filtered out and the
        // downstream "no bottle for this machine" error would be misleading
        warn!(
            "brew: cannot determine the macOS version from `sw_vers` — only version-independent ('all') bottles will match"
        );
    }
    major.unwrap_or(0)
});

/// Bottle tags acceptable on this machine, in preference order
pub fn candidates() -> Vec<String> {
    let mut tags: Vec<String> = if cfg!(target_os = "macos") {
        MACOS_VERSIONS
            .iter()
            .filter(|(major, _)| *major <= *MACOS_MAJOR)
            .map(|(_, name)| format!("arm64_{name}"))
            .collect()
    } else if cfg!(target_arch = "aarch64") {
        vec!["arm64_linux".to_string()]
    } else {
        vec!["x86_64_linux".to_string()]
    };
    tags.push("all".to_string());
    tags
}

/// Pick the best bottle for this machine from a formula's `files` map.
/// Returns the tag and the bottle entry.
pub fn select(files: &HashMap<String, BottleFile>) -> Option<(String, &BottleFile)> {
    candidates()
        .into_iter()
        .find_map(|tag| files.get(&tag).map(|f| (tag, f)))
}

/// The host's exact preferred tag (for `variations` lookups)
pub fn host_tag() -> String {
    candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| "all".to_string())
}
