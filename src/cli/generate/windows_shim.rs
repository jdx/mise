use crate::dirs;
use crate::file;
use crate::http::HTTP;
use crate::ui::multi_progress_report::MultiProgressReport;
use color_eyre::eyre::Result;
use std::path::PathBuf;

const SHIM_BASE_URL: &str = "https://mise.jdx.dev";

/// Get the Windows shim for the current or specified architecture
pub async fn get_windows_shim(arch: Option<&str>) -> Result<PathBuf> {
    let arch = arch.unwrap_or({
        if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        }
    });

    // Check cache first
    let cache_dir = dirs::CACHE.join("windows-shims");
    file::create_dir_all(&cache_dir)?;
    let cached_shim = cache_dir.join(format!("mise-windows-shim-{arch}.exe"));

    if cached_shim.exists() {
        debug!("Using cached Windows shim: {}", cached_shim.display());
        return Ok(cached_shim);
    }

    // Download the shim
    download_windows_shim(arch, &cached_shim).await?;
    Ok(cached_shim)
}

async fn download_windows_shim(arch: &str, dest: &PathBuf) -> Result<()> {
    let url = format!("{SHIM_BASE_URL}/mise-windows-shim-{arch}.exe");
    info!("Downloading Windows shim from {}", url);

    let mpr = MultiProgressReport::get();
    let pr = mpr.add(&format!("download mise-windows-shim-{arch}.exe"));
    HTTP.download_file(&url, dest, Some(&pr)).await?;
    pr.finish();

    // Make it executable (though Windows doesn't use Unix permissions)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(dest)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(dest, perms)?;
    }

    Ok(())
}

/// Check if a Windows shim is available locally (for Windows builds)
pub fn get_local_windows_shim() -> Option<PathBuf> {
    // First check if we have it bundled with mise
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let bundled_stub = parent.join("mise-stub.exe");
            if bundled_stub.exists() {
                return Some(bundled_stub);
            }
        }
    }

    // Check for development build locations
    let locations = vec![
        PathBuf::from("target/release/mise-stub.exe"),
        PathBuf::from("target/debug/mise-stub.exe"),
    ];

    for location in locations {
        if location.exists() {
            return Some(location);
        }
    }

    None
}

/// Detect the target Windows architecture from platform string
pub fn detect_windows_arch(platform: &str) -> &str {
    if platform.contains("arm64") || platform.contains("aarch64") {
        "arm64"
    } else {
        "x64"
    }
}
