use crate::types::*;
use eyre::Result;
use std::collections::HashMap;

// Macro helper for creating hashmaps
#[macro_export]
macro_rules! hashmap {
    (@single $($x:tt)*) => (());
    (@count $($rest:expr),*) => (<[()]>::len(&[$(hashmap!(@single $rest)),*]));

    ($($key:expr => $value:expr,)+) => { hashmap!($($key => $value),+) };
    ($($key:expr => $value:expr),*) => {
        {
            let _cap = hashmap!(@count $($key),*);
            let mut _map = ::std::collections::HashMap::with_capacity(_cap);
            $(
                let _ = _map.insert($key, $value);
            )*
            _map
        }
    };
}

// Re-export the macro for use in other modules
pub use hashmap;

pub fn apply_override(mut orig: AquaPackage, avo: &AquaPackage) -> AquaPackage {
    // For now, we need to manually check each field because deepmerge doesn't have a
    // built-in policy for "only merge if non-empty". We could create a custom policy
    // but it would require modifying the deepmerge crate itself.

    // Only override fields if they're not empty/default in the override
    if avo.r#type != AquaPackageType::GithubRelease {
        orig.r#type = avo.r#type.clone();
    }
    if !avo.repo_owner.is_empty() {
        orig.repo_owner = avo.repo_owner.clone();
    }
    if !avo.repo_name.is_empty() {
        orig.repo_name = avo.repo_name.clone();
    }
    if avo.name.is_some() {
        orig.name = avo.name.clone();
    }
    if !avo.asset.is_empty() {
        orig.asset = avo.asset.clone();
    }
    if !avo.url.is_empty() {
        orig.url = avo.url.clone();
    }
    if avo.description.is_some() {
        orig.description = avo.description.clone();
    }
    if !avo.format.is_empty() {
        orig.format = avo.format.clone();
    }
    if avo.rosetta2 {
        orig.rosetta2 = avo.rosetta2;
    }
    if avo.windows_arm_emulation {
        orig.windows_arm_emulation = avo.windows_arm_emulation;
    }
    if avo.complete_windows_ext {
        orig.complete_windows_ext = avo.complete_windows_ext;
    }
    if !avo.supported_envs.is_empty() {
        orig.supported_envs = avo.supported_envs.clone();
    }
    if !avo.files.is_empty() {
        orig.files = avo.files.clone();
    }
    if !avo.replacements.is_empty() {
        orig.replacements.extend(avo.replacements.clone());
    }
    if avo.version_prefix.is_some() {
        orig.version_prefix = avo.version_prefix.clone();
    }
    if avo.version_filter.is_some() {
        orig.version_filter = avo.version_filter.clone();
    }
    if avo.version_source.is_some() {
        orig.version_source = avo.version_source.clone();
    }
    if avo.checksum.is_some() {
        orig.checksum = avo.checksum.clone();
    }
    if avo.slsa_provenance.is_some() {
        orig.slsa_provenance = avo.slsa_provenance.clone();
    }
    if avo.minisign.is_some() {
        orig.minisign = avo.minisign.clone();
    }
    if !avo.overrides.is_empty() {
        orig.overrides = avo.overrides.clone();
    }
    if !avo.version_constraint.is_empty() {
        orig.version_constraint = avo.version_constraint.clone();
    }
    if !avo.version_overrides.is_empty() {
        orig.version_overrides = avo.version_overrides.clone();
    }
    if avo.no_asset {
        orig.no_asset = avo.no_asset;
    }
    if avo.error_message.is_some() {
        orig.error_message = avo.error_message.clone();
    }
    if avo.path.is_some() {
        orig.path = avo.path.clone();
    }
    orig
}

// Platform detection helpers
pub fn os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

pub fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    }
}

// Template rendering function - basic implementation for aqua templates
pub fn aqua_template_render(template: &str, ctx: &HashMap<String, String>) -> Result<String> {
    let mut result = template.to_string();

    // Simple template substitution for aqua templates like {{.Version}}, {{.Arch}}, etc.
    for (key, value) in ctx {
        let patterns = [
            format!("{{{{.{}}}}}", key), // {{.Key}}
            format!("{{{{{}}}}}", key),  // {{Key}} (alternative format)
        ];

        for pattern in &patterns {
            result = result.replace(pattern, value);
        }
    }

    Ok(result)
}

// Version utility functions - stubs for now
pub fn split_version_prefix(v: &str) -> (&str, &str) {
    // Split version into prefix and semver parts
    // Common prefixes: "v", "release-", "version-", etc.
    let prefixes = ["version-", "release-", "ver-", "v"];

    for prefix in &prefixes {
        if let Some(remaining) = v.strip_prefix(prefix) {
            // Check if remaining part looks like a version (starts with digit)
            if remaining.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return (prefix, remaining);
            }
        }
    }

    // If no common prefix found, check for single 'v' prefix
    if v.starts_with('v') && v.len() > 1 {
        let remaining = &v[1..];
        if remaining.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return ("v", remaining);
        }
    }

    // No prefix found
    ("", v)
}

pub fn versions_versioning_new(v: &str) -> Option<semver::Version> {
    // Parse version using semver - remove prefix first
    let (_, clean_version) = split_version_prefix(v);
    semver::Version::parse(clean_version).ok()
}

pub fn versions_requirement_new(req: &str) -> Option<semver::VersionReq> {
    // Parse version requirement using semver
    semver::VersionReq::parse(req).ok()
}
