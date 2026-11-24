//! Patch the dynamic linkage (PT_INTERP and DT_RPATH/DT_RUNPATH) of ELF
//! binaries when pouring Linux bottles — the same work `brew` does via its
//! PatchELF gem (Library/Homebrew/extend/os/linux/keg_relocate.rb).
//!
//! Linux bottles are built at /home/linuxbrew/.linuxbrew and bottled with
//! `@@HOMEBREW_PREFIX@@` placeholders written into the ELF interpreter and
//! rpath. Restoring the real prefix grows those strings (19 -> 26 bytes), and
//! unlike Mach-O there is no header padding to grow into. We use patchelf's
//! strategy: when a string no longer fits in place, append a new read-only
//! PT_LOAD segment at the end of the file holding a relocated program header
//! table, the new interpreter string, and a new dynamic string table; then
//! point the ELF header, PT_PHDR/PT_INTERP, and DT_STRTAB/DT_STRSZ/DT_RPATH
//! entries at it. Old copies are left in place (unreferenced), exactly like
//! patchelf.
//!
//! Scope: 64-bit little-endian ELF only (x86_64/aarch64 — the only Linux
//! bottle architectures).

use std::path::Path;

use eyre::bail;
use log::debug;

use eyre::Result;

use super::{Replacement, memmem, replace_text};

const PLACEHOLDER_PREFIX: &[u8] = b"@@HOMEBREW_PREFIX@@";
const PLACEHOLDER_CELLAR: &[u8] = b"@@HOMEBREW_CELLAR@@";

const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;
const SHDR_SIZE: usize = 64;

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_PHDR: u32 = 6;
const PF_R: u32 = 4;

/// Alignment of the segment `patch` appends: covers 4K, 16K and 64K runtime
/// page sizes. The padding up to it is written out, so it is a constant
/// rather than the existing PT_LOADs' p_align, which is whatever the file says.
const NEW_SEGMENT_ALIGN: u64 = 0x10000;

const DT_NULL: i64 = 0;
const DT_STRTAB: i64 = 5;
const DT_STRSZ: i64 = 10;
const DT_RPATH: i64 = 15;
const DT_RUNPATH: i64 = 29;

pub(super) fn is_elf(content: &[u8]) -> bool {
    content.len() >= 4 && content[..4] == [0x7f, b'E', b'L', b'F']
}

/// What to relocate to. `gcc_current` applies brew's `lib/gcc/<N>` ->
/// `lib/gcc/current` rpath rewrite (disabled when pouring gcc itself).
pub(super) struct LinkageOpts {
    pub prefix: String,
    pub cellar: String,
    pub gcc_current: bool,
}

impl LinkageOpts {
    pub(super) fn for_formula(name: &str, prefix: &Path) -> Self {
        let is_gcc = name == "gcc" || name.starts_with("gcc@");
        LinkageOpts {
            prefix: prefix.to_string_lossy().to_string(),
            cellar: prefix.join("Cellar").to_string_lossy().to_string(),
            gcc_current: !is_gcc,
        }
    }
}

/// The `N` bytes at `off`, or an error when they are not all in the file.
fn rd_bytes<const N: usize>(b: &[u8], off: usize) -> Result<[u8; N]> {
    let s = off
        .checked_add(N)
        .and_then(|end| b.get(off..end))
        .ok_or_else(|| eyre::eyre!("truncated ELF"))?;
    Ok(s.try_into()?)
}

fn rd_u16(b: &[u8], off: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(rd_bytes(b, off)?))
}

fn rd_u32(b: &[u8], off: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(rd_bytes(b, off)?))
}

fn rd_u64(b: &[u8], off: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(rd_bytes(b, off)?))
}

/// `base + i * size` as a file offset, or None when it overflows.
fn table_entry(base: usize, i: usize, size: usize) -> Option<usize> {
    i.checked_mul(size)?.checked_add(base)
}

fn wr_u16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn wr_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

#[derive(Clone, Copy)]
struct Phdr {
    p_type: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_filesz: u64,
    p_memsz: u64,
}

