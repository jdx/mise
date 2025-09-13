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
    use deepmerge::{DeepMerge, DefaultPolicy};
    orig.merge_with_policy(avo.clone(), &DefaultPolicy);
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
    // Simple stub - would need actual version prefix splitting logic
    // Should split version into prefix and semver parts
    ("", v)
}

pub fn versions_versioning_new(_v: &str) -> Option<()> {
    // Stub for versions crate integration
    // Would return parsed version object
    None
}

pub fn versions_requirement_new(_req: &str) -> Option<()> {
    // Stub for versions crate integration
    // Would return parsed requirement object
    None
}
