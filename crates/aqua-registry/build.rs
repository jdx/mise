use std::env;
use std::fs;
use std::path::Path;

fn main() {
    if let Ok(out_dir) = env::var("OUT_DIR") {
        generate_baked_registry(&out_dir);
    }
}
fn generate_baked_registry(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("aqua_standard_registry.rs");

    // Look for the aqua-registry directory in the workspace root or current directory
    let registry_dir = find_registry_dir();

    let mut code = String::from("HashMap::from([\n");

    if let Some(registry_dir) = registry_dir
        && let Ok(registries) = collect_aqua_registries(&registry_dir)
    {
        for (id, content) in registries {
            code.push_str(&format!("    ({:?}, {:?}),\n", id, content));
        }
    }

    code.push_str("])");

    fs::write(dest_path, code).expect("Failed to write baked registry file");
}

fn find_registry_dir() -> Option<std::path::PathBuf> {
    let current_dir = env::current_dir().ok()?;

    // Try the workspace root
    let workspace_root = current_dir
        .ancestors()
        .find(|dir| dir.join("Cargo.toml").exists())?;

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
            registries.push((prefix.clone(), content));
        }
    }
    Ok(())
}
