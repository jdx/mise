//! Linux-specific library path fixing for conda packages
//!
//! Uses `patchelf` to set RPATH entries for proper library resolution.

use eyre::Result;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::install_context::InstallContext;

/// Fix library paths on Linux using patchelf
pub fn fix_library_paths(ctx: &InstallContext, install_dir: &Path) -> Result<()> {
    ctx.pr.set_message("fixing library paths".to_string());

    let lib_dir = install_dir.join("lib");

    // Check if patchelf is available
    if Command::new("patchelf").arg("--version").output().is_err() {
        debug!("patchelf not found, skipping library path fixes");
        return Ok(());
    }

    // Find all ELF files
    let files = find_elf_files(install_dir)?;

    for file_path in files {
        // Set RPATH to $ORIGIN/../lib for binaries, $ORIGIN for libraries
        let rpath = if file_path.parent().is_some_and(|p| p.ends_with("bin")) {
            "$ORIGIN/../lib"
        } else if file_path.parent().is_some_and(|p| p.ends_with("lib")) {
            "$ORIGIN"
        } else {
            // For other locations, use absolute path
            lib_dir.to_str().unwrap_or("$ORIGIN/../lib")
        };

        let _ = Command::new("patchelf")
            .args(["--set-rpath", rpath, file_path.to_str().unwrap_or("")])
            .output();
    }

    Ok(())
}

/// Find all ELF files (executables and shared objects) in a directory
fn find_elf_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() && is_elf_file(path) {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}

/// Check if a file is an ELF binary
fn is_elf_file(path: &Path) -> bool {
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut magic = [0u8; 4];
        if file.read_exact(&mut magic).is_ok() {
            // ELF magic number: 0x7f 'E' 'L' 'F'
            return magic == [0x7f, b'E', b'L', b'F'];
        }
    }
    false
}
