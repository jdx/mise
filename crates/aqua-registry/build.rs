use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, eyre};
use serde_yaml::Value;

fn main() -> Result<()> {
    let out_dir = env::var("OUT_DIR").wrap_err("OUT_DIR environment variable must be set")?;
    generate_baked_registry(&out_dir)?;
    generate_registry_metadata(&out_dir)?;
    Ok(())
}

#[derive(Debug)]
struct PackageRegistry {
    id: String,
    content: String,
    aliases: Vec<String>,
}

fn generate_baked_registry(out_dir: &str) -> Result<()> {
    let files_dest_path = Path::new(out_dir).join("aqua_standard_registry_files.rs");
    let aliases_dest_path = Path::new(out_dir).join("aqua_standard_registry_aliases.rs");

    let registry_file = find_registry_file()?;

    println!("cargo:rerun-if-changed={}", registry_file.display());

    let content = fs::read_to_string(&registry_file).wrap_err_with(|| {
        format!(
            "Failed to read aqua registry file {}",
            registry_file.display()
        )
    })?;

    let registry = serde_yaml::from_str::<Value>(&content).wrap_err_with(|| {
        format!(
            "Failed to parse aqua registry file {}",
            registry_file.display()
        )
    })?;
    let packages = registry
        .get("packages")
        .and_then(|packages| packages.as_sequence())
        .ok_or_else(|| {
            eyre!(
                "Aqua registry file {} does not contain a packages list",
                registry_file.display()
            )
        })?;
    let registries = package_registries(packages)?;
    if registries.is_empty() {
        return Err(eyre!(
            "Aqua registry file {} contains no packages",
            registry_file.display()
        ));
    }

    fs::write(files_dest_path, registry_files_code(&registries))
        .wrap_err("Failed to write baked registry files")?;
    fs::write(aliases_dest_path, registry_aliases_code(&registries))
        .wrap_err("Failed to write baked registry aliases")?;
    Ok(())
}

fn generate_registry_metadata(out_dir: &str) -> Result<()> {
    let metadata_dest_path = Path::new(out_dir).join("aqua_standard_registry_metadata.rs");
    let metadata_file = find_registry_metadata_file()?;

    println!("cargo:rerun-if-changed={}", metadata_file.display());

    let content = fs::read_to_string(&metadata_file).wrap_err_with(|| {
        format!(
            "Failed to read aqua registry metadata file {}",
            metadata_file.display()
        )
    })?;
    let metadata = serde_yaml::from_str::<Value>(&content).wrap_err_with(|| {
        format!(
            "Failed to parse aqua registry metadata file {}",
            metadata_file.display()
        )
    })?;
    let repository = string_field(&metadata, "repository").ok_or_else(|| {
        eyre!(
            "Aqua registry metadata file {} does not contain a repository",
            metadata_file.display()
        )
    })?;
    let tag = string_field(&metadata, "tag").ok_or_else(|| {
        eyre!(
            "Aqua registry metadata file {} does not contain a tag",
            metadata_file.display()
        )
    })?;

    fs::write(
        metadata_dest_path,
        format!("AquaRegistryMetadata {{ repository: {repository:?}, tag: {tag:?} }}"),
    )
    .wrap_err("Failed to write baked registry metadata")?;
    Ok(())
}

fn package_registries(packages: &[Value]) -> Result<Vec<PackageRegistry>> {
    let mut registries = Vec::new();
    for package in packages {
        let Some(id) = canonical_package_id(package) else {
            continue;
        };
        let content = package_registry_yaml(package)?;
        let aliases = package_aliases(package);
        registries.push(PackageRegistry {
            id,
            content,
            aliases,
        });
    }
    Ok(registries)
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

fn package_registry_yaml(package: &Value) -> Result<String> {
    let mut registry = serde_yaml::Mapping::new();
    registry.insert(
        Value::String("packages".to_string()),
        Value::Sequence(vec![package.clone()]),
    );
    serde_yaml::to_string(&Value::Mapping(registry))
        .wrap_err("Failed to serialize aqua package registry")
}

fn canonical_package_id(package: &Value) -> Option<String> {
    string_field(package, "name")
        .or_else(|| {
            let repo_owner = string_field(package, "repo_owner")?;
            let repo_name = string_field(package, "repo_name")?;
            Some(format!("{repo_owner}/{repo_name}"))
        })
        .or_else(|| string_field(package, "path"))
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

fn find_registry_file() -> Result<PathBuf> {
    registry_file("registry.yaml", "Registry file")
}

fn find_registry_metadata_file() -> Result<PathBuf> {
    registry_file("metadata.json", "Registry metadata file")
}

fn registry_file(file_name: &str, description: &str) -> Result<PathBuf> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .wrap_err("CARGO_MANIFEST_DIR environment variable must be set")?;
    let embedded = Path::new(&manifest_dir)
        .join("aqua-registry")
        .join(file_name);
    if embedded.exists() {
        return Ok(embedded);
    }
    Err(eyre!("{description} not found at {}", embedded.display()))
}
