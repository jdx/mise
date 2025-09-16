use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable must be set");
    generate_baked_registry(&out_dir);
}
fn generate_baked_registry(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("aqua_standard_registry.rs");

    // Look for the aqua-registry directory in the workspace root
    let registry_dir = find_registry_dir()
        .expect("Could not find aqua-registry directory in workspace root. Expected to find it at workspace_root/aqua-registry/");

    let registries =
        collect_aqua_registries(&registry_dir).expect("Failed to collect aqua registry files");

    if registries.is_empty() {
        panic!(
            "No aqua registry files found in {}/pkgs/",
            registry_dir.display()
        );
    }

    let mut code = String::from("HashMap::from([\n");
    for (id, content) in registries {
        code.push_str(&format!("    ({:?}, {:?}),\n", id, content));
    }
    code.push_str("])");

    fs::write(dest_path, code).expect("Failed to write baked registry file");
}

fn find_registry_dir() -> Option<std::path::PathBuf> {
    // Prefer the registry embedded within this crate: crates/aqua-registry/aqua-registry
    if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let embedded = std::path::Path::new(&manifest_dir).join("aqua-registry");
        if embedded.exists() {
            return Some(embedded);
        }
    }

    let current_dir = env::current_dir().ok()?;

    // Look for the workspace root by finding a Cargo.toml that contains [workspace]
    let workspace_root = current_dir.ancestors().find(|dir| {
        let cargo_toml = dir.join("Cargo.toml");
        if !cargo_toml.exists() {
            return false;
        }
        // Check if this Cargo.toml defines a workspace
        if let Ok(content) = fs::read_to_string(&cargo_toml) {
            content.contains("[workspace]")
        } else {
            false
        }
    })?;

    let aqua_registry = workspace_root.join("aqua-registry");
    if aqua_registry.exists() {
        return Some(aqua_registry);
    }

    None
}

fn collect_aqua_registries(
    dir: &Path,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut registries = Vec::new();

    if !dir.exists() {
        return Ok(registries);
    }

    let pkgs_dir = dir.join("pkgs");
    if !pkgs_dir.exists() {
        return Ok(registries);
    }

    collect_registries_recursive(&pkgs_dir, &mut registries, String::new())?;
    Ok(registries)
}

fn collect_registries_recursive(
    dir: &Path,
    registries: &mut Vec<(String, String)>,
    prefix: String,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let dir_name = path.file_name().unwrap().to_string_lossy();
            let new_prefix = if prefix.is_empty() {
                dir_name.to_string()
            } else {
                format!("{}/{}", prefix, dir_name)
            };
            collect_registries_recursive(&path, registries, new_prefix)?;
        } else if path.file_name() == Some(std::ffi::OsStr::new("registry.yaml")) {
            let content = fs::read_to_string(&path)?;
            registries.push((prefix.clone(), content.clone()));

            // Process aliases if they exist
            #[allow(clippy::collapsible_if)]
            if content.contains("aliases") {
                if let Ok(registry) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                    if let Some(packages) = registry.get("packages").and_then(|p| p.as_sequence()) {
                        for package in packages {
                            if let Some(aliases) =
                                package.get("aliases").and_then(|a| a.as_sequence())
                            {
                                for alias in aliases {
                                    if let Some(name) = alias.get("name").and_then(|n| n.as_str()) {
                                        registries.push((name.to_string(), content.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
