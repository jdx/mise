use std::path::Path;

use crate::dirs::*;
use crate::file;
use eyre::Result;

pub async fn run() {
    tokio::join!(task(migrate_trusted_configs), task(migrate_tracked_configs),);
}

async fn task(job: impl FnOnce() -> Result<()> + Send + 'static) {
    if let Err(err) = job() {
        eprintln!("[WARN] migrate: {err}");
    }
}

fn migrate_tracked_configs() -> Result<()> {
    move_dirs(&DATA.join("tracked_config_files"), &TRACKED_CONFIGS)?;
    move_dirs(&DATA.join("tracked-config-files"), &TRACKED_CONFIGS)?;
    Ok(())
}

fn migrate_trusted_configs() -> Result<()> {
    move_dirs(&CACHE.join("trusted-configs"), &TRUSTED_CONFIGS)?;
    move_dirs(&CONFIG.join("trusted-configs"), &TRUSTED_CONFIGS)?;
    move_dirs(&DATA.join("trusted-configs"), &TRUSTED_CONFIGS)?;
    Ok(())
}

fn move_dirs(from: &Path, to: &Path) -> Result<bool> {
    if from.exists() && !to.exists() {
        eprintln!(
            "migrating {} to {}",
            file::display_path(from),
            file::display_path(to)
        );
        file::create_dir_all(to.parent().unwrap())?;
        file::rename(from, to)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
