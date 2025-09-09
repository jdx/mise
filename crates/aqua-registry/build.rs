use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=registry.yaml");

    // Only bake if registry.yaml exists
    let registry_path = Path::new("registry.yaml");
    if !registry_path.exists() {
        // If no registry.yaml, create an empty baked registry
        create_empty_baked_registry();
        return;
    }

    bake_registry().expect("Failed to bake registry");
}

fn create_empty_baked_registry() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("aqua_baked.rs");

    let empty_registry_code = r#"
{
    use super::types::{RegistryIndex};
    use indexmap::IndexMap;
    
    RegistryIndex {
        packages_by_name: IndexMap::new(),
        aliases: IndexMap::new(),
    }
}
"#;

    fs::write(&dest_path, empty_registry_code).expect("Failed to write empty baked registry");
}

fn bake_registry() -> Result<(), Box<dyn std::error::Error>> {
    let registry_content = fs::read_to_string("registry.yaml")?;

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("aqua_baked.rs");

    // Generate Rust code for the baked registry
    let mut code_lines = vec![
        "{".to_string(),
        "    use super::types::{RegistryIndex, AquaPackage};".to_string(),
        "    use indexmap::IndexMap;".to_string(),
        "".to_string(),
        "    let registry_yaml = r###\"".to_string(),
    ];

    // Embed the YAML content as a string literal
    for line in registry_content.lines() {
        code_lines.push(line.to_string());
    }

    code_lines.extend(vec![
        "\"###;".to_string(),
        "".to_string(),
        "    let registry: super::types::RegistryYaml = serde_yaml::from_str(registry_yaml).unwrap();".to_string(),
        "    let mut packages_by_name = IndexMap::new();".to_string(),
        "    let mut aliases = IndexMap::new();".to_string(),
        "".to_string(),
        "    for package in registry.packages {".to_string(),
        "        let canonical_name = if !package.repo_owner.is_empty() && !package.repo_name.is_empty() {".to_string(),
        "            format!(\"{}/{}\", package.repo_owner, package.repo_name)".to_string(),
        "        } else if let Some(name) = &package.name {".to_string(),
        "            name.clone()".to_string(),
        "        } else {".to_string(),
        "            continue; // Skip packages without identifiable names".to_string(),
        "        };".to_string(),
        "        packages_by_name.insert(canonical_name, package);".to_string(),
        "    }".to_string(),
        "".to_string(),
        "    if let Some(registry_aliases) = registry.aliases {".to_string(),
        "        for alias in registry_aliases {".to_string(),
        "            aliases.insert(alias.name, alias.package);".to_string(),
        "        }".to_string(),
        "    }".to_string(),
        "".to_string(),
        "    RegistryIndex {".to_string(),
        "        packages_by_name,".to_string(),
        "        aliases,".to_string(),
        "    }".to_string(),
        "}".to_string(),
    ]);

    fs::write(&dest_path, code_lines.join("\n"))?;
    Ok(())
}
