use std::path::Path;

use eyre::Result;
use serde::Deserialize;
use serde::de::Deserializer;

use crate::file;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageJson {
    dev_engines: Option<DevEngines>,
    package_manager: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevEngines {
    #[serde(default, deserialize_with = "deserialize_one_or_first")]
    runtime: Option<DevEngine>,
    #[serde(default, deserialize_with = "deserialize_one_or_first")]
    package_manager: Option<DevEngine>,
}

#[derive(Debug, Clone, Deserialize)]
struct DevEngine {
    name: Option<String>,
    version: Option<String>,
}

/// Deserialize a field that may be a single object or an array (take the first element).
/// The npm devEngines spec allows both forms.
fn deserialize_one_or_first<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<DevEngine>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(DevEngine),
        Many(Vec<DevEngine>),
    }

    match Option::<OneOrMany>::deserialize(deserializer)? {
        None => Ok(None),
        Some(OneOrMany::One(engine)) => Ok(Some(engine)),
        Some(OneOrMany::Many(engines)) => Ok(engines.into_iter().next()),
    }
}

impl PackageJson {
    pub fn parse(path: &Path) -> Result<Self> {
        let contents = file::read_to_string(path)?;
        let pkg: PackageJson = serde_json::from_str(&contents)?;
        Ok(pkg)
    }

    /// Extract a runtime version for the given tool name from devEngines.runtime
    pub fn runtime_version(&self, tool_name: &str) -> Option<String> {
        self.dev_engines
            .as_ref()
            .and_then(|de| de.runtime.as_ref())
            .filter(|r| r.name.as_deref() == Some(tool_name))
            .and_then(|r| r.version.as_deref())
            .map(simplify_semver)
            .filter(|v| !v.is_empty())
    }

    /// Extract a package manager version for the given tool name.
    /// Checks devEngines.packageManager first, then falls back to the packageManager field.
    pub fn package_manager_version(&self, tool_name: &str) -> Option<String> {
        // Try devEngines.packageManager first
        self.dev_engines
            .as_ref()
            .and_then(|de| de.package_manager.as_ref())
            .filter(|pm| pm.name.as_deref() == Some(tool_name))
            .and_then(|pm| pm.version.as_deref())
            .map(simplify_semver)
            .filter(|v| !v.is_empty())
            .or_else(|| {
                // Fall back to packageManager field (e.g. "pnpm@9.1.0+sha256.abc")
                let pm_field = self.package_manager.as_deref()?;
                let (name, rest) = pm_field.split_once('@')?;
                if name != tool_name {
                    return None;
                }
                // Strip +sha... suffix
                let version = rest.split('+').next().unwrap_or(rest).trim();
                if version.is_empty() {
                    return None;
                }
                Some(version.to_string())
            })
    }
}

