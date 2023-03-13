use std::fs;
use std::path::Path;

use color_eyre::eyre::{eyre, Result};
use rust_embed::RustEmbed;
use tar::Archive;
use xz::read::XzDecoder;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/core_plugins"]
#[include = "*.tar.xz"]
struct CorePlugins;

pub fn has_plugin(name: &str) -> bool {
    CorePlugins::get(&to_tar_xz(name)).is_some()
}

pub fn install_plugin(name: &str, path: &Path) -> Result<()> {
    let tar_xz = CorePlugins::get(&to_tar_xz(name)).ok_or(eyre!("core plugin {name} not found"))?;
    let tar = XzDecoder::new(&tar_xz.data[..]);
    let mut archive = Archive::new(tar);
    archive.unpack(path.parent().unwrap())?;
    fs::write(path.join(".rtx-core"), env!("CARGO_PKG_VERSION"))?;
    Ok(())
}

fn to_tar_xz(name: &str) -> String {
    name.to_owned() + ".tar.xz"
}
