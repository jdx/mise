use std::path::Path;

use color_eyre::eyre::Result;

use crate::{dirs, file};

pub fn run() -> Result<()> {
    move_subdirs(&dirs::INSTALLS.join("nodejs"), &dirs::INSTALLS.join("node"))?;
    move_subdirs(&dirs::INSTALLS.join("golang"), &dirs::INSTALLS.join("go"))?;
    move_subdirs(&dirs::PLUGINS.join("nodejs"), &dirs::PLUGINS.join("node"))?;
    move_subdirs(&dirs::PLUGINS.join("golang"), &dirs::PLUGINS.join("go"))?;
    move_trusted_configs()?;

    Ok(())
}

fn move_subdirs(from: &Path, to: &Path) -> Result<()> {
    if from.exists() {
        info!("migrating {} to {}", from.display(), to.display());
        file::create_dir_all(to)?;
        for f in from.read_dir()? {
            let f = f?.file_name();
            let from_file = from.join(&f);
            let to_file = to.join(&f);
            if !to_file.exists() {
                debug!("moving {} to {}", from_file.display(), to_file.display());
                file::rename(from_file, to_file)?;
            }
        }
        file::remove_all(from)?;
    }

    Ok(())
}

fn move_trusted_configs() -> Result<()> {
    let from = dirs::CACHE.join("trusted-configs");
    let to = dirs::CONFIG.join("trusted-configs");
    if from.exists() && !to.exists() {
        info!("migrating {} to {}", from.display(), to.display());
        file::create_dir_all(to.parent().unwrap())?;
        file::rename(from, to)?;
    }
    Ok(())
}