/// Simplify a semver range to a mise-compatible version prefix.
///
/// Strips range operators (>=, ^, ~) and trailing `.0` components to produce
/// a prefix that mise can match against. For exact versions, returns as-is.
/// Upper-bound operators (`<`, `<=`) are ignored since they don't indicate
/// a version to install.
///
/// # TODO
/// This doesn't handle all edge cases correctly. For example, `^20.0.1` should not
/// match `20.0.0`, but our simplified approach strips it to `20` which would match.
/// Full semver range support may be added in the future.
pub fn simplify_semver(input: &str) -> String {
    let input = input.trim();
    if input == "*" || input == "x" {
        return "latest".to_string();
    }

    // Upper-bound operators don't indicate a version to install
    if input.starts_with('<') || input.starts_with("<=") {
        return String::new();
    }

    // Strip leading range operators
    let version = input
        .trim_start_matches(">=")
        .trim_start_matches('>')
        .trim_start_matches('^')
        .trim_start_matches('~')
        .trim_start_matches('=')
        .trim();

    if version.is_empty() {
        return "latest".to_string();
    }

    // Replace wildcard segments (x, *) with truncation
    // e.g. "18.x" -> "18", "18.2.*" -> "18.2"
    let parts: Vec<&str> = version
        .split('.')
        .take_while(|p| *p != "x" && *p != "*")
        .collect();
    if parts.is_empty() {
        return "latest".to_string();
    }
    if parts.len() < version.split('.').count() {
        // Had wildcard segments, return truncated prefix
        return parts.join(".");
    }

    let had_operator = version != input;

    // Only strip trailing .0 components when a range operator was present,
    // since ranges imply prefix matching. Exact versions are kept as-is.
    if had_operator {
        let trimmed: Vec<&str> = match parts.as_slice() {
            [major, "0", "0"] => vec![major],
            [major, minor, "0"] => vec![major, minor],
            _ => parts,
        };
        trimmed.join(".")
    } else {
        version.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simplify_semver() {
        assert_eq!(simplify_semver(">=18.0.0"), "18");
        assert_eq!(simplify_semver("^20.0.0"), "20");
        assert_eq!(simplify_semver("~18.2.0"), "18.2");
        assert_eq!(simplify_semver("9.1.0"), "9.1.0");
        assert_eq!(simplify_semver("9.1.2"), "9.1.2");
        assert_eq!(simplify_semver("18"), "18");
        assert_eq!(simplify_semver("*"), "latest");
        assert_eq!(simplify_semver("x"), "latest");
        assert_eq!(simplify_semver(">= 18.0.0"), "18");
        assert_eq!(simplify_semver("^18.2.0"), "18.2");
        assert_eq!(simplify_semver("~18.0.0"), "18");
        assert_eq!(simplify_semver("=18.0.0"), "18");
    }

    #[test]
    fn test_simplify_semver_upper_bound() {
        assert_eq!(simplify_semver("<18.0.0"), "");
        assert_eq!(simplify_semver("<=18.0.0"), "");
    }

    #[test]
    fn test_simplify_semver_wildcards() {
        assert_eq!(simplify_semver("18.x"), "18");
        assert_eq!(simplify_semver("18.*"), "18");
        assert_eq!(simplify_semver("18.2.x"), "18.2");
        assert_eq!(simplify_semver("18.2.*"), "18.2");
    }

    #[test]
    fn test_runtime_version() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "node",
                        "version": ">=20.0.0"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.runtime_version("node"), Some("20".to_string()));
        assert_eq!(pkg.runtime_version("bun"), None);
    }

    #[test]
    fn test_runtime_version_bun() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "bun",
                        "version": "^1.0.0"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.runtime_version("bun"), Some("1".to_string()));
        assert_eq!(pkg.runtime_version("node"), None);
    }

    #[test]
    fn test_runtime_version_array_form() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devEngines": {
                    "runtime": [
                        { "name": "node", "version": ">=22.0.0" },
                        { "name": "bun", "version": ">=1.0.0" }
                    ]
                }
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.runtime_version("node"), Some("22".to_string()));
    }

    #[test]
    fn test_runtime_version_missing_name() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devEngines": {
                    "runtime": {
                        "version": ">=20.0.0"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.runtime_version("node"), None);
    }

    #[test]
    fn test_package_manager_version_dev_engines() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devEngines": {
                    "packageManager": {
                        "name": "pnpm",
                        "version": ">=9.0.0"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.package_manager_version("pnpm"), Some("9".to_string()));
        assert_eq!(pkg.package_manager_version("yarn"), None);
    }

    #[test]
    fn test_package_manager_version_field() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "packageManager": "pnpm@9.1.0+sha256.abcdef"
            }"#,
        )
        .unwrap();
        assert_eq!(
            pkg.package_manager_version("pnpm"),
            Some("9.1.0".to_string())
        );
        assert_eq!(pkg.package_manager_version("yarn"), None);
    }

    #[test]
    fn test_package_manager_version_no_hash() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "packageManager": "yarn@4.1.0"
            }"#,
        )
        .unwrap();
        assert_eq!(
            pkg.package_manager_version("yarn"),
            Some("4.1.0".to_string())
        );
    }

    #[test]
    fn test_dev_engines_overrides_package_manager_field() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devEngines": {
                    "packageManager": {
                        "name": "pnpm",
                        "version": "^10.0.0"
                    }
                },
                "packageManager": "pnpm@9.1.0"
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.package_manager_version("pnpm"), Some("10".to_string()));
    }

    #[test]
    fn test_missing_fields() {
        let pkg: PackageJson = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(pkg.runtime_version("node"), None);
        assert_eq!(pkg.package_manager_version("pnpm"), None);
    }

    #[test]
    fn test_empty_dev_engines() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devEngines": {}
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.runtime_version("node"), None);
        assert_eq!(pkg.package_manager_version("pnpm"), None);
    }

    #[test]
    fn test_bun_as_package_manager() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "packageManager": "bun@1.2.0"
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.runtime_version("bun"), None);
        assert_eq!(
            pkg.package_manager_version("bun"),
            Some("1.2.0".to_string())
        );
    }

    #[test]
    fn test_deno_dev_engines() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "deno",
                        "version": "1.40.0"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.runtime_version("deno"), Some("1.40.0".to_string()));
    }
}
