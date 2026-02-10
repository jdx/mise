//! Linux-specific library path fixing for conda packages
//!
//! Uses `patchelf` to set RPATH entries for proper library resolution.

use super::conda_common::{find_binary_files, is_elf_file};
use crate::install_context::InstallContext;
use eyre::Result;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

/// Known dynamic linker names per architecture
const LINKER_NAMES: &[&str] = &["ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1"];

/// System paths to search for dynamic linkers (in priority order)
const SYSTEM_LINKER_PATHS: &[&str] = &[
    "/lib64/ld-linux-x86-64.so.2",
    "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
    "/lib/ld-linux-x86-64.so.2",
    "/lib64/ld-linux-aarch64.so.1",
    "/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1",
    "/lib/ld-linux-aarch64.so.1",
];

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
        // Only fix interpreter for executables (files in bin/), not all ELF files.
        // Shared libraries don't have PT_INTERP, so this avoids unnecessary
        // patchelf --print-interpreter calls for large installs.
        if file_path
            .parent()
            .is_some_and(|p| p.ends_with("bin") || p.ends_with("libexec"))
        {
            fix_interpreter(file_path, install_dir);
        }
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
///
/// Excludes sysroot directories which contain compilation stubs (e.g. libc.so)
/// that must not be loaded at runtime — using them causes symbol lookup errors
/// because they're incompatible with the system's dynamic linker.
fn find_lib_dirs(install_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = std::collections::HashSet::new();
    for entry in WalkDir::new(install_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
            && (name.ends_with(".so") || name.contains(".so."))
            && let Some(parent) = path.parent()
            && !path_contains_sysroot(parent)
        {
            dirs.insert(parent.to_path_buf());
        }
    }

    let mut sorted: Vec<_> = dirs.into_iter().collect();
    sorted.sort();
    sorted
}

/// Build RPATH string for a binary, including all library directories
/// Uses $ORIGIN-relative paths where possible
fn build_rpath(path: &Path, install_dir: &Path, lib_dirs: &[PathBuf]) -> String {
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
        if let Ok(rel_path) = lib_dir.strip_prefix(install_dir) {
            let Some(rel) = rel_path.to_str() else {
                continue; // Skip non-UTF8 paths
            };
            if let Some(parent) = path.parent()
                && let Ok(from_parent) = parent.strip_prefix(install_dir)
            {
                let depth = from_parent.components().count();
                let up = "../".repeat(depth);
                let entry = format!("$ORIGIN/{}{}", up, rel);
                if !entries.contains(&entry) {
                    entries.push(entry);
                }
            }
        }
    }

    entries.join(":")
}

/// Fix ELF interpreter if it points to a conda build path
fn fix_interpreter(path: &Path, install_dir: &Path) {
    let Some(path_str) = path.to_str() else {
        debug!(
            "skipping non-UTF8 path in fix_interpreter: {}",
            path.display()
        );
        return;
    };

    // Read current interpreter
    let output = match Command::new("patchelf")
        .args(["--print-interpreter", path_str])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            debug!("patchelf --print-interpreter failed to spawn for {path_str}: {e}");
            return;
        }
    };

    if !output.status.success() {
        return; // Shared libraries don't have PT_INTERP
    }

    let interp = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Check if interpreter points to a conda build path
    if !is_conda_build_path(&interp) {
        return;
    }

    // Try local linker first (check all known architectures)
    for linker_name in LINKER_NAMES {
        let local_linker = install_dir.join("lib").join(linker_name);
        if local_linker.exists()
            && let Some(local_str) = local_linker.to_str()
            && run_set_interpreter(path_str, local_str)
        {
            return;
        }
    }

    // Fall back to system linker — only break on successful patchelf
    for system_linker in SYSTEM_LINKER_PATHS {
        if Path::new(system_linker).exists() && run_set_interpreter(path_str, system_linker) {
            return;
        }
    }

    debug!("no working linker found for {path_str} (original: {interp})");
}

/// Run patchelf --set-interpreter, returning true on success
fn run_set_interpreter(path_str: &str, interpreter: &str) -> bool {
    match Command::new("patchelf")
        .args(["--set-interpreter", interpreter, path_str])
        .output()
    {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            debug!(
                "patchelf --set-interpreter {interpreter} failed for {path_str}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            false
        }
        Err(e) => {
            debug!("patchelf --set-interpreter failed to spawn for {path_str}: {e}");
            false
        }
    }
}

/// Check if a path is inside a sysroot directory (compilation stubs, not runtime libs)
fn path_contains_sysroot(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == "sysroot" || c.as_os_str().to_string_lossy().contains("sysroot"))
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
    let Some(path_str) = path.to_str() else {
        debug!("skipping non-UTF8 path in set_rpath: {}", path.display());
        return;
    };
    match Command::new("patchelf")
        .args(["--set-rpath", rpath, path_str])
        .output()
    {
        Ok(o) if !o.status.success() => {
            debug!(
                "patchelf --set-rpath failed for {path_str}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => {
            debug!("patchelf --set-rpath failed to spawn for {path_str}: {e}");
        }
        _ => {}
    }
}
