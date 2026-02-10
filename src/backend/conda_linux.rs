//! Linux-specific library path fixing for conda packages
//!
//! Uses `patchelf` to set RPATH entries for proper library resolution.

use super::conda_common::{find_binary_files, is_elf_file};
use crate::install_context::InstallContext;
use eyre::Result;
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

/// Fix library paths on Linux using patchelf
pub fn fix_library_paths(ctx: &InstallContext, install_dir: &Path) -> Result<()> {
    ctx.pr.set_message("fixing library paths".to_string());

    // Check if patchelf is available
    if !patchelf_available() {
        debug!("patchelf not found, skipping library path fixes");
        return Ok(());
    }

    // Discover all directories containing .so files for comprehensive RPATH
    let lib_dirs = find_lib_dirs(install_dir);

    // Find all ELF files
    let files = find_binary_files(install_dir, is_elf_file);

    for file_path in &files {
        fix_interpreter(file_path, install_dir);
        let rpath = build_rpath(file_path, install_dir, &lib_dirs);
        set_rpath(file_path, &rpath);
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

/// Find all directories under install_dir that contain shared libraries (.so files)
fn find_lib_dirs(install_dir: &Path) -> Vec<String> {
    let mut dirs = std::collections::HashSet::new();
    for entry in WalkDir::new(install_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.contains(".so") {
                    if let Some(parent) = path.parent() {
                        dirs.insert(parent.to_path_buf());
                    }
                }
            }
        }
    }

    let mut sorted: Vec<_> = dirs.into_iter().collect();
    sorted.sort();
    sorted
        .into_iter()
        .filter_map(|d| d.to_str().map(|s| s.to_string()))
        .collect()
}

/// Build RPATH string for a binary, including all library directories
/// Uses $ORIGIN-relative paths where possible
fn build_rpath(path: &Path, install_dir: &Path, lib_dirs: &[String]) -> String {
    let install_str = install_dir.to_str().unwrap_or("");

    // Start with the standard entries
    let mut entries = Vec::new();

    if path.parent().is_some_and(|p| p.ends_with("bin")) {
        entries.push("$ORIGIN/../lib".to_string());
    } else if let Some(parent) = path.parent() {
        // For libraries, add $ORIGIN so they can find siblings
        entries.push("$ORIGIN".to_string());
        // Also add path to lib/ from wherever this file is
        if let Ok(rel) = parent.strip_prefix(install_dir) {
            let depth = rel.components().count();
            if depth > 0 {
                let up = "../".repeat(depth);
                entries.push(format!("$ORIGIN/{}lib", up));
            }
        }
    }

    // Add all discovered lib directories as $ORIGIN-relative paths
    for lib_dir in lib_dirs {
        if let Some(rel) = lib_dir.strip_prefix(install_str) {
            let rel = rel.trim_start_matches('/');
            if let Some(parent) = path.parent() {
                if let Ok(from_parent) = parent.strip_prefix(install_dir) {
                    let depth = from_parent.components().count();
                    let up = "../".repeat(depth);
                    let entry = format!("$ORIGIN/{}{}", up, rel);
                    if !entries.contains(&entry) {
                        entries.push(entry);
                    }
                }
            }
        }
    }

    entries.join(":")
}

/// Fix ELF interpreter if it points to a conda build path
fn fix_interpreter(path: &Path, install_dir: &Path) {
    let path_str = path.to_str().unwrap_or("");

    // Read current interpreter
    let Ok(output) = Command::new("patchelf")
        .args(["--print-interpreter", path_str])
        .output()
    else {
        return; // Not an executable (shared libs don't have interpreters)
    };

    if !output.status.success() {
        return; // Shared libraries don't have PT_INTERP
    }

    let interp = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Check if interpreter points to a conda build path
    if is_conda_build_path(&interp) {
        // Check if we have a linker in our install dir
        let local_linker = install_dir.join("lib").join("ld-linux-x86-64.so.2");
        if local_linker.exists() {
            let local_str = local_linker.to_str().unwrap_or("");
            let result = Command::new("patchelf")
                .args(["--set-interpreter", local_str, path_str])
                .output();
            if let Ok(o) = result {
                if !o.status.success() {
                    debug!(
                        "patchelf --set-interpreter failed for {}: {}",
                        path_str,
                        String::from_utf8_lossy(&o.stderr)
                    );
                }
            }
        } else {
            // Fall back to system linker
            for system_linker in &[
                "/lib64/ld-linux-x86-64.so.2",
                "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
                "/lib/ld-linux-x86-64.so.2",
                "/lib/ld-linux-aarch64.so.1",
                "/lib64/ld-linux-aarch64.so.1",
            ] {
                if Path::new(system_linker).exists() {
                    let result = Command::new("patchelf")
                        .args(["--set-interpreter", system_linker, path_str])
                        .output();
                    if let Ok(o) = result {
                        if !o.status.success() {
                            debug!(
                                "patchelf --set-interpreter failed for {}: {}",
                                path_str,
                                String::from_utf8_lossy(&o.stderr)
                            );
                        }
                    }
                    break;
                }
            }
        }
    }
}

/// Check if a path looks like a conda build-time path
fn is_conda_build_path(path: &str) -> bool {
    path.contains("conda-bld")
        || path.contains("_build_env")
        || path.contains("_h_env_placehold")
        || path.contains("/home/conda/")
        || path.contains("/Users/runner/miniforge3/")
        || path.contains("/opt/conda/")
}

/// Set RPATH on a binary using patchelf
fn set_rpath(path: &Path, rpath: &str) {
    let path_str = path.to_str().unwrap_or("");
    let result = Command::new("patchelf")
        .args(["--set-rpath", rpath, path_str])
        .output();
    if let Ok(o) = result {
        if !o.status.success() {
            debug!(
                "patchelf --set-rpath failed for {}: {}",
                path_str,
                String::from_utf8_lossy(&o.stderr)
            );
        }
    }
}
