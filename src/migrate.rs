use std::path::Path;

use color_eyre::eyre::Result;

use crate::dirs::{CACHE, CONFIG, INSTALLS, PLUGINS, ROOT};
use crate::file;

pub fn run() -> Result<()> {
    move_subdirs(&INSTALLS.join("nodejs"), &INSTALLS.join("node"))?;
    move_subdirs(&INSTALLS.join("golang"), &INSTALLS.join("go"))?;
    move_subdirs(&PLUGINS.join("nodejs"), &PLUGINS.join("node"))?;
    move_subdirs(&PLUGINS.join("golang"), &PLUGINS.join("go"))?;
    move_dirs(
        &CACHE.join("trusted-configs"),
        &ROOT.join("trusted-configs"),
    )?;
    move_dirs(
        &CONFIG.join("trusted-configs"),
        &ROOT.join("trusted-configs"),
    )?;

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

fn move_dirs(from: &Path, to: &Path) -> Result<()> {
    if from.exists() && !to.exists() {
        info!("migrating {} to {}", from.display(), to.display());
        file::create_dir_all(to.parent().unwrap())?;
        file::rename(from, to)?;
    }
    Ok(())
}
