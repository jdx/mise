//! Minimal Mach-O editor for bottle relocation.
//!
//! Replaces Homebrew placeholder strings inside dylib load commands
//! (LC_ID_DYLIB, LC_LOAD_DYLIB, ..., LC_RPATH), growing a command's
//! `cmdsize` when the new string doesn't fit — the same thing brew does via
//! ruby-macho. Growth consumes the zero padding that exists between the end
//! of the load-command table and the first section's file data, so the file
//! size and all section offsets are unchanged.
//!
//! Handles 64-bit little-endian Mach-O (arm64/x86_64) and fat binaries whose
//! slices are patched independently in place.

use std::path::Path;

use eyre::bail;

use super::relocate::Replacement;
use crate::result::Result;

const MH_MAGIC_64_LE: u32 = 0xfeedfacf;
const FAT_MAGIC_BE: u32 = 0xcafebabe;
const HEADER_SIZE_64: usize = 32;

const LC_REQ_DYLD: u32 = 0x8000_0000;
const LC_ID_DYLIB: u32 = 0xd;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_WEAK_DYLIB: u32 = 0x18 | LC_REQ_DYLD;
const LC_REEXPORT_DYLIB: u32 = 0x1f | LC_REQ_DYLD;
const LC_LAZY_LOAD_DYLIB: u32 = 0x20;
const LC_LOAD_UPWARD_DYLIB: u32 = 0x23 | LC_REQ_DYLD;
const LC_RPATH: u32 = 0x1c | LC_REQ_DYLD;
const LC_SEGMENT_64: u32 = 0x19;

