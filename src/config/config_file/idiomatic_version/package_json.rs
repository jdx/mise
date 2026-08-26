use crate::file;
use crate::toolset::ToolVersionOptions;
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

pub(crate) fn is_package_json(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|file_name| file_name == "package.json")
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
        // serde_json rejects a leading byte-order mark outright, which would fail the whole file
        // rather than just the version it declares.
        let pkg: PackageJsonData = serde_json::from_str(file::strip_utf8_bom(&contents))?;
        Ok(pkg)
    }

    /// Extract a runtime version for the given tool name.
    fn runtime_version(&self, tool_name: &str) -> Option<String> {
        self.dev_engines
            .as_ref()
            .and_then(|de| de.runtime.as_ref())
            .filter(|r| r.name.as_deref() == Some(tool_name))
            .and_then(|r| r.version.as_deref())
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    }

    /// Extract a package manager version and checksum from the same declaration.
    /// Checks devEngines.packageManager first, then falls back to the packageManager field.
    fn package_manager_spec(&self, tool_name: &str) -> Option<(String, Option<String>)> {
        self.dev_engine_package_manager_spec(tool_name)
            .or_else(|| self.top_level_package_manager_spec(tool_name))
    }

    fn dev_engine_package_manager_spec(&self, tool_name: &str) -> Option<(String, Option<String>)> {
        self.dev_engines
            .as_ref()
            .and_then(|de| de.package_manager.as_ref())
            .filter(|pm| pm.name.as_deref() == Some(tool_name))
            .and_then(|pm| pm.version.as_deref())
            .filter(|v| !v.is_empty())
            .and_then(parse_package_manager_version)
    }

    fn top_level_package_manager_spec(&self, tool_name: &str) -> Option<(String, Option<String>)> {
        let pm_field = self.package_manager.as_deref()?;
        let (name, rest) = pm_field.split_once('@')?;
        if name != tool_name {
            return None;
        }
        parse_package_manager_version(rest)
    }

    fn package_manager_checksum_for_version(
        &self,
        tool_name: &str,
        version: &str,
    ) -> Option<String> {
        [
            self.dev_engine_package_manager_spec(tool_name),
            self.top_level_package_manager_spec(tool_name),
        ]
        .into_iter()
        .flatten()
        .find_map(|(candidate, checksum)| (candidate == version).then_some(checksum).flatten())
    }

    #[cfg(test)]
    fn package_manager_version(&self, tool_name: &str) -> Option<String> {
        self.package_manager_spec(tool_name)
            .map(|(version, _)| version)
    }
}

fn parse_package_manager_version(raw: &str) -> Option<(String, Option<String>)> {
    let version = raw.split('+').next().unwrap_or(raw).trim();
    if version.is_empty() {
        return None;
    }
    Some((version.to_string(), checksum_from_version(raw)))
}

fn checksum_from_version(raw: &str) -> Option<String> {
    let (_, checksum) = raw.split_once('+')?;
    if let Some((algorithm, digest)) = checksum.split_once('.') {
        return Some(format!("{algorithm}:{digest}"));
    }
    Some(checksum.to_string())
}

pub(crate) fn parse_with_options(
    path: &Path,
    tool_name: &str,
) -> Result<Vec<(String, Option<ToolVersionOptions>)>> {
    let pkg = PackageJsonData::parse(path)?;
    let (version, checksum) = match tool_name {
        "node" | "deno" => pkg
            .runtime_version(tool_name)
            .map(|version| (version, None)),
        "bun" => pkg
            .runtime_version(tool_name)
            .map(|version| {
                let checksum = pkg.package_manager_checksum_for_version(tool_name, &version);
                (version, checksum)
            })
            .or_else(|| pkg.package_manager_spec(tool_name)),
        "npm" | "yarn" | "pnpm" => pkg.package_manager_spec(tool_name),
        _ => None,
    }
    .unwrap_or_default();
    if version.is_empty() {
        return Ok(vec![]);
    }

    let options = checksum.map(|checksum| {
        let mut options = ToolVersionOptions::default();
        options.opts.insert(
            "package_manager_checksum".to_string(),
            toml::Value::String(checksum),
        );
        options
    });
    Ok(vec![(version, options)])
}

