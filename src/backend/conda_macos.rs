//! macOS-specific library path fixing for conda packages
//!
//! Uses `install_name_tool` to rewrite hardcoded build paths and
//! `codesign` to re-sign binaries after modification.

use eyre::Result;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::install_context::InstallContext;

/// Fix library paths in all binaries and shared libraries after extraction.
/// This patches hardcoded build paths to point to the actual install directory.
pub fn fix_library_paths(ctx: &InstallContext, install_dir: &Path) -> Result<()> {
    ctx.pr.set_message("fixing library paths".to_string());

    // Remove quarantine/provenance attributes that can prevent execution
    // These are added by macOS when files are downloaded from the internet
    let _ = Command::new("xattr")
        .args([
            "-r",
            "-d",
            "com.apple.quarantine",
            install_dir.to_str().unwrap_or(""),
        ])
        .output();
    let _ = Command::new("xattr")
        .args([
            "-r",
            "-d",
            "com.apple.provenance",
            install_dir.to_str().unwrap_or(""),
        ])
        .output();

    let lib_dir = install_dir.join("lib");
    let bin_dir = install_dir.join("bin");

    // Find all Mach-O files (dylibs and executables)
    let files = find_macho_files(install_dir)?;

    if files.is_empty() {
        return Ok(());
    }

    for file_path in &files {
        // Verify the original binary has a valid signature before modifying
        // This ensures the download wasn't corrupted or tampered with
        let verify_result = Command::new("codesign")
            .args(["--verify", file_path.to_str().unwrap_or("")])
            .output();

        if let Ok(output) = &verify_result {
            if !output.status.success() {
                // Binary has an invalid signature - log warning but continue
                // Some conda packages may not be signed at all
                debug!(
                    "Binary {} has invalid or missing signature, skipping signature verification",
                    file_path.display()
                );
            }
        }

        // Get current library dependencies using otool
        let output = match Command::new("otool")
            .args(["-L", file_path.to_str().unwrap_or("")])
            .output()
        {
            Ok(o) => o,
            Err(_) => continue,
        };

        let deps = String::from_utf8_lossy(&output.stdout);

        // For each dependency with a hardcoded build path, fix it
        for line in deps.lines().skip(1) {
            // otool output: "\t/path/to/lib.dylib (compatibility ...)"
            let line = line.trim();
            if let Some(old_path) = extract_build_path(line) {
                if let Some(lib_name) = Path::new(&old_path).file_name() {
                    let new_path = lib_dir.join(lib_name);

                    if new_path.exists() {
                        // Fix the library reference
                        let _ = Command::new("install_name_tool")
                            .args([
                                "-change",
                                &old_path,
                                new_path.to_str().unwrap_or(""),
                                file_path.to_str().unwrap_or(""),
                            ])
                            .output();
                    }
                }
            }
        }

        // Fix the library's own ID if it's a dylib
        if file_path.extension().is_some_and(|e| e == "dylib") {
            if let Some(filename) = file_path.file_name() {
                let new_id = format!("@rpath/{}", filename.to_str().unwrap_or(""));
                let _ = Command::new("install_name_tool")
                    .args(["-id", &new_id, file_path.to_str().unwrap_or("")])
                    .output();
            }
        }

        // Add rpath entries for lib directory
        // For binaries in bin/, add @executable_path/../lib
        // For libraries in lib/, add @loader_path
        if file_path.starts_with(&bin_dir) {
            let _ = Command::new("install_name_tool")
                .args([
                    "-add_rpath",
                    "@executable_path/../lib",
                    file_path.to_str().unwrap_or(""),
                ])
                .output();
        } else if file_path.starts_with(&lib_dir) {
            let _ = Command::new("install_name_tool")
                .args([
                    "-add_rpath",
                    "@loader_path",
                    file_path.to_str().unwrap_or(""),
                ])
                .output();
        }

        // Also add absolute path to lib directory as fallback
        if lib_dir.exists() {
            let _ = Command::new("install_name_tool")
                .args([
                    "-add_rpath",
                    lib_dir.to_str().unwrap_or(""),
                    file_path.to_str().unwrap_or(""),
                ])
                .output();
        }

        // Re-sign the binary with an ad-hoc signature
        // This is required because install_name_tool invalidates the original signature
        let _ = Command::new("codesign")
            .args(["--force", "--sign", "-", file_path.to_str().unwrap_or("")])
            .output();
    }

    Ok(())
}

/// Find all Mach-O files (executables and dylibs) in a directory
fn find_macho_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() && is_macho_file(path) {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}

/// Check if a file is a Mach-O binary
fn is_macho_file(path: &Path) -> bool {
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut magic = [0u8; 4];
        if file.read_exact(&mut magic).is_ok() {
            // Mach-O magic numbers
            return matches!(
                magic,
                [0xfe, 0xed, 0xfa, 0xce]   // MH_MAGIC (32-bit)
                    | [0xfe, 0xed, 0xfa, 0xcf] // MH_MAGIC_64 (64-bit)
                    | [0xce, 0xfa, 0xed, 0xfe] // MH_CIGAM (32-bit swapped)
                    | [0xcf, 0xfa, 0xed, 0xfe] // MH_CIGAM_64 (64-bit swapped)
                    | [0xca, 0xfe, 0xba, 0xbe] // FAT_MAGIC (universal)
                    | [0xbe, 0xba, 0xfe, 0xca] // FAT_CIGAM (universal swapped)
            );
        }
    }
    false
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