fn read_phdrs(content: &[u8]) -> Result<Vec<Phdr>> {
    let e_phoff = rd_u64(content, 32)? as usize;
    let e_phentsize = rd_u16(content, 54)? as usize;
    let e_phnum = rd_u16(content, 56)? as usize;
    if e_phentsize != PHDR_SIZE {
        bail!("unexpected ELF e_phentsize {e_phentsize}");
    }
    if e_phnum >= 0xffff {
        bail!("ELF uses PN_XNUM program header counts");
    }
    let mut phdrs = Vec::with_capacity(e_phnum);
    for i in 0..e_phnum {
        let off = table_entry(e_phoff, i, PHDR_SIZE).ok_or_else(|| eyre::eyre!("truncated ELF"))?;
        phdrs.push(Phdr {
            p_type: rd_u32(content, off)?,
            p_offset: rd_u64(content, off + 8)?,
            p_vaddr: rd_u64(content, off + 16)?,
            p_filesz: rd_u64(content, off + 32)?,
            p_memsz: rd_u64(content, off + 40)?,
        });
    }
    Ok(phdrs)
}

fn vaddr_to_offset(phdrs: &[Phdr], vaddr: u64) -> Option<usize> {
    let p = phdrs
        .iter()
        .find(|p| p.p_type == PT_LOAD && p.p_vaddr <= vaddr && vaddr - p.p_vaddr < p.p_filesz)?;
    usize::try_from(p.p_offset.checked_add(vaddr - p.p_vaddr)?).ok()
}

/// ELF strings are byte strings, not necessarily UTF-8. They stay bytes, so a
/// string's length is the size of its slot in the file.
fn read_cstr(content: &[u8], off: usize) -> Result<Vec<u8>> {
    let bytes = content
        .get(off..)
        .ok_or_else(|| eyre::eyre!("string offset out of bounds"))?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| eyre::eyre!("unterminated string in ELF"))?;
    Ok(bytes[..end].to_vec())
}

fn replace_placeholders(s: &[u8], opts: &LinkageOpts) -> Vec<u8> {
    replace_text(
        s,
        &[
            Replacement {
                placeholder: PLACEHOLDER_CELLAR,
                value: opts.cellar.as_bytes().to_vec(),
            },
            Replacement {
                placeholder: PLACEHOLDER_PREFIX,
                value: opts.prefix.as_bytes().to_vec(),
            },
        ],
    )
}

/// brew's rpath rewrite (extend/os/linux/keg_relocate.rb#change_rpath!):
/// substitute the placeholder per component, rewrite versioned gcc lib dirs
/// to `current`, drop components outside the prefix (and not $ORIGIN-based),
/// and make sure `<prefix>/lib` is present.
fn new_rpath(old: &[u8], opts: &LinkageOpts) -> Vec<u8> {
    const GCC_LIB: &[u8] = b"/lib/gcc/";
    let lib_path = format!("{}/lib", opts.prefix).into_bytes();
    let mut components: Vec<Vec<u8>> = old
        .split(|&b| b == b':')
        .map(|c| replace_placeholders(c, opts))
        .map(|mut c| {
            if opts.gcc_current
                && let Some(pos) = c.windows(GCC_LIB.len()).rposition(|w| w == GCC_LIB)
                && let version = &c[pos + GCC_LIB.len()..]
                && !version.is_empty()
                && version.iter().all(u8::is_ascii_digit)
            {
                c.truncate(pos + GCC_LIB.len());
                c.extend_from_slice(b"current");
            }
            c
        })
        .filter(|c| c.starts_with(opts.prefix.as_bytes()) || c.starts_with(b"$ORIGIN"))
        .collect();
    if !components.contains(&lib_path) {
        components.push(lib_path);
    }
    components.join(&b':')
}

fn round_up(v: u64, align: u64) -> u64 {
    v.div_ceil(align) * align
}

