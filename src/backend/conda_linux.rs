//! Linux-specific library path fixing for conda packages
//!
//! Uses `patchelf` to set RPATH entries for proper library resolution.

use super::conda_common::{find_binary_files, is_elf_file};
use crate::install_context::InstallContext;
use eyre::Result;
use std::path::Path;
use std::process::Command;

/// Fix library paths on Linux using patchelf
pub fn fix_library_paths(ctx: &InstallContext, install_dir: &Path) -> Result<()> {
    ctx.pr.set_message("fixing library paths".to_string());

    let lib_dir = install_dir.join("lib");

    // Check if patchelf is available
    if !patchelf_available() {
        debug!("patchelf not found, skipping library path fixes");
        return Ok(());
    }

    // Find all ELF files
    let files = find_binary_files(install_dir, is_elf_file);

    for file_path in files {
        let rpath = determine_rpath(&file_path, &lib_dir);
        set_rpath(&file_path, rpath);
    }

    Ok(())
}

/// Check if patchelf is available
fn patchelf_available() -> bool {
    Command::new("patchelf")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Determine the appropriate RPATH for a binary
fn determine_rpath<'a>(path: &Path, lib_dir: &'a Path) -> &'a str {
    // Set RPATH to $ORIGIN/../lib for binaries, $ORIGIN for libraries
    if path.parent().is_some_and(|p| p.ends_with("bin")) {
        "$ORIGIN/../lib"
    } else if path.parent().is_some_and(|p| p.ends_with("lib")) {
        "$ORIGIN"
    } else {
        // For other locations, use absolute path
        lib_dir.to_str().unwrap_or("$ORIGIN/../lib")
    }
}

/// Set RPATH on a binary using patchelf
fn set_rpath(path: &Path, rpath: &str) {
    let _ = Command::new("patchelf")
        .args(["--set-rpath", rpath, path.to_str().unwrap_or("")])
        .output();
}
