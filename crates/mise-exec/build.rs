use std::path::Path;

/// Embed the workspace mise version: records are keyed by the version of the
/// mise binary that wrote them, and mise-exec ships alongside that binary.
fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let root = std::fs::read_to_string(&manifest).expect("read workspace Cargo.toml");
    let version = root
        .lines()
        .skip_while(|l| l.trim() != "[package]")
        .find_map(|l| {
            l.strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .expect("version in workspace Cargo.toml");
    println!("cargo:rustc-env=MISE_PAIRED_VERSION={version}");
}
