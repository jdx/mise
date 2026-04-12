use crate::file;
use eyre::Result;
use serde::Deserialize;
use serde::de::Deserializer;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageJsonData {
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

impl PackageJsonData {
    fn parse(path: &Path) -> Result<Self> {
        let contents = file::read_to_string(path)?;
        let pkg: PackageJsonData = serde_json::from_str(&contents)?;
        Ok(pkg)
    }

    /// Extract a runtime version for the given tool name.
    fn runtime_version(&self, tool_name: &str) -> Option<String> {
        self.dev_engines
            .as_ref()
            .and_then(|de| de.runtime.as_ref())
            .filter(|r| r.name.as_deref() == Some(tool_name))
            .and_then(|r| r.version.as_deref())
            .map(normalize_semver_range)
            .filter(|v| !v.is_empty())
    }

    /// Extract a package manager version for the given tool name.
    /// Checks devEngines.packageManager first, then falls back to the packageManager field.
    fn package_manager_version(&self, tool_name: &str) -> Option<String> {
        // Try devEngines.packageManager first
        self.dev_engines
            .as_ref()
            .and_then(|de| de.package_manager.as_ref())
            .filter(|pm| pm.name.as_deref() == Some(tool_name))
            .and_then(|pm| pm.version.as_deref())
            .map(normalize_semver_range)
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

/// Preserve npm semver ranges from package.json for resolution against the
/// backend's available versions.
fn normalize_semver_range(input: &str) -> String {
    input.trim().to_string()
}

pub fn parse(path: &Path, tool_name: &str) -> Result<Vec<String>> {
    let pkg = PackageJsonData::parse(path)?;
    // We ignore unknown tools in package.json
    let v = match tool_name {
        "node" | "deno" => pkg.runtime_version(tool_name),
        "bun" => pkg
            .runtime_version(tool_name)
            .or_else(|| pkg.package_manager_version(tool_name)),
        "npm" | "yarn" | "pnpm" => pkg.package_manager_version(tool_name),
        _ => None,
    };
    if let Some(v) = v {
        Ok(vec![v])
    } else {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_normalize_semver_range() {
        assert_eq!(normalize_semver_range(" >=18.0.0 "), ">=18.0.0");
        assert_eq!(normalize_semver_range("^20.0.0"), "^20.0.0");
        assert_eq!(normalize_semver_range("~18.2.0"), "~18.2.0");
        assert_eq!(normalize_semver_range("9.1.0"), "9.1.0");
        assert_eq!(normalize_semver_range("18"), "18");
        assert_eq!(normalize_semver_range("*"), "*");
        assert_eq!(normalize_semver_range("x"), "x");
        assert_eq!(
            normalize_semver_range(">=20 <21 || >=22"),
            ">=20 <21 || >=22"
        );
    }

    #[test]
    fn test_parse_package_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(
            &path,
            r#"{
                "devEngines": {
                    "packageManager": {
                        "name": "yarn",
                        "version": "1.22.19"
                    },
                    "runtime": {
                        "name": "node",
                        "version": "20.0.0"
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(parse(&path, "yarn").unwrap(), vec!["1.22.19".to_string()]);
        assert_eq!(parse(&path, "node").unwrap(), vec!["20.0.0".to_string()]);
    }

    #[test]
    fn test_bun_logic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(
            &path,
            r#"{
                "packageManager": "bun@1.0.0"
            }"#,
        )
        .unwrap();

        assert_eq!(parse(&path, "bun").unwrap(), vec!["1.0.0".to_string()]);
        assert_eq!(parse(&path, "node").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn test_normalize_semver_range_upper_bound() {
        assert_eq!(normalize_semver_range("<18.0.0"), "<18.0.0");
        assert_eq!(normalize_semver_range("<=18.0.0"), "<=18.0.0");
    }

    #[test]
    fn test_normalize_semver_range_wildcards() {
        assert_eq!(normalize_semver_range("18.x"), "18.x");
        assert_eq!(normalize_semver_range("18.*"), "18.*");
        assert_eq!(normalize_semver_range("18.2.x"), "18.2.x");
        assert_eq!(normalize_semver_range("18.2.*"), "18.2.*");
    }

    #[test]
    fn test_runtime_version() {
        let pkg: PackageJsonData = serde_json::from_str(
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
        assert_eq!(pkg.runtime_version("node"), Some(">=20.0.0".to_string()));
        assert_eq!(pkg.runtime_version("bun"), None);
    }

    #[test]
    fn test_runtime_version_lower_bound_range() {
        let pkg: PackageJsonData = serde_json::from_str(
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "node",
                        "version": ">=25.6.1"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(pkg.runtime_version("node"), Some(">=25.6.1".to_string()));
    }

    #[test]
    fn test_runtime_version_compound_range() {
        let pkg: PackageJsonData = serde_json::from_str(
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "node",
                        "version": ">=20 <21 || >=22"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            pkg.runtime_version("node"),
            Some(">=20 <21 || >=22".to_string())
        );
    }

    #[test]
    fn test_runtime_version_bun() {
        let pkg: PackageJsonData = serde_json::from_str(
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
        assert_eq!(pkg.runtime_version("bun"), Some("^1.0.0".to_string()));
        assert_eq!(pkg.runtime_version("node"), None);
    }

    #[test]
    fn test_runtime_version_array_form() {
        let pkg: PackageJsonData = serde_json::from_str(
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
        assert_eq!(pkg.runtime_version("node"), Some(">=22.0.0".to_string()));
    }

    #[test]
    fn test_runtime_version_missing_name() {
        let pkg: PackageJsonData = serde_json::from_str(
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
        let pkg: PackageJsonData = serde_json::from_str(
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
        assert_eq!(
            pkg.package_manager_version("pnpm"),
            Some(">=9.0.0".to_string())
        );
        assert_eq!(pkg.package_manager_version("yarn"), None);
    }

    #[test]
    fn test_package_manager_version_dev_engines_lower_bound_range() {
        let pkg: PackageJsonData = serde_json::from_str(
            r#"{
                "devEngines": {
                    "packageManager": {
                        "name": "yarn",
                        "version": ">=4.12.0"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            pkg.package_manager_version("yarn"),
            Some(">=4.12.0".to_string())
        );
    }

    #[test]
    fn test_package_manager_version_field() {
        let pkg: PackageJsonData = serde_json::from_str(
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
        let pkg: PackageJsonData = serde_json::from_str(
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
        let pkg: PackageJsonData = serde_json::from_str(
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
        assert_eq!(
            pkg.package_manager_version("pnpm"),
            Some("^10.0.0".to_string())
        );
    }

    #[test]
    fn test_missing_fields() {
        let pkg: PackageJsonData = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(pkg.runtime_version("node"), None);
        assert_eq!(pkg.package_manager_version("pnpm"), None);
    }

    #[test]
    fn test_empty_dev_engines() {
        let pkg: PackageJsonData = serde_json::from_str(
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
        let pkg: PackageJsonData = serde_json::from_str(
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
        let pkg: PackageJsonData = serde_json::from_str(
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

    #[test]
    fn test_engines_field_ignored() {
        let pkg: PackageJsonData = serde_json::from_str(
            r#"{
                "engines": {
                    "node": ">=18.0.0",
                    "pnpm": "9.0.0"
                }
            }"#,
        )
        .unwrap();
        // Should ignore engines field
        assert_eq!(pkg.runtime_version("node"), None);
        assert_eq!(pkg.package_manager_version("pnpm"), None);
    }

    #[test]
    fn test_engines_field_does_not_interfere() {
        let pkg: PackageJsonData = serde_json::from_str(
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "node",
                        "version": "20.0.0"
                    }
                },
                "engines": {
                    "node": "18.0.0"
                }
            }"#,
        )
        .unwrap();
        // Should ignore engines and pick devEngines
        assert_eq!(pkg.runtime_version("node"), Some("20.0.0".to_string()));
    }
}
