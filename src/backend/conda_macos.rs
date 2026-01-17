//! macOS-specific library path fixing for conda packages
//!
//! Uses `install_name_tool` to rewrite hardcoded build paths and
//! `codesign` to re-sign binaries after modification.

use super::conda_common::{find_binary_files, is_macho_file};
use crate::install_context::InstallContext;
use eyre::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Fix library paths in all binaries and shared libraries after extraction.
/// This patches hardcoded build paths to point to the actual install directory.
pub fn fix_library_paths(ctx: &InstallContext, install_dir: &Path) -> Result<()> {
    ctx.pr.set_message("fixing library paths".to_string());

    // Remove quarantine/provenance attributes that can prevent execution
    // These are added by macOS when files are downloaded from the internet
    remove_quarantine_attrs(install_dir);

    let lib_dir = install_dir.join("lib");
    let bin_dir = install_dir.join("bin");

    // Find all Mach-O files (dylibs and executables)
    let files = find_binary_files(install_dir, is_macho_file);

    if files.is_empty() {
        return Ok(());
    }

    for file_path in &files {
        // Get current library dependencies using otool
        let deps = match get_library_dependencies(file_path) {
            Some(d) => d,
            None => continue,
        };

        // Collect paths that need fixing
        let paths_to_fix = find_paths_to_fix(&deps, &lib_dir);

        // Check if this is a library that needs its ID fixed
        let is_dylib = file_path.extension().is_some_and(|e| e == "dylib");
        let needs_id_fix = is_dylib && file_path.starts_with(&lib_dir);

        // Skip binaries that don't need any modification to preserve their checksum
        // This is important for tools like Santa that allowlist by checksum
        if paths_to_fix.is_empty() && !needs_id_fix {
            continue;
        }

        // Verify the original binary has a valid signature before modifying
        verify_signature(file_path);

        // Fix each hardcoded path
        for (old_path, new_path) in &paths_to_fix {
            fix_library_reference(file_path, old_path, new_path);
        }

        // Fix the library's own ID if it's a dylib in the lib directory
        if needs_id_fix {
            fix_library_id(file_path);
        }

        // Add rpath entries for lib directory
        add_rpath_entries(file_path, &lib_dir, &bin_dir);

        // Re-sign the binary with an ad-hoc signature
        // This is required because install_name_tool invalidates the original signature
        resign_binary(file_path);
    }

    Ok(())
}

/// Remove macOS quarantine and provenance attributes
fn remove_quarantine_attrs(dir: &Path) {
    let dir_str = dir.to_str().unwrap_or("");
    let _ = Command::new("xattr")
        .args(["-r", "-d", "com.apple.quarantine", dir_str])
        .output();
    let _ = Command::new("xattr")
        .args(["-r", "-d", "com.apple.provenance", dir_str])
        .output();
}

/// Get library dependencies using otool -L
fn get_library_dependencies(path: &Path) -> Option<String> {
    let output = Command::new("otool")
        .args(["-L", path.to_str().unwrap_or("")])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Find paths that need fixing in the otool output
fn find_paths_to_fix(deps: &str, lib_dir: &Path) -> Vec<(String, PathBuf)> {
    deps.lines()
        .skip(1) // Skip the first line (file path)
        .filter_map(|line| {
            let line = line.trim();
            if let Some(old_path) = extract_build_path(line) {
                if let Some(lib_name) = Path::new(&old_path).file_name() {
                    let new_path = lib_dir.join(lib_name);
                    if new_path.exists() {
                        return Some((old_path, new_path));
                    }
                }
            }
            None
        })
        .collect()
}

/// Verify binary signature (logging only)
fn verify_signature(path: &Path) {
    let result = Command::new("codesign")
        .args(["--verify", path.to_str().unwrap_or("")])
        .output();

    if let Ok(output) = result {
        if !output.status.success() {
            debug!(
                "Binary {} has invalid or missing signature, skipping signature verification",
                path.display()
            );
        }
    }
}

/// Fix a single library reference using install_name_tool
fn fix_library_reference(binary: &Path, old_path: &str, new_path: &Path) {
    let _ = Command::new("install_name_tool")
        .args([
            "-change",
            old_path,
            new_path.to_str().unwrap_or(""),
            binary.to_str().unwrap_or(""),
        ])
        .output();
}

/// Fix the library's own ID to use @rpath
fn fix_library_id(path: &Path) {
    if let Some(filename) = path.file_name() {
        let new_id = format!("@rpath/{}", filename.to_str().unwrap_or(""));
        let _ = Command::new("install_name_tool")
            .args(["-id", &new_id, path.to_str().unwrap_or("")])
            .output();
    }
}

/// Add rpath entries for library resolution
fn add_rpath_entries(path: &Path, lib_dir: &Path, bin_dir: &Path) {
    let path_str = path.to_str().unwrap_or("");

    // For binaries in bin/, add @executable_path/../lib
    // For libraries in lib/, add @loader_path
    if path.starts_with(bin_dir) {
        let _ = Command::new("install_name_tool")
            .args(["-add_rpath", "@executable_path/../lib", path_str])
            .output();
    } else if path.starts_with(lib_dir) {
        let _ = Command::new("install_name_tool")
            .args(["-add_rpath", "@loader_path", path_str])
            .output();
    }

    // Also add absolute path to lib directory as fallback
    if lib_dir.exists() {
        let _ = Command::new("install_name_tool")
            .args(["-add_rpath", lib_dir.to_str().unwrap_or(""), path_str])
            .output();
    }
}

/// Re-sign the binary with an ad-hoc signature
fn resign_binary(path: &Path) {
    let _ = Command::new("codesign")
        .args(["--force", "--sign", "-", path.to_str().unwrap_or("")])
        .output();
}

/// Extract a build path from an otool -L output line
/// Returns Some(path) if the line contains a hardcoded conda build path
fn extract_build_path(line: &str) -> Option<String> {
    // otool output format: "\t/path/to/lib.dylib (compatibility version ...)"
    let path = line.split('(').next()?.trim();

    // Only fix paths that look like conda build paths
    // These typically contain patterns like:
    // - /Users/runner/miniforge3/conda-bld/
    // - /home/conda/feedstock_root/
    // - /opt/conda/conda-bld/
    // - paths with _h_env_placehold_ (conda placeholder paths)
    if path.contains("conda-bld")
        || path.contains("feedstock_root")
        || path.contains("_h_env_placehold")
        || path.contains("_build_env")
        || path.contains("/conda/")
    {
        Some(path.to_string())
    } else {
        None
    }
}
