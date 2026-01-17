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