pub(crate) fn parse(path: &Path, tool_name: &str) -> Result<Vec<String>> {
    Ok(parse_with_options(path, tool_name)?
        .into_iter()
        .map(|(version, _)| version)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

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
    fn test_parse_package_json_with_byte_order_mark() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        // serde_json rejects the mark, which used to fail the file rather than the field.
        fs::write(&path, "\u{feff}{\"packageManager\": \"pnpm@9.0.0\"}").unwrap();

        assert_eq!(parse(&path, "pnpm").unwrap(), vec!["9.0.0".to_string()]);
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
    fn test_package_manager_checksum_becomes_install_option() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(&path, r#"{"packageManager":"pnpm@9.1.0+sha224.abcdef"}"#).unwrap();

        let parsed = parse_with_options(&path, "pnpm").unwrap();
        assert_eq!(parsed[0].0, "9.1.0");
        assert_eq!(
            parsed[0]
                .1
                .as_ref()
                .and_then(|options| options.opts.get("package_manager_checksum"))
                .and_then(toml::Value::as_str),
            Some("sha224:abcdef")
        );
    }

    #[test]
    fn test_dev_engines_package_manager_checksum_becomes_install_option() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(
            &path,
            r#"{
                "devEngines": {
                    "packageManager": {
                        "name": "yarn",
                        "version": "4.1.0+sha512.abcdef"
                    }
                }
            }"#,
        )
        .unwrap();

        let parsed = parse_with_options(&path, "yarn").unwrap();
        assert_eq!(parsed[0].0, "4.1.0");
        assert_eq!(
            parsed[0]
                .1
                .as_ref()
                .and_then(|options| options.opts.get("package_manager_checksum"))
                .and_then(toml::Value::as_str),
            Some("sha512:abcdef")
        );
    }

    #[test]
    fn test_package_manager_checksum_uses_selected_declaration() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(
            &path,
            r#"{
                "devEngines": {
                    "packageManager": {
                        "name": "pnpm",
                        "version": "10.0.0"
                    }
                },
                "packageManager": "pnpm@9.0.0+sha224.unrelated"
            }"#,
        )
        .unwrap();

        let parsed = parse_with_options(&path, "pnpm").unwrap();
        assert_eq!(parsed, vec![("10.0.0".to_string(), None)]);
    }

    #[test]
    fn test_bun_runtime_uses_matching_package_manager_checksum() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(
            &path,
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "bun",
                        "version": "1.3.14"
                    }
                },
                "packageManager": "bun@1.3.14+sha224.abcdef"
            }"#,
        )
        .unwrap();

        let parsed = parse_with_options(&path, "bun").unwrap();
        assert_eq!(parsed[0].0, "1.3.14");
        assert_eq!(
            parsed[0]
                .1
                .as_ref()
                .and_then(|options| options.opts.get("package_manager_checksum"))
                .and_then(toml::Value::as_str),
            Some("sha224:abcdef")
        );
    }

    #[test]
    fn test_bun_runtime_ignores_different_package_manager_checksum() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(
            &path,
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "bun",
                        "version": "1.3.14"
                    }
                },
                "packageManager": "bun@1.3.13+sha224.unrelated"
            }"#,
        )
        .unwrap();

        assert_eq!(
            parse_with_options(&path, "bun").unwrap(),
            vec![("1.3.14".to_string(), None)]
        );
    }

    #[test]
    fn test_bun_runtime_uses_unshadowed_matching_checksum() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(
            &path,
            r#"{
                "devEngines": {
                    "runtime": {
                        "name": "bun",
                        "version": "1.3.14"
                    },
                    "packageManager": {
                        "name": "bun",
                        "version": "1.3.13+sha224.unrelated"
                    }
                },
                "packageManager": "bun@1.3.14+sha224.abcdef"
            }"#,
        )
        .unwrap();

        let parsed = parse_with_options(&path, "bun").unwrap();
        assert_eq!(parsed[0].0, "1.3.14");
        assert_eq!(
            parsed[0]
                .1
                .as_ref()
                .and_then(|options| options.opts.get("package_manager_checksum"))
                .and_then(toml::Value::as_str),
            Some("sha224:abcdef")
        );
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
