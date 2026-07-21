use crate::codec::{decode_package_rkyv, encode_package_rkyv};
use crate::types::{AquaPackage, RegistryYaml};
use crate::{AquaRegistryError, Result};
use rkyv::rancor::Error as RkyvError;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const INDEX_FILE: &str = "index.rkyv";
const PACKAGES_DIR: &str = "packages";

#[derive(Debug, Clone)]
pub struct CompiledRegistry {
    root: PathBuf,
    index: CompiledRegistryIndex,
}

#[derive(Debug, Clone)]
pub struct ParsedRegistry {
    packages: HashMap<String, AquaPackage>,
    aliases: HashMap<String, String>,
}

#[derive(Debug, Clone, Archive, RkyvDeserialize, RkyvSerialize)]
struct CompiledRegistryIndex {
    packages: HashMap<String, String>,
    aliases: HashMap<String, String>,
}

impl CompiledRegistry {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let index = read_index(&root)?;
        validate_package_files(&root, &index)?;
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
        decode_package_rkyv(resolved_id, &bytes)
    }
}

impl ParsedRegistry {
    pub fn parse_yaml(source: &str) -> Result<Self> {
        let registry_yaml = serde_yaml::from_str::<RegistryYaml>(source)?;
        Self::from_registry_yaml(registry_yaml)
    }

    pub fn package(&self, package_id: &str) -> Result<AquaPackage> {
        let resolved_id = self
            .aliases
            .get(package_id)
            .map_or(package_id, String::as_str);
        self.packages
            .get(resolved_id)
            .cloned()
            .ok_or_else(|| AquaRegistryError::PackageNotFound(package_id.to_string()))
    }

    pub fn write_compiled_cache(&self, root: impl AsRef<Path>) -> Result<CompiledRegistry> {
        let root = root.as_ref().to_path_buf();
        let index = write_compiled_index(self, &root)?;
        Ok(CompiledRegistry { root, index })
    }

    fn from_registry_yaml(registry_yaml: RegistryYaml) -> Result<Self> {
        let package_entries = registry_yaml
            .packages
            .into_iter()
            .filter_map(|row| canonical_package_id(&row.package).map(|id| (id, row)))
            .collect::<Vec<_>>();

        if package_entries.is_empty() {
            return Err(AquaRegistryError::RegistryNotAvailable(
                "aqua registry contains no packages".to_string(),
            ));
        }

        let canonical_ids = package_entries
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        let mut packages = HashMap::new();
        let mut aliases = HashMap::new();

        for (id, row) in package_entries {
            for alias in &row.aliases {
                if alias != &id && !canonical_ids.contains(alias.as_str()) {
                    aliases.insert(alias.clone(), id.clone());
                }
            }
            packages.insert(id, row.package);
        }

        Ok(Self { packages, aliases })
    }
}

fn read_index(root: &Path) -> Result<CompiledRegistryIndex> {
    let path = root.join(INDEX_FILE);
    let bytes = fs::read(&path)?;
    rkyv::from_bytes::<CompiledRegistryIndex, RkyvError>(&bytes).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to decode compiled aqua registry index {} from rkyv: {err}",
            path.display()
        ))
    })
}

