use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/core_plugins"]
#[include = "*.tar.xz"]
struct CorePlugins;

pub fn list_assets() {
    for p in CorePlugins::iter() {
        dbg!(p);
    }
}
