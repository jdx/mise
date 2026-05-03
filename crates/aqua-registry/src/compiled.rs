use crate::codec::{decode_package_msgpack, encode_package_msgpack};
use crate::types::{AquaPackage, RegistryYaml};
use crate::{AquaRegistryError, Result};
use serde_derive::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const INDEX_FILE: &str = "index.msgpack";
const PACKAGES_DIR: &str = "packages";

#[derive(Debug, Clone)]
pub struct CompiledRegistry {
    root: PathBuf,
    index: CompiledRegistryIndex,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CompiledRegistryIndex {
    packages: HashMap<String, String>,
    aliases: HashMap<String, String>,
}

impl CompiledRegistry {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let index = read_index(&root)?;
        Ok(Self { root, index })
    }

    pub fn compile_from_yaml(source: &str, root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let index = compile_index(source, &root)?;
        Ok(Self { root, index })
    }

    pub fn package(&self, package_id: &str) -> Result<AquaPackage> {
        let resolved_id = self
            .index
            .aliases
            .get(package_id)
            .map_or(package_id, String::as_str);
        let filename = self
            .index
            .packages
            .get(resolved_id)
            .ok_or_else(|| AquaRegistryError::PackageNotFound(package_id.to_string()))?;
        let path = self.root.join(PACKAGES_DIR).join(filename);
        let bytes = fs::read(&path)?;
        decode_package_msgpack(resolved_id, &bytes)
    }
}

fn read_index(root: &Path) -> Result<CompiledRegistryIndex> {
    let path = root.join(INDEX_FILE);
    let bytes = fs::read(&path)?;
    rmp_serde::from_slice(&bytes).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to decode compiled aqua registry index {}: {err}",
            path.display()
        ))
    })
}

fn write_index(root: &Path, index: &CompiledRegistryIndex) -> Result<()> {
    let path = root.join(INDEX_FILE);
    let bytes = rmp_serde::to_vec_named(index).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to encode compiled aqua registry index {}: {err}",
            path.display()
        ))
    })?;
    fs::write(path, bytes)?;
    Ok(())
}

fn compile_index(source: &str, root: &Path) -> Result<CompiledRegistryIndex> {
    let registry_yaml = serde_yaml::from_str::<RegistryYaml>(source)?;
    let registry_value = serde_yaml::from_str::<Value>(source)?;
    let package_values = registry_value
        .get("packages")
        .and_then(|packages| packages.as_sequence())
        .ok_or_else(|| {
            AquaRegistryError::RegistryNotAvailable(
                "aqua registry does not contain a packages list".to_string(),
            )
        })?;

    if registry_yaml.packages.len() != package_values.len() {
        return Err(AquaRegistryError::RegistryNotAvailable(format!(
            "parsed aqua package count mismatch: RegistryYaml has {}, raw YAML has {}",
            registry_yaml.packages.len(),
            package_values.len()
        )));
    }

    let packages_dir = root.join(PACKAGES_DIR);
    fs::create_dir_all(&packages_dir)?;

    let package_entries = registry_yaml
        .packages
        .iter()
        .zip(package_values)
        .filter_map(|(package, package_value)| {
            canonical_package_id(package).map(|id| (id, package, package_value))
        })
        .collect::<Vec<_>>();

    if package_entries.is_empty() {
        return Err(AquaRegistryError::RegistryNotAvailable(
            "aqua registry contains no packages".to_string(),
        ));
    }

    let canonical_ids = package_entries
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect::<HashSet<_>>();
    let mut used_filenames = HashSet::new();
    let mut packages = HashMap::new();
    let mut aliases = HashMap::new();

    for (id, package, package_value) in package_entries {
        let filename = package_filename(&id, &mut used_filenames);
        let path = packages_dir.join(&filename);
        let content = encode_package_msgpack(package)?;
        fs::write(path, content)?;
        packages.insert(id.clone(), filename);

        for alias in package_aliases(package_value) {
            if alias != id && !canonical_ids.contains(alias.as_str()) {
                aliases.insert(alias, id.clone());
            }
        }
    }

    let index = CompiledRegistryIndex { packages, aliases };
    write_index(root, &index)?;
    Ok(index)
}

fn canonical_package_id(package: &AquaPackage) -> Option<String> {
    package
        .name
        .clone()
        .or_else(|| {
            if package.repo_owner.is_empty() || package.repo_name.is_empty() {
                None
            } else {
                Some(format!("{}/{}", package.repo_owner, package.repo_name))
            }
        })
        .or_else(|| package.path.clone())
}

fn package_aliases(package: &Value) -> Vec<String> {
    package
        .get("aliases")
        .and_then(|aliases| aliases.as_sequence())
        .map(|aliases| {
            aliases
                .iter()
                .filter_map(|alias| yaml_string_field(alias, "name"))
                .collect()
        })
        .unwrap_or_default()
}

fn yaml_string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn package_filename(id: &str, used_filenames: &mut HashSet<String>) -> String {
    let stem = package_filename_stem(id);
    let mut filename = format!("{stem}.msgpack");
    let mut suffix = 2;
    while !used_filenames.insert(filename.clone()) {
        filename = format!("{stem}-{suffix}.msgpack");
        suffix += 1;
    }
    filename
}

fn package_filename_stem(id: &str) -> String {
    let sanitized = sanitize_filename_prefix(id);
    let hash = fnv1a64(id);
    format!("{sanitized}-{hash:016x}")
}

fn sanitize_filename_prefix(id: &str) -> String {
    let mut prefix = String::new();
    for byte in id.bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric() {
            prefix.push(c.to_ascii_lowercase());
        } else {
            prefix.push('_');
        }
        if prefix.len() >= 80 {
            break;
        }
    }
    if prefix.is_empty() {
        "package".to_string()
    } else {
        prefix
    }
}

fn fnv1a64(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn compiles_flat_registry_cache_and_resolves_aliases() {
        let root = temp_cache_dir("compiled-aqua-registry");
        let source = r#"
packages:
  - type: http
    repo_owner: example
    repo_name: tool
    url: https://example.com/tool
    aliases:
      - name: example/tool-alias
"#;

        let registry = CompiledRegistry::compile_from_yaml(source, &root).unwrap();
        let package = registry.package("example/tool-alias").unwrap();

        assert_eq!(package.repo_owner, "example");
        assert_eq!(package.repo_name, "tool");
        assert!(root.join(INDEX_FILE).exists());

        let packages_dir = root.join(PACKAGES_DIR);
        let files = fs::read_dir(&packages_dir)
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].file_type().unwrap().is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_compiled_registry_without_reparsing_yaml() {
        let root = temp_cache_dir("compiled-aqua-registry-load");
        let source = r#"
packages:
  - type: http
    name: example/named-tool
    url: https://example.com/tool
"#;

        CompiledRegistry::compile_from_yaml(source, &root).unwrap();
        let registry = CompiledRegistry::load(&root).unwrap();
        let package = registry.package("example/named-tool").unwrap();

        assert_eq!(package.name.as_deref(), Some("example/named-tool"));

        fs::remove_dir_all(root).unwrap();
    }

    fn temp_cache_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{nanos}"))
    }
}