fn validate_package_files(root: &Path, index: &CompiledRegistryIndex) -> Result<()> {
    let packages_dir = root.join(PACKAGES_DIR);
    for filename in index.packages.values() {
        let path = packages_dir.join(filename);
        if !path.is_file() {
            return Err(AquaRegistryError::RegistryNotAvailable(format!(
                "compiled aqua registry package file is missing: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn write_index(root: &Path, index: &CompiledRegistryIndex) -> Result<()> {
    let path = root.join(INDEX_FILE);
    let bytes = rkyv::to_bytes::<RkyvError>(index)
        .map(|bytes| bytes.to_vec())
        .map_err(|err| {
            AquaRegistryError::RegistryNotAvailable(format!(
                "failed to encode compiled aqua registry index {} as rkyv: {err}",
                path.display()
            ))
        })?;
    fs::write(path, bytes)?;
    Ok(())
}

fn write_compiled_index(registry: &ParsedRegistry, root: &Path) -> Result<CompiledRegistryIndex> {
    let packages_dir = root.join(PACKAGES_DIR);
    fs::create_dir_all(&packages_dir)?;

    let mut used_filenames = HashSet::new();
    let mut packages = HashMap::new();

    for (id, package) in &registry.packages {
        let filename = package_filename(id, &mut used_filenames);
        let path = packages_dir.join(&filename);
        let content = encode_package_rkyv(package)?;
        fs::write(path, content)?;
        packages.insert(id.clone(), filename);
    }

    let index = CompiledRegistryIndex {
        packages,
        aliases: registry.aliases.clone(),
    };
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

fn package_filename(id: &str, used_filenames: &mut HashSet<String>) -> String {
    let stem = package_filename_stem(id);
    let mut filename = format!("{stem}.rkyv");
    let mut suffix = 2;
    while !used_filenames.insert(filename.clone()) {
        filename = format!("{stem}-{suffix}.rkyv");
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

/// Hashes the canonical package ID with FNV-1a 64-bit to keep compiled cache
/// filenames deterministic. The sanitized ID prefix is only for readability.
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
    name: example/canonical-tool
    repo_owner: example
    repo_name: tool
    url: https://example.com/tool
    aliases:
      - name: example/tool-alias
    version_overrides:
      - aliases:
          - name: example/nested-alias
"#;

        let registry = compile_registry(source, &root);
        let package = registry.package("example/tool-alias").unwrap();

        assert_eq!(package.name.as_deref(), Some("example/canonical-tool"));
        assert_eq!(package.repo_owner, "example");
        assert_eq!(package.repo_name, "tool");
        assert!(registry.package("example/canonical-tool").is_ok());
        assert!(matches!(
            registry.package("example/tool"),
            Err(AquaRegistryError::PackageNotFound(_))
        ));
        assert!(matches!(
            registry.package("example/nested-alias"),
            Err(AquaRegistryError::PackageNotFound(_))
        ));
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
    fn parsed_registry_resolves_packages_before_cache_is_written() {
        let source = r#"
packages:
  - type: http
    name: example/canonical-tool
    url: https://example.com/tool
    aliases:
      - name: example/tool-alias
"#;

        let registry = ParsedRegistry::parse_yaml(source).unwrap();
        let package = registry.package("example/tool-alias").unwrap();

        assert_eq!(package.name.as_deref(), Some("example/canonical-tool"));
        assert!(matches!(
            registry.package("example/missing"),
            Err(AquaRegistryError::PackageNotFound(_))
        ));
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

        compile_registry(source, &root);
        let registry = CompiledRegistry::load(&root).unwrap();
        let package = registry.package("example/named-tool").unwrap();

        assert_eq!(package.name.as_deref(), Some("example/named-tool"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_rejects_missing_package_blob() {
        let root = temp_cache_dir("compiled-aqua-registry-missing-package");
        let source = r#"
packages:
  - type: http
    name: example/missing-package
    url: https://example.com/tool
"#;

        compile_registry(source, &root);
        let packages_dir = root.join(PACKAGES_DIR);
        let package_file = fs::read_dir(&packages_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(package_file).unwrap();

        let err = CompiledRegistry::load(&root).unwrap_err();
        assert!(matches!(err, AquaRegistryError::RegistryNotAvailable(_)));

        fs::remove_dir_all(root).unwrap();
    }

    fn compile_registry(source: &str, root: &Path) -> CompiledRegistry {
        ParsedRegistry::parse_yaml(source)
            .unwrap()
            .write_compiled_cache(root)
            .unwrap()
    }

    fn temp_cache_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{nanos}"))
    }
}
