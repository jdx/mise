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
    // For boolean fields, we need to check if they're explicitly set in the override
    // Since we can't distinguish between "false by default" and "explicitly set to false",
    // we'll apply these boolean overrides unconditionally to allow both true and false overrides
    orig.rosetta2 = avo.rosetta2;
    orig.windows_arm_emulation = avo.windows_arm_emulation;
    orig.complete_windows_ext = avo.complete_windows_ext;
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
    // Apply no_asset unconditionally to allow both true and false overrides
    orig.no_asset = avo.no_asset;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_override_boolean_fields() {
        let mut orig = AquaPackage {
            complete_windows_ext: true,
            rosetta2: false,
            windows_arm_emulation: true,
            no_asset: false,
            ..Default::default()
        };

        // Test overriding complete_windows_ext from true to false
        let override_to_false = AquaPackage {
            complete_windows_ext: false,
            ..Default::default()
        };
        orig = apply_override(orig.clone(), &override_to_false);
        assert_eq!(
            orig.complete_windows_ext, false,
            "complete_windows_ext should be overridden to false"
        );

        // Test overriding complete_windows_ext from false to true
        let override_to_true = AquaPackage {
            complete_windows_ext: true,
            ..Default::default()
        };
        orig = apply_override(orig.clone(), &override_to_true);
        assert_eq!(
            orig.complete_windows_ext, true,
            "complete_windows_ext should be overridden to true"
        );

        // Test overriding rosetta2 from false to true
        let override_rosetta2 = AquaPackage {
            rosetta2: true,
            ..Default::default()
        };
        orig = apply_override(orig.clone(), &override_rosetta2);
        assert_eq!(orig.rosetta2, true, "rosetta2 should be overridden to true");

        // Test overriding no_asset from false to true
        let override_no_asset = AquaPackage {
            no_asset: true,
            ..Default::default()
        };
        orig = apply_override(orig.clone(), &override_no_asset);
        assert_eq!(orig.no_asset, true, "no_asset should be overridden to true");
    }

    #[test]
    fn test_apply_override_preserves_other_fields() {
        let orig = AquaPackage {
            repo_owner: "original".to_string(),
            repo_name: "repo".to_string(),
            complete_windows_ext: true,
            ..Default::default()
        };

        let avo = AquaPackage {
            complete_windows_ext: false, // This should override
            // repo_owner and repo_name are empty, so they should NOT be overridden
            ..Default::default()
        };

        let result = apply_override(orig.clone(), &avo);

        // Boolean field should be overridden
        assert_eq!(result.complete_windows_ext, false);

        // Non-empty fields should be preserved when override is empty
        assert_eq!(result.repo_owner, "original");
        assert_eq!(result.repo_name, "repo");
    }
}