fn u32_at(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

fn has_path_string(cmd: u32) -> bool {
    matches!(
        cmd,
        LC_ID_DYLIB
            | LC_LOAD_DYLIB
            | LC_LOAD_WEAK_DYLIB
            | LC_REEXPORT_DYLIB
            | LC_LAZY_LOAD_DYLIB
            | LC_LOAD_UPWARD_DYLIB
            | LC_RPATH
    )
}

fn replace_all(s: &[u8], replacements: &[Replacement]) -> Vec<u8> {
    let mut out = s.to_vec();
    for r in replacements {
        while let Some(pos) = out
            .windows(r.placeholder.len())
            .position(|w| w == r.placeholder)
        {
            out.splice(pos..pos + r.placeholder.len(), r.value.iter().cloned());
        }
    }
    out
}

/// Patch one 64-bit LE Mach-O slice in place. Returns whether it changed.
fn patch_slice(slice: &mut [u8], replacements: &[Replacement], path: &Path) -> Result<bool> {
    if slice.len() < HEADER_SIZE_64 || u32_at(slice, 0) != MH_MAGIC_64_LE {
        // not a 64-bit LE Mach-O (32-bit or big-endian) — nothing modern on
        // arm64 macOS; leave it to the caller's generic byte-level pass
        return Ok(false);
    }
    let ncmds = u32_at(slice, 16) as usize;
    let sizeofcmds = u32_at(slice, 20) as usize;
    if HEADER_SIZE_64 + sizeofcmds > slice.len() {
        bail!("malformed Mach-O in {}", path.display());
    }

    // upper bound for growing the load-command table: the first byte of
    // section data (everything between sizeofcmds and there is padding)
    let lc_end = HEADER_SIZE_64 + sizeofcmds;
    let mut first_data = slice.len();
    {
        let mut off = HEADER_SIZE_64;
        for _ in 0..ncmds {
            if off + 8 > lc_end {
                bail!("malformed load command table in {}", path.display());
            }
            let cmd = u32_at(slice, off);
            let cmdsize = u32_at(slice, off + 4) as usize;
            if cmdsize < 8 || off + cmdsize > lc_end {
                bail!("malformed load command in {}", path.display());
            }
            if cmd == LC_SEGMENT_64 {
                let nsects = u32_at(slice, off + 64) as usize;
                for i in 0..nsects {
                    // struct section_64 is 80 bytes; offset field at +48
                    let sect = off + 72 + i * 80;
                    if sect + 80 > off + cmdsize {
                        break;
                    }
                    let file_off = u32_at(slice, sect + 48) as usize;
                    if file_off > 0 {
                        first_data = first_data.min(file_off);
                    }
                }
            }
            off += cmdsize;
        }
    }

    // rebuild the load-command table, editing path strings as we go
    let mut commands: Vec<Vec<u8>> = Vec::with_capacity(ncmds);
    let mut changed = false;
    let mut off = HEADER_SIZE_64;
    for _ in 0..ncmds {
        if off + 8 > lc_end {
            bail!("malformed load command table in {}", path.display());
        }
        let cmd = u32_at(slice, off);
        let cmdsize = u32_at(slice, off + 4) as usize;
        let mut bytes = slice[off..off + cmdsize].to_vec();
        off += cmdsize;
        if has_path_string(cmd) {
            let str_off = u32_at(&bytes, 8) as usize;
            if str_off < bytes.len() {
                let str_end = bytes[str_off..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| str_off + p)
                    .unwrap_or(bytes.len());
                let old = &bytes[str_off..str_end];
                let new = replace_all(old, replacements);
                if new != old {
                    // rebuild the command at the (possibly larger) aligned size
                    let new_cmdsize = (str_off + new.len() + 1).div_ceil(8) * 8;
                    let mut rebuilt = bytes[..str_off].to_vec();
                    rebuilt.extend_from_slice(&new);
                    rebuilt.resize(new_cmdsize.max(cmdsize), 0);
                    let len = rebuilt.len() as u32;
                    rebuilt[4..8].copy_from_slice(&len.to_le_bytes());
                    bytes = rebuilt;
                    changed = true;
                }
            }
        }
        commands.push(bytes);
    }
    if !changed {
        return Ok(false);
    }
    let new_sizeofcmds: usize = commands.iter().map(|c| c.len()).sum();
    if HEADER_SIZE_64 + new_sizeofcmds > first_data {
        bail!(
            "cannot relocate {}: not enough padding to grow load commands ({} > {} bytes)",
            path.display(),
            HEADER_SIZE_64 + new_sizeofcmds,
            first_data,
        );
    }
    let mut out = Vec::with_capacity(new_sizeofcmds);
    for c in &commands {
        out.extend_from_slice(c);
    }
    // zero everything from the header to the first data byte, then lay the
    // table down — leaves old table bytes cleanly erased
    slice[HEADER_SIZE_64..first_data].fill(0);
    slice[HEADER_SIZE_64..HEADER_SIZE_64 + out.len()].copy_from_slice(&out);
    slice[20..24].copy_from_slice(&(new_sizeofcmds as u32).to_le_bytes());
    Ok(true)
}

/// Patch load-command path strings in a Mach-O file (thin or fat).
/// Returns whether anything changed.
pub fn patch(content: &mut [u8], replacements: &[Replacement], path: &Path) -> Result<bool> {
    if content.len() < 8 {
        return Ok(false);
    }
    let be_magic = u32::from_be_bytes(content[..4].try_into().unwrap());
    if be_magic == FAT_MAGIC_BE {
        let nfat = u32::from_be_bytes(content[4..8].try_into().unwrap()) as usize;
        let mut changed = false;
        // collect slice ranges first (fat headers are big-endian)
        let mut ranges = vec![];
        for i in 0..nfat {
            let entry = 8 + i * 20;
            if entry + 20 > content.len() {
                bail!("malformed fat header in {}", path.display());
            }
            let offset =
                u32::from_be_bytes(content[entry + 8..entry + 12].try_into().unwrap()) as usize;
            let size =
                u32::from_be_bytes(content[entry + 12..entry + 16].try_into().unwrap()) as usize;
            if offset + size > content.len() {
                bail!("malformed fat arch in {}", path.display());
            }
            ranges.push(offset..offset + size);
        }
        for range in ranges {
            changed |= patch_slice(&mut content[range], replacements, path)?;
        }
        Ok(changed)
    } else {
        patch_slice(content, replacements, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::packages::brew::relocate::tests::test_replacements;

    /// build a minimal 64-bit Mach-O: header + LC_SEGMENT_64 (one section)
    /// + LC_LOAD_DYLIB with the given name, then data at `data_off`
    fn fake_macho(dylib_name: &[u8], pad: usize, data_off: u32) -> Vec<u8> {
        let mut lc_dylib = vec![];
        lc_dylib.extend_from_slice(&LC_LOAD_DYLIB.to_le_bytes());
        let str_off = 24u32;
        let cmdsize = (24 + dylib_name.len() + 1 + pad) as u32;
        lc_dylib.extend_from_slice(&cmdsize.to_le_bytes());
        lc_dylib.extend_from_slice(&str_off.to_le_bytes());
        lc_dylib.extend_from_slice(&[0u8; 12]); // timestamp/versions
        lc_dylib.extend_from_slice(dylib_name);
        lc_dylib.resize(cmdsize as usize, 0);

        let mut lc_seg = vec![];
        lc_seg.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
        lc_seg.extend_from_slice(&152u32.to_le_bytes()); // 72 + 80
        lc_seg.extend_from_slice(&[0u8; 56]); // segname + vm/file ranges
        lc_seg.extend_from_slice(&1u32.to_le_bytes()); // nsects
        lc_seg.extend_from_slice(&0u32.to_le_bytes()); // flags
        let mut sect = vec![0u8; 80];
        sect[48..52].copy_from_slice(&data_off.to_le_bytes());
        lc_seg.extend_from_slice(&sect);

        let mut out = vec![];
        out.extend_from_slice(&MH_MAGIC_64_LE.to_le_bytes());
        out.extend_from_slice(&[0u8; 12]); // cputype etc.
        out.extend_from_slice(&2u32.to_le_bytes()); // ncmds
        out.extend_from_slice(&((lc_seg.len() + lc_dylib.len()) as u32).to_le_bytes());
        out.extend_from_slice(&[0u8; 8]); // flags + reserved
        out.extend_from_slice(&lc_seg);
        out.extend_from_slice(&lc_dylib);
        out.resize(data_off as usize, 0);
        out.extend_from_slice(b"SECTION-DATA");
        out
    }

    fn find_dylib_name(buf: &[u8]) -> Vec<u8> {
        // second load command starts after the 152-byte segment command
        let lc = HEADER_SIZE_64 + 152;
        let str_off = lc + u32_at(buf, lc + 8) as usize;
        let end = buf[str_off..].iter().position(|&b| b == 0).unwrap() + str_off;
        buf[str_off..end].to_vec()
    }

    #[test]
    fn test_patch_in_place_when_fits() {
        // shrinking replacement, no resize needed
        let mut buf = fake_macho(b"@@HOMEBREW_PREFIX@@/lib/libx.dylib", 0, 4096);
        let changed = patch(&mut buf, &test_replacements(), Path::new("t")).unwrap();
        assert!(changed);
        assert_eq!(find_dylib_name(&buf), b"/opt/homebrew/lib/libx.dylib");
        assert_eq!(&buf[buf.len() - 12..], b"SECTION-DATA");
    }

    #[test]
    fn test_patch_grows_command_into_padding() {
        // the icu4c case: cellar replacement grows by 1 byte, zero slack
        let name = b"@@HOMEBREW_CELLAR@@/icu4c@78/78.3/lib/libicutu.78.dylib";
        let mut buf = fake_macho(name, 0, 4096);
        let old_sizeofcmds = u32_at(&buf, 20);
        let changed = patch(&mut buf, &test_replacements(), Path::new("t")).unwrap();
        assert!(changed);
        assert_eq!(
            find_dylib_name(&buf),
            b"/opt/homebrew/Cellar/icu4c@78/78.3/lib/libicutu.78.dylib"
        );
        assert_eq!(u32_at(&buf, 20), old_sizeofcmds + 8);
        assert_eq!(&buf[buf.len() - 12..], b"SECTION-DATA");
    }

    #[test]
    fn test_patch_fails_without_padding() {
        // section data starts immediately after the load commands — no room
        let name = b"@@HOMEBREW_CELLAR@@/icu4c@78/78.3/lib/libicutu.78.dylib";
        let tight = HEADER_SIZE_64 + 152 + 24 + name.len() + 1;
        let mut buf = fake_macho(name, 0, tight.div_ceil(8) as u32 * 8);
        let res = patch(&mut buf, &test_replacements(), Path::new("t"));
        assert!(res.is_err());
    }

    #[test]
    fn test_patch_noop_without_placeholders() {
        let mut buf = fake_macho(b"/usr/lib/libSystem.B.dylib", 0, 4096);
        let orig = buf.clone();
        let changed = patch(&mut buf, &test_replacements(), Path::new("t")).unwrap();
        assert!(!changed);
        assert_eq!(buf, orig);
    }
}
