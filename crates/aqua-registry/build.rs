use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable must be set");
    generate_baked_registry(&out_dir);
}
fn generate_baked_registry(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("aqua_standard_registry.rs");

    let registry_dir = find_registry_dir();

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

fn find_registry_dir() -> std::path::PathBuf {
    // Registry location is constant: crates/aqua-registry/aqua-registry
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR environment variable must be set");
    let embedded = std::path::Path::new(&manifest_dir).join("aqua-registry");
    if embedded.exists() {
        return embedded;
    }
    panic!("Registry directory not found");
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
