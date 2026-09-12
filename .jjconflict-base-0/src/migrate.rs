use std::path::Path;

use crate::config::Config;
use crate::dirs::*;
use crate::file;
use crate::runtime_symlinks;
use crate::toolset::install_state;
use eyre::Result;

pub(crate) async fn run() {
    tokio::join!(
        task(migrate_trusted_configs),
        task(migrate_tracked_configs),
        migrate_runtime_symlink_dirs(),
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

fn move_dirs(from: &Path, to: &Path) -> Result<bool> {
    if from.exists() && !to.exists() {
        eprintln!("migrating {} to {}", from.display(), to.display());
        file::create_dir_all(to.parent().unwrap())?;
        file::rename(from, to)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

async fn migrate_runtime_symlink_dirs() {
    const MARKER: &str = "runtime-symlink-dirs-v2";
    let marker = DATA.join("migrations").join(MARKER);
    if marker.exists() {
        return;
    }

    if let Err(err) = migrate_runtime_symlink_dirs_impl(&marker).await {
        eprintln!("[WARN] migrate: {err}");
    }
}

async fn migrate_runtime_symlink_dirs_impl(marker: &Path) -> Result<()> {
    // One-time cleanup for stale fuzzy-version directories like `latest`, `24`,
    // and `24.0` that should be runtime symlinks. Remove after 2026.10.0.
    let config = Config::get().await?;
    runtime_symlinks::migrate_real_dirs(&config).await?;
    // `backend::load_tools()` initializes install_state before migrations run in
    // normal CLI startup. Refresh only install_state after rewriting stale dirs
    // so config alias removals in the already-loaded backend map are preserved.
    install_state::reset();
    install_state::init().await?;
    file::create_dir_all(marker.parent().unwrap())?;
    file::write(marker, "ok\n")?;
    Ok(())
}
