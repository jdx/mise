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
    (27, "golden_gate"),
    (26, "tahoe"),
    (15, "sequoia"),
    (14, "sonoma"),
    (13, "ventura"),
    (12, "monterey"),
    (11, "big_sur"),
    (10, "catalina"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperatingSystem {
    Macos,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Architecture {
    Arm64,
    Intel,
    Unsupported,
}

static MACOS_MAJOR: Lazy<u32> = Lazy::new(|| {
    let major = cmd("sw_vers", ["-productVersion"])
        .read()
        .ok()
        .and_then(|v| parse_host_macos_major(v.trim()));
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
pub(super) fn candidates() -> Vec<String> {
    candidates_for(host_os(), host_arch(), host_macos_major())
}

fn candidates_for(
    os: OperatingSystem,
    arch: Architecture,
    macos_major: Option<u32>,
) -> Vec<String> {
    let mut tags: Vec<String> = if arch == Architecture::Unsupported {
        Vec::new()
    } else if os == OperatingSystem::Macos {
        MACOS_VERSIONS
            .iter()
            .filter(|(major, _)| macos_major.is_some_and(|host| *major <= host))
            .filter_map(|(major, name)| match arch {
                Architecture::Arm64 if *major >= 11 => Some(format!("arm64_{name}")),
                Architecture::Arm64 => None,
                Architecture::Intel => Some((*name).to_string()),
                Architecture::Unsupported => unreachable!(),
            })
            .collect()
    } else if arch == Architecture::Arm64 {
        vec!["arm64_linux".to_string()]
    } else {
        vec!["x86_64_linux".to_string()]
    };
    tags.push("all".to_string());
    tags
}

pub(super) fn host_os() -> OperatingSystem {
    if cfg!(target_os = "macos") {
        OperatingSystem::Macos
    } else {
        OperatingSystem::Linux
    }
}

pub(super) fn host_arch() -> Architecture {
    if cfg!(target_arch = "aarch64") {
        Architecture::Arm64
    } else if cfg!(target_arch = "x86_64") {
        Architecture::Intel
    } else {
        Architecture::Unsupported
    }
}

fn parse_host_macos_major(version: &str) -> Option<u32> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    if major == 10 {
        (parts.next()?.parse::<u32>().ok()? == 15).then_some(10)
    } else {
        (major >= 11).then_some(major)
    }
}

pub(super) fn host_macos_major() -> Option<u32> {
    (host_os() == OperatingSystem::Macos && *MACOS_MAJOR != 0).then_some(*MACOS_MAJOR)
}

pub(super) fn macos_major(name: &str) -> Option<u32> {
    MACOS_VERSIONS
        .iter()
        .find_map(|(major, candidate)| (*candidate == name).then_some(*major))
}

pub(super) fn is_known_platform_tag(tag: &str) -> bool {
    matches!(tag, "x86_64_linux" | "arm64_linux")
        || MACOS_VERSIONS.iter().any(|(_, name)| {
            tag == *name
                || (*name != "catalina"
                    && tag.strip_prefix("arm64_").is_some_and(|tag| tag == *name))
        })
}

/// Pick the best bottle for this machine from a formula's `files` map.
/// Returns the tag and the bottle entry.
pub(super) fn select(files: &HashMap<String, BottleFile>) -> Option<(String, &BottleFile)> {
    candidates()
        .into_iter()
        .find_map(|tag| files.get(&tag).map(|f| (tag, f)))
}

/// The host's exact preferred tag (for `variations` lookups)
pub(super) fn host_tag() -> String {
    candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| "all".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_tag_mapping_covers_architecture_and_golden_gate() {
        assert_eq!(
            candidates_for(OperatingSystem::Macos, Architecture::Arm64, Some(27))[0],
            "arm64_golden_gate"
        );
        assert_eq!(
            candidates_for(OperatingSystem::Macos, Architecture::Intel, Some(27))[0],
            "golden_gate"
        );
        assert_eq!(
            candidates_for(OperatingSystem::Linux, Architecture::Arm64, None)[0],
            "arm64_linux"
        );
        assert_eq!(
            candidates_for(OperatingSystem::Linux, Architecture::Intel, None)[0],
            "x86_64_linux"
        );
        assert_eq!(macos_major("golden_gate"), Some(27));
        assert_eq!(macos_major("catalina"), Some(10));
        assert!(is_known_platform_tag("arm64_golden_gate"));
        assert!(is_known_platform_tag("catalina"));
        assert!(!is_known_platform_tag("arm64_catalina"));
        assert!(!is_known_platform_tag("future_os"));
    }

    #[test]
    fn catalina_is_exact_and_intel_only() {
        assert_eq!(parse_host_macos_major("10.15"), Some(10));
        assert_eq!(parse_host_macos_major("10.15.7"), Some(10));
        assert_eq!(parse_host_macos_major("10.14.6"), None);
        assert_eq!(parse_host_macos_major("10.16"), None);
        assert_eq!(parse_host_macos_major("15.6.1"), Some(15));
        assert_eq!(
            candidates_for(OperatingSystem::Macos, Architecture::Intel, Some(10)),
            ["catalina", "all"]
        );
        assert_eq!(
            candidates_for(OperatingSystem::Macos, Architecture::Arm64, Some(10)),
            ["all"]
        );
    }

    #[test]
    fn unsupported_architecture_only_accepts_universal_formula_bottles() {
        assert_eq!(
            candidates_for(OperatingSystem::Macos, Architecture::Unsupported, Some(15)),
            ["all"]
        );
        assert_eq!(
            candidates_for(OperatingSystem::Linux, Architecture::Unsupported, None),
            ["all"]
        );
    }
}
