use std::path::Path;

use crate::dirs::*;
use crate::file;
use eyre::Result;

pub async fn run() {
    tokio::join!(
        task(migrate_trusted_configs),
        task(migrate_tracked_configs),
        task(migrate_incomplete_markers),
    );
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

remove_by!("2027.1.0", "legacy-incomplete-markers");

/// mise 2026.10.3 and earlier kept incomplete-install markers in the cache,
/// where `mise cache clear` and pruning delete them. Moves any left there to
/// the state dir, once.
fn migrate_incomplete_markers() -> Result<()> {
    use crate::toolset::install_state;
    let done = install_state::legacy_incomplete_markers_migrated_path();
    if done.exists() {
        return Ok(());
    }
    if install_state::migrate_legacy_incomplete_markers()? {
        file::create_dir_all(done.parent().unwrap())?;
        file::write(&done, "")?;
    }
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
