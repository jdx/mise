//! Shared utilities for platform-specific conda library path fixing
//!
//! This module provides common functionality used by both macOS and Linux
//! implementations for fixing hardcoded library paths in conda packages.
//!
//! Note: Some items are only used on specific platforms, so allow dead_code.

#![allow(dead_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Fix hardcoded conda build prefixes in text files (shell scripts, configs, etc.)
///
/// Conda packages are built with a long placeholder prefix (containing
/// `_h_env_placehold` padding) that normally gets replaced by conda during
/// installation. Since we extract packages directly, we need to do this
/// replacement ourselves.
pub fn fix_text_prefixes(install_dir: &Path) {
    let Some(install_str) = install_dir.to_str() else {
        return;
    };

    let Some(prefix) = find_conda_prefix(install_dir) else {
        return;
    };

    debug!(
        "replacing conda prefix ({} chars) with {}",
        prefix.len(),
        install_str
    );

    for entry in WalkDir::new(install_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // Skip binary files (contain null bytes in first chunk)
        if bytes.iter().take(512).any(|&b| b == 0) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if !text.contains(&prefix) {
            continue;
        }
        let new_text = text.replace(&prefix, install_str);
        if let Err(e) = std::fs::write(path, new_text) {
            debug!("failed to fix prefix in {}: {e}", path.display());
        }
    }
}

/// Find the conda build prefix by scanning files for the `_h_env_placehold` marker.
/// Returns the full prefix path (from leading `/` to end of placeholder segment).
fn find_conda_prefix(install_dir: &Path) -> Option<String> {
    // Check bin/ first since that's where shell script wrappers live
    let bin_dir = install_dir.join("bin");
    if bin_dir.exists() {
        for entry in WalkDir::new(&bin_dir)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(path)
                && let Some(prefix) = extract_placeholder_prefix(&content)
            {
                return Some(prefix);
            }
        }
    }

    // Fall back to scanning all text files
    for entry in WalkDir::new(install_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if bytes.iter().take(512).any(|&b| b == 0) {
            continue;
        }
        if let Ok(content) = std::str::from_utf8(&bytes)
            && let Some(prefix) = extract_placeholder_prefix(content)
        {
            return Some(prefix);
        }
    }

    None
}

/// Extract the conda placeholder prefix from text content.
/// The prefix is an absolute path containing `_h_env_placehold` padding,
/// e.g. `/home/conda/feedstock_root/.../ghc_163.../_h_env_placehold_placehold_...`
fn extract_placeholder_prefix(content: &str) -> Option<String> {
    let marker = "_h_env_placehold";
    let idx = content.find(marker)?;

    // Walk backward to find start of the absolute path
    let before = &content[..idx];
    let pos = before
        .rfind(|c: char| !c.is_alphanumeric() && !matches!(c, '/' | '_' | '-' | '.' | '+'))?;
    // Skip past the delimiter character (handles multi-byte UTF-8 correctly)
    let start = pos + before[pos..].chars().next()?.len_utf8();

    // Validate it starts with /
    if !content[start..].starts_with('/') {
        return None;
    }

    // Walk forward from marker to find end of the placeholder segment
    let after = &content[idx..];
    let end = after
        .find(['/', '"', '\'', '\n', ' ', ':'])
        .unwrap_or(after.len());

    let prefix = &content[start..idx + end];
    if prefix.len() > 20 {
        Some(prefix.to_string())
    } else {
        None
    }
}

/// Mach-O magic numbers for binary detection (macOS)
pub const MACHO_MAGIC_32: [u8; 4] = [0xfe, 0xed, 0xfa, 0xce];
pub const MACHO_MAGIC_64: [u8; 4] = [0xfe, 0xed, 0xfa, 0xcf];
pub const MACHO_CIGAM_32: [u8; 4] = [0xce, 0xfa, 0xed, 0xfe];
pub const MACHO_CIGAM_64: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];
pub const FAT_MAGIC: [u8; 4] = [0xca, 0xfe, 0xba, 0xbe];
pub const FAT_CIGAM: [u8; 4] = [0xbe, 0xba, 0xfe, 0xca];

/// ELF magic number (Linux)
pub const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// Read the first 4 bytes (magic number) from a file
pub fn read_magic(path: &Path) -> Option<[u8; 4]> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    Some(magic)
}

/// Check if a file is a Mach-O binary (macOS executable or dylib)
pub fn is_macho_file(path: &Path) -> bool {
    read_magic(path).is_some_and(|magic| {
        matches!(
            magic,
            MACHO_MAGIC_32
                | MACHO_MAGIC_64
                | MACHO_CIGAM_32
                | MACHO_CIGAM_64
                | FAT_MAGIC
                | FAT_CIGAM
        )
    })
}

/// Check if a file is an ELF binary (Linux executable or shared object)
pub fn is_elf_file(path: &Path) -> bool {
    read_magic(path).is_some_and(|magic| magic == ELF_MAGIC)
}

/// Find all files in a directory that match a predicate
pub fn find_binary_files<F>(dir: &Path, is_binary: F) -> Vec<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter(|e| is_binary(e.path()))
        .map(|e| e.path().to_path_buf())
        .collect()
}
