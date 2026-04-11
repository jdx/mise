use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable must be set");
    generate_baked_registry(&out_dir);
}
fn generate_baked_registry(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("aqua_standard_registry.rs");

    let registry_file = find_registry_file();

    println!("cargo:rerun-if-changed={}", registry_file.display());

    let content = fs::read_to_string(&registry_file).unwrap_or_else(|e| {
        panic!(
            "Failed to read aqua registry file {}: {e}",
            registry_file.display()
        )
    });

    let registry = serde_yaml::from_str::<serde_yaml::Value>(&content).unwrap_or_else(|e| {
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
    if packages.is_empty() {
        panic!(
            "Aqua registry file {} contains no packages",
            registry_file.display()
        );
    }

    fs::write(dest_path, format!("{content:?}")).expect("Failed to write baked registry file");
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
