use std::env;
use std::fs;
use std::path::Path;

/// Embed the mise release this shim ships with, so mise can tell a stale `mise-shim.exe` template
/// from its own. The crate's own version is not that release: it stays put while mise's moves, so the
/// version comes from the workspace root `Cargo.toml` instead.
fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let root = Path::new(&manifest_dir).join("../..");
    let root_manifest = root.join("Cargo.toml");
    let prefix_file = root.join("src/assets/native-shim-version-prefix");
    println!("cargo:rerun-if-changed={}", root_manifest.display());
    println!("cargo:rerun-if-changed={}", prefix_file.display());

    let manifest = fs::read_to_string(&root_manifest)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", root_manifest.display()));
    let version = mise_version(&manifest)
        .unwrap_or_else(|| panic!("no [package] version in {}", root_manifest.display()));
    let prefix = fs::read_to_string(&prefix_file)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", prefix_file.display()));
    // `main.rs` appends the newline that closes the tag: a `rustc-env` line cannot carry one.
    println!(
        "cargo:rustc-env=MISE_SHIM_VERSION_TAG={}{version}",
        prefix.trim()
    );
}

/// The `version = "..."` of the `[package]` table, ignoring the `[workspace]` tables before it and
/// any `version = ` inside dependency tables after it.
fn mise_version(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package && let Some(rest) = line.strip_prefix("version") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                // Take what sits between the quotes, so `version = "1.0" # bump` yields `1.0`.
                let quoted = rest.trim().strip_prefix('"')?;
                return quoted
                    .split_once('"')
                    .map(|(version, _)| version.to_string());
            }
        }
    }
    None
}
