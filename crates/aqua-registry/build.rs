use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::Path;

use serde_yaml::Value;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable must be set");
    generate_baked_registry(&out_dir);
    generate_registry_metadata(&out_dir);
}

#[derive(Debug)]
struct PackageRegistry {
    id: String,
    content: String,
    aliases: Vec<String>,
}

fn generate_baked_registry(out_dir: &str) {
    let files_dest_path = Path::new(out_dir).join("aqua_standard_registry_files.rs");
    let aliases_dest_path = Path::new(out_dir).join("aqua_standard_registry_aliases.rs");

    let registry_file = find_registry_file();

    println!("cargo:rerun-if-changed={}", registry_file.display());

    let content = fs::read_to_string(&registry_file).unwrap_or_else(|e| {
        panic!(
            "Failed to read aqua registry file {}: {e}",
            registry_file.display()
        )
    });

    let registry = serde_yaml::from_str::<Value>(&content).unwrap_or_else(|e| {
        panic!(
            "Failed to parse aqua registry file {}: {e}",
            registry_file.display()
        )
    });
    let packages = registry
        .get("packages")
        .and_then(|packages| packages.as_sequence())
        .unwrap_or_else(|| {
            panic!(
                "Aqua registry file {} does not contain a packages list",
                registry_file.display()
            )
        });
    let registries = package_registries(packages);
    if registries.is_empty() {
        panic!(
            "Aqua registry file {} contains no packages",
            registry_file.display()
        );
    }

    fs::write(files_dest_path, registry_files_code(&registries))
        .expect("Failed to write baked registry files");
    fs::write(aliases_dest_path, registry_aliases_code(&registries))
        .expect("Failed to write baked registry aliases");
}

fn generate_registry_metadata(out_dir: &str) {
    let metadata_dest_path = Path::new(out_dir).join("aqua_standard_registry_metadata.rs");
    let metadata_file = find_registry_metadata_file();

    println!("cargo:rerun-if-changed={}", metadata_file.display());

    let content = fs::read_to_string(&metadata_file).unwrap_or_else(|e| {
        panic!(
            "Failed to read aqua registry metadata file {}: {e}",
            metadata_file.display()
        )
    });
    let metadata = serde_yaml::from_str::<Value>(&content).unwrap_or_else(|e| {
        panic!(
            "Failed to parse aqua registry metadata file {}: {e}",
            metadata_file.display()
        )
    });
    let repository = string_field(&metadata, "repository").unwrap_or_else(|| {
        panic!(
            "Aqua registry metadata file {} does not contain a repository",
            metadata_file.display()
        )
    });
    let tag = string_field(&metadata, "tag").unwrap_or_else(|| {
        panic!(
            "Aqua registry metadata file {} does not contain a tag",
            metadata_file.display()
        )
    });

    fs::write(
        metadata_dest_path,
        format!("AquaRegistryMetadata {{ repository: {repository:?}, tag: {tag:?} }}"),
    )
    .expect("Failed to write baked registry metadata");
}

fn package_registries(packages: &[Value]) -> Vec<PackageRegistry> {
    packages
        .iter()
        .filter_map(|package| {
            let id = canonical_package_id(package)?;
            let content = package_registry_yaml(package);
            let aliases = package_aliases(package);
            Some(PackageRegistry {
                id,
                content,
                aliases,
            })
        })
        .collect()
}

fn registry_files_code(registries: &[PackageRegistry]) -> String {
    let mut entries = registries
        .iter()
        .map(|registry| (registry.id.clone(), registry.content.clone()))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    registry_map_code(&entries)
}

fn registry_aliases_code(registries: &[PackageRegistry]) -> String {
    let canonical_ids = registries
        .iter()
        .map(|registry| registry.id.as_str())
        .collect::<HashSet<_>>();
    let mut aliases = HashMap::new();

    for registry in registries {
        for alias in &registry.aliases {
            if alias != &registry.id && !canonical_ids.contains(alias.as_str()) {
                aliases.insert(alias.clone(), registry.id.clone());
            }
        }
    }

    let mut entries = aliases.into_iter().collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    registry_map_code(&entries)
}

fn registry_map_code(entries: &[(String, String)]) -> String {
    let mut code = String::from("HashMap::from([\n");
    for (key, value) in entries {
        code.push_str(&format!("    ({key:?}, {value:?}),\n"));
    }
    code.push_str("])");
    code
}

fn package_registry_yaml(package: &Value) -> String {
    let mut registry = serde_yaml::Mapping::new();
    registry.insert(
        Value::String("packages".to_string()),
        Value::Sequence(vec![package.clone()]),
    );
    serde_yaml::to_string(&Value::Mapping(registry))
        .expect("Failed to serialize aqua package registry")
}

fn canonical_package_id(package: &Value) -> Option<String> {
    string_field(package, "name").or_else(|| {
        let repo_owner = string_field(package, "repo_owner")?;
        let repo_name = string_field(package, "repo_name")?;
        Some(format!("{repo_owner}/{repo_name}"))
    })
}

fn package_aliases(package: &Value) -> Vec<String> {
    package
        .get("aliases")
        .and_then(|aliases| aliases.as_sequence())
        .map(|aliases| {
            aliases
                .iter()
                .filter_map(|alias| string_field(alias, "name"))
                .collect()
        })
        .unwrap_or_default()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn find_registry_file() -> std::path::PathBuf {
    // Registry location is constant: crates/aqua-registry/aqua-registry
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable must be set");
    let embedded = std::path::Path::new(&manifest_dir).join("aqua-registry/registry.yaml");
    if embedded.exists() {
        return embedded;
    }
    panic!("Registry file not found at {}", embedded.display());
}

fn find_registry_metadata_file() -> std::path::PathBuf {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable must be set");
    let embedded = std::path::Path::new(&manifest_dir).join("aqua-registry/metadata.json");
    if embedded.exists() {
        return embedded;
    }
    panic!("Registry metadata file not found at {}", embedded.display());
}