/// Patch the interpreter and rpath of one ELF file in memory. Returns whether
/// anything changed. No-op unless a bottling placeholder is present.
pub(super) fn patch(content: &mut Vec<u8>, opts: &LinkageOpts, path: &Path) -> Result<bool> {
    if !is_elf(content) || content.len() < EHDR_SIZE {
        return Ok(false);
    }
    // 64-bit little-endian only
    if content[4] != 2 || content[5] != 1 {
        debug!("{}: not a 64-bit LE ELF, skipping", path.display());
        return Ok(false);
    }
    let phdrs = read_phdrs(content)?;

    // current interpreter
    let interp = phdrs.iter().find(|p| p.p_type == PT_INTERP).copied();
    let old_interp = match &interp {
        Some(p) => Some(read_cstr(content, p.p_offset as usize)?),
        None => None,
    };
    // brew sets the interpreter to <prefix>/lib/ld.so (which
    // prefix::setup_linux_runtime points at a real loader)
    let new_interp = match &old_interp {
        Some(s) if memmem(s, PLACEHOLDER_PREFIX).is_some() => {
            Some(format!("{}/lib/ld.so", opts.prefix))
        }
        _ => None,
    };

    // current rpath via the dynamic section
    let dynamic = phdrs.iter().find(|p| p.p_type == PT_DYNAMIC).copied();
    let mut strtab_vaddr = None;
    let mut strsz = None;
    // file offsets of the d_val fields to rewrite
    let mut strtab_val_off = None;
    let mut strsz_val_off = None;
    let mut rpath_val_offs: Vec<usize> = vec![];
    let mut rpath_strtab_off = None;
    if let Some(dyn_seg) = &dynamic {
        let start = dyn_seg.p_offset as usize;
        let end = start
            .saturating_add(dyn_seg.p_filesz as usize)
            .min(content.len());
        let mut off = start;
        while off < end && end - off >= 16 {
            let d_tag = rd_u64(content, off)? as i64;
            let d_val = rd_u64(content, off + 8)?;
            match d_tag {
                DT_NULL => break,
                DT_STRTAB => {
                    strtab_vaddr = Some(d_val);
                    strtab_val_off = Some(off + 8);
                }
                DT_STRSZ => {
                    strsz = Some(d_val);
                    strsz_val_off = Some(off + 8);
                }
                DT_RPATH | DT_RUNPATH => {
                    rpath_val_offs.push(off + 8);
                    rpath_strtab_off = Some(d_val);
                }
                _ => {}
            }
            off += 16;
        }
    }
    let strtab_off = strtab_vaddr.and_then(|v| vaddr_to_offset(&phdrs, v));
    // file offset of the current rpath string
    let rpath_off = match (strtab_off, rpath_strtab_off) {
        (Some(st), Some(rp)) => Some(
            usize::try_from(rp)
                .ok()
                .and_then(|rp| st.checked_add(rp))
                .ok_or_else(|| eyre::eyre!("string offset out of bounds"))?,
        ),
        _ => None,
    };
    // with DT_STRSZ, the rpath and its NUL lie inside the string table, so the
    // in-place rewrite below never touches bytes past the table
    let table_end = match (strtab_off, strsz) {
        (Some(st), Some(sz)) => usize::try_from(sz)
            .ok()
            .and_then(|sz| st.checked_add(sz))
            .map_or(content.len(), |end| end.min(content.len())),
        _ => content.len(),
    };
    let old_rpath = match rpath_off {
        Some(off) => Some(read_cstr(&content[..table_end], off).map_err(|e| {
            eyre::eyre!(
                "cannot relocate {}: its rpath does not lie inside the dynamic string table \
                 ({e})",
                path.display()
            )
        })?),
        None => None,
    };
    let new_rpath_str = match &old_rpath {
        Some(s)
            if memmem(s, PLACEHOLDER_PREFIX).is_some()
                || memmem(s, PLACEHOLDER_CELLAR).is_some() =>
        {
            Some(new_rpath(s, opts))
        }
        _ => None,
    };

    if new_interp.is_none() && new_rpath_str.is_none() {
        return Ok(false);
    }

    // PT_INTERP's p_filesz is the slot rewritten in place, so it has to lie
    // within the file
    if let (Some(p), Some(_)) = (&interp, &new_interp)
        && p.p_offset
            .checked_add(p.p_filesz)
            .is_none_or(|end| end > content.len() as u64)
    {
        bail!(
            "cannot relocate {}: PT_INTERP runs past the end of the file",
            path.display()
        );
    }

    // in-place when the new string fits in the old one's slot
    let interp_in_place = match (&interp, &new_interp) {
        // the string plus its NUL terminator must fit in the old slot
        (Some(p), Some(s)) => s.len() < p.p_filesz as usize,
        _ => true, // nothing to move
    };
    let rpath_in_place = match (&old_rpath, &new_rpath_str) {
        (Some(old), Some(new)) => new.len() <= old.len(),
        _ => true,
    };

    if interp_in_place && let (Some(p), Some(s)) = (&interp, &new_interp) {
        let start = p.p_offset as usize;
        let slot = p.p_filesz as usize;
        content[start..start + s.len()].copy_from_slice(s.as_bytes());
        for b in &mut content[start + s.len()..start + slot] {
            *b = 0;
        }
    }
    if rpath_in_place
        && let (Some(old), Some(new)) = (&old_rpath, &new_rpath_str)
        && let Some(start) = rpath_off
    {
        content[start..start + new.len()].copy_from_slice(new);
        for b in &mut content[start + new.len()..start + old.len()] {
            *b = 0;
        }
    }
    if interp_in_place && rpath_in_place {
        return Ok(true);
    }

    // grow: append a new PT_LOAD holding the relocated program header table
    // plus whichever strings no longer fit (patchelf's approach)
    let move_interp = !interp_in_place;
    let move_dynstr = !rpath_in_place;
    if move_dynstr && (strtab_off.is_none() || strsz.is_none() || strsz_val_off.is_none()) {
        bail!(
            "cannot relocate {}: rpath must grow but the dynamic string table \
             could not be located",
            path.display()
        );
    }
    // the whole table is copied into the new segment, so DT_STRSZ sizes the
    // output: it has to describe bytes that are in the file
    if move_dynstr
        && let (Some(st), Some(sz)) = (strtab_off, strsz)
        && (st as u64)
            .checked_add(sz)
            .is_none_or(|end| end > content.len() as u64)
    {
        bail!(
            "cannot relocate {}: its dynamic string table (DT_STRSZ {sz}) runs past the end \
             of the file",
            path.display()
        );
    }

    let align = NEW_SEGMENT_ALIGN;
    let new_off = round_up(content.len() as u64, align);
    let max_vaddr_end = phdrs
        .iter()
        .filter(|p| p.p_type == PT_LOAD)
        .map(|p| p.p_vaddr.saturating_add(p.p_memsz))
        .max()
        .unwrap_or(0);

    let e_phnum = rd_u16(content, 56)? as usize;
    if e_phnum + 1 >= 0xffff {
        bail!(
            "cannot relocate {}: no room for another program header",
            path.display()
        );
    }
    let table_len = (e_phnum + 1) * PHDR_SIZE;
    let rel_interp = round_up(table_len as u64, 8) as usize;
    let interp_len = match (&new_interp, move_interp) {
        (Some(s), true) => s.len() + 1,
        _ => 0,
    };
    let rel_dynstr = round_up((rel_interp + interp_len) as u64, 8) as usize;
    let (old_strsz, new_strsz, rpath_off_in_dynstr) = if move_dynstr {
        let old_strsz = strsz.unwrap() as usize;
        let appended = new_rpath_str.as_ref().unwrap().len() + 1;
        (old_strsz, old_strsz + appended, old_strsz)
    } else {
        (0, 0, 0)
    };
    let seg_len = rel_dynstr + new_strsz;
    let Some(new_vaddr) = max_vaddr_end
        .checked_next_multiple_of(align)
        .filter(|v| v.checked_add(seg_len as u64).is_some())
    else {
        bail!(
            "cannot relocate {}: no address space left for a new segment",
            path.display()
        );
    };

    let orig_len = content.len();
    content.resize(new_off as usize + seg_len, 0);

    // relocated program header table: copy entries, then fix up the moved ones
    let e_phoff = rd_u64(content, 32)? as usize;
    let table: Vec<u8> = content[e_phoff..e_phoff + e_phnum * PHDR_SIZE].to_vec();
    let base = new_off as usize;
    content[base..base + table.len()].copy_from_slice(&table);
    for i in 0..e_phnum {
        let off = base + i * PHDR_SIZE;
        let p_type = rd_u32(content, off)?;
        if p_type == PT_PHDR {
            wr_u64(content, off + 8, new_off);
            wr_u64(content, off + 16, new_vaddr);
            wr_u64(content, off + 24, new_vaddr);
            wr_u64(content, off + 32, table_len as u64);
            wr_u64(content, off + 40, table_len as u64);
        } else if p_type == PT_INTERP && move_interp {
            let s = new_interp.as_ref().unwrap();
            wr_u64(content, off + 8, new_off + rel_interp as u64);
            wr_u64(content, off + 16, new_vaddr + rel_interp as u64);
            wr_u64(content, off + 24, new_vaddr + rel_interp as u64);
            wr_u64(content, off + 32, (s.len() + 1) as u64);
            wr_u64(content, off + 40, (s.len() + 1) as u64);
        }
    }
    // the new PT_LOAD covering this segment (highest vaddr, appended last so
    // PT_LOAD entries stay sorted by vaddr)
    let off = base + e_phnum * PHDR_SIZE;
    let new_load = [
        (0usize, PT_LOAD as u64, 4usize), // p_type (u32)
        (4, PF_R as u64, 4),              // p_flags (u32)
        (8, new_off, 8),                  // p_offset
        (16, new_vaddr, 8),               // p_vaddr
        (24, new_vaddr, 8),               // p_paddr
        (32, seg_len as u64, 8),          // p_filesz
        (40, seg_len as u64, 8),          // p_memsz
        (48, align, 8),                   // p_align
    ];
    for (field_off, value, size) in new_load {
        if size == 4 {
            content[off + field_off..off + field_off + 4]
                .copy_from_slice(&(value as u32).to_le_bytes());
        } else {
            wr_u64(content, off + field_off, value);
        }
    }

    if move_interp && let Some(s) = &new_interp {
        let start = base + rel_interp;
        content[start..start + s.len()].copy_from_slice(s.as_bytes());
        content[start + s.len()] = 0;
    }
    if move_dynstr {
        let st = strtab_off.unwrap();
        let dynstr: Vec<u8> = content[st..st + old_strsz].to_vec();
        let start = base + rel_dynstr;
        content[start..start + old_strsz].copy_from_slice(&dynstr);
        let new = new_rpath_str.as_ref().unwrap();
        let rp_start = start + rpath_off_in_dynstr;
        content[rp_start..rp_start + new.len()].copy_from_slice(new);
        content[rp_start + new.len()] = 0;
    }

    // ELF header: program header table moved and grew by one entry
    wr_u64(content, 32, new_off);
    wr_u16(content, 56, (e_phnum + 1) as u16);

    // dynamic entries
    if move_dynstr {
        wr_u64(
            content,
            strtab_val_off.unwrap(),
            new_vaddr + rel_dynstr as u64,
        );
        wr_u64(content, strsz_val_off.unwrap(), new_strsz as u64);
        for val_off in &rpath_val_offs {
            wr_u64(content, *val_off, rpath_off_in_dynstr as u64);
        }
    }

    // keep section headers consistent for readelf/strip (runtime ignores them)
    let e_shoff = rd_u64(content, 40)? as usize;
    let e_shnum = rd_u16(content, 60)? as usize;
    // a table that does not lie wholly inside the original file is left alone,
    // with none of its entries changed
    let table_fits = table_entry(e_shoff, e_shnum, SHDR_SIZE).is_some_and(|end| end <= orig_len);
    if e_shoff != 0 && table_fits && rd_u16(content, 58)? as usize == SHDR_SIZE {
        for i in 0..e_shnum {
            let off = e_shoff + i * SHDR_SIZE;
            let sh_offset = rd_u64(content, off + 24)?;
            if move_interp
                && let Some(p) = &interp
                && sh_offset == p.p_offset
            {
                let s = new_interp.as_ref().unwrap();
                wr_u64(content, off + 16, new_vaddr + rel_interp as u64);
                wr_u64(content, off + 24, new_off + rel_interp as u64);
                wr_u64(content, off + 32, (s.len() + 1) as u64);
            } else if move_dynstr && sh_offset == strtab_off.unwrap() as u64 {
                wr_u64(content, off + 16, new_vaddr + rel_dynstr as u64);
                wr_u64(content, off + 24, new_off + rel_dynstr as u64);
                wr_u64(content, off + 32, new_strsz as u64);
            }
        }
    }

    Ok(true)
}

#[cfg(test)]
pub(crate) mod tests;
