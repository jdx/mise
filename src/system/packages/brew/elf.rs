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

use crate::result::Result;

const PLACEHOLDER_PREFIX: &str = "@@HOMEBREW_PREFIX@@";
const PLACEHOLDER_CELLAR: &str = "@@HOMEBREW_CELLAR@@";

const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;
const SHDR_SIZE: usize = 64;

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_PHDR: u32 = 6;
const PF_R: u32 = 4;

const DT_NULL: i64 = 0;
const DT_STRTAB: i64 = 5;
const DT_STRSZ: i64 = 10;
const DT_RPATH: i64 = 15;
const DT_RUNPATH: i64 = 29;

pub fn is_elf(content: &[u8]) -> bool {
    content.len() >= 4 && content[..4] == [0x7f, b'E', b'L', b'F']
}

/// What to relocate to. `gcc_current` applies brew's `lib/gcc/<N>` ->
/// `lib/gcc/current` rpath rewrite (disabled when pouring gcc itself).
pub struct LinkageOpts {
    pub prefix: String,
    pub cellar: String,
    pub gcc_current: bool,
}

impl LinkageOpts {
    pub fn for_formula(name: &str) -> Self {
        let is_gcc = name == "gcc" || name.starts_with("gcc@");
        LinkageOpts {
            prefix: super::prefix::prefix().to_string_lossy().to_string(),
            cellar: super::prefix::cellar().to_string_lossy().to_string(),
            gcc_current: !is_gcc,
        }
    }
}

fn rd_u16(b: &[u8], off: usize) -> Result<u16> {
    let s: [u8; 2] = b
        .get(off..off + 2)
        .ok_or_else(|| eyre::eyre!("truncated ELF"))?
        .try_into()?;
    Ok(u16::from_le_bytes(s))
}

fn rd_u32(b: &[u8], off: usize) -> Result<u32> {
    let s: [u8; 4] = b
        .get(off..off + 4)
        .ok_or_else(|| eyre::eyre!("truncated ELF"))?
        .try_into()?;
    Ok(u32::from_le_bytes(s))
}

fn rd_u64(b: &[u8], off: usize) -> Result<u64> {
    let s: [u8; 8] = b
        .get(off..off + 8)
        .ok_or_else(|| eyre::eyre!("truncated ELF"))?
        .try_into()?;
    Ok(u64::from_le_bytes(s))
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
    p_align: u64,
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
        let off = e_phoff + i * PHDR_SIZE;
        phdrs.push(Phdr {
            p_type: rd_u32(content, off)?,
            p_offset: rd_u64(content, off + 8)?,
            p_vaddr: rd_u64(content, off + 16)?,
            p_filesz: rd_u64(content, off + 32)?,
            p_memsz: rd_u64(content, off + 40)?,
            p_align: rd_u64(content, off + 48)?,
        });
    }
    Ok(phdrs)
}

fn vaddr_to_offset(phdrs: &[Phdr], vaddr: u64) -> Option<usize> {
    phdrs
        .iter()
        .find(|p| p.p_type == PT_LOAD && p.p_vaddr <= vaddr && vaddr < p.p_vaddr + p.p_filesz)
        .map(|p| (vaddr - p.p_vaddr + p.p_offset) as usize)
}

fn read_cstr(content: &[u8], off: usize) -> Result<String> {
    let bytes = content
        .get(off..)
        .ok_or_else(|| eyre::eyre!("string offset out of bounds"))?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| eyre::eyre!("unterminated string in ELF"))?;
    Ok(String::from_utf8_lossy(&bytes[..end]).to_string())
}

fn replace_placeholders(s: &str, opts: &LinkageOpts) -> String {
    s.replace(PLACEHOLDER_CELLAR, &opts.cellar)
        .replace(PLACEHOLDER_PREFIX, &opts.prefix)
}

/// brew's rpath rewrite (extend/os/linux/keg_relocate.rb#change_rpath!):
/// substitute the placeholder per component, rewrite versioned gcc lib dirs
/// to `current`, drop components outside the prefix (and not $ORIGIN-based),
/// and make sure `<prefix>/lib` is present.
fn new_rpath(old: &str, opts: &LinkageOpts) -> String {
    let lib_path = format!("{}/lib", opts.prefix);
    let mut components: Vec<String> = old
        .split(':')
        .map(|c| replace_placeholders(c, opts))
        .map(|c| {
            if opts.gcc_current
                && let Some(pos) = c.rfind("/lib/gcc/")
                && !c[pos + 9..].is_empty()
                && c[pos + 9..].bytes().all(|b| b.is_ascii_digit())
            {
                format!("{}current", &c[..pos + 9])
            } else {
                c
            }
        })
        .filter(|c| c.starts_with(&opts.prefix) || c.starts_with("$ORIGIN"))
        .collect();
    if !components.contains(&lib_path) {
        components.push(lib_path);
    }
    components.join(":")
}

fn round_up(v: u64, align: u64) -> u64 {
    v.div_ceil(align) * align
}

/// Patch the interpreter and rpath of one ELF file in memory. Returns whether
/// anything changed. No-op unless a bottling placeholder is present.
pub fn patch(content: &mut Vec<u8>, opts: &LinkageOpts, path: &Path) -> Result<bool> {
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
        Some(s) if s.contains(PLACEHOLDER_PREFIX) => Some(format!("{}/lib/ld.so", opts.prefix)),
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
        let end = start + dyn_seg.p_filesz as usize;
        let mut off = start;
        while off + 16 <= end.min(content.len()) {
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
    let old_rpath = match (strtab_off, rpath_strtab_off) {
        (Some(st), Some(rp)) => Some(read_cstr(content, st + rp as usize)?),
        _ => None,
    };
    let new_rpath_str = match &old_rpath {
        Some(s) if s.contains(PLACEHOLDER_PREFIX) || s.contains(PLACEHOLDER_CELLAR) => {
            Some(new_rpath(s, opts))
        }
        _ => None,
    };

    if new_interp.is_none() && new_rpath_str.is_none() {
        return Ok(false);
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
        && let (Some(st), Some(rp)) = (strtab_off, rpath_strtab_off)
    {
        let start = st + rp as usize;
        content[start..start + new.len()].copy_from_slice(new.as_bytes());
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

    let align = phdrs
        .iter()
        .filter(|p| p.p_type == PT_LOAD)
        .map(|p| p.p_align)
        .max()
        .unwrap_or(0x1000)
        .max(0x10000); // covers 4K/16K/64K runtime page sizes
    let new_off = round_up(content.len() as u64, align);
    let max_vaddr_end = phdrs
        .iter()
        .filter(|p| p.p_type == PT_LOAD)
        .map(|p| p.p_vaddr + p.p_memsz)
        .max()
        .unwrap_or(0);
    let new_vaddr = round_up(max_vaddr_end, align);

    let e_phnum = rd_u16(content, 56)? as usize;
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
        content[rp_start..rp_start + new.len()].copy_from_slice(new.as_bytes());
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
    if e_shoff != 0 && rd_u16(content, 58)? as usize == SHDR_SIZE {
        for i in 0..e_shnum {
            let off = e_shoff + i * SHDR_SIZE;
            if off + SHDR_SIZE > content.len() {
                break;
            }
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
mod tests {
    use super::*;

    const PREFIX: &str = "/home/linuxbrew/.linuxbrew";

    fn test_opts() -> LinkageOpts {
        LinkageOpts {
            prefix: PREFIX.to_string(),
            cellar: format!("{PREFIX}/Cellar"),
            gcc_current: true,
        }
    }

    /// minimal 64-bit LE ET_DYN ELF: PHDR + INTERP + LOAD + DYNAMIC headers,
    /// an interpreter string, a dynamic section with an rpath, and a dynstr
    fn synthetic_elf(interp: &str, rpath: &str) -> Vec<u8> {
        let phnum = 4;
        let phoff = EHDR_SIZE;
        let interp_off = phoff + phnum * PHDR_SIZE;
        let interp_len = interp.len() + 1;
        let dynstr_off = interp_off + interp_len;
        // dynstr: "\0<rpath>\0"
        let rpath_idx = 1u64;
        let dynstr_len = 1 + rpath.len() + 1;
        let dyn_off = dynstr_off + dynstr_len;
        let dyn_entries: Vec<(i64, u64)> = vec![
            (DT_STRTAB, dynstr_off as u64), // vaddr == offset in our LOAD
            (DT_STRSZ, dynstr_len as u64),
            (DT_RPATH, rpath_idx),
            (DT_NULL, 0),
        ];
        let total = dyn_off + dyn_entries.len() * 16;
        let mut elf = vec![0u8; total];
        elf[..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        elf[4] = 2; // 64-bit
        elf[5] = 1; // little-endian
        elf[6] = 1;
        wr_u16(&mut elf, 16, 3); // ET_DYN
        wr_u16(&mut elf, 18, 0xb7); // aarch64
        wr_u64(&mut elf, 32, phoff as u64);
        wr_u16(&mut elf, 52, EHDR_SIZE as u16);
        wr_u16(&mut elf, 54, PHDR_SIZE as u16);
        wr_u16(&mut elf, 56, phnum as u16);
        let mut write_phdr = |i: usize, p_type: u32, off: u64, sz: u64, align: u64| {
            let o = phoff + i * PHDR_SIZE;
            elf[o..o + 4].copy_from_slice(&p_type.to_le_bytes());
            elf[o + 4..o + 8].copy_from_slice(&PF_R.to_le_bytes());
            wr_u64(&mut elf, o + 8, off); // p_offset
            wr_u64(&mut elf, o + 16, off); // p_vaddr == p_offset
            wr_u64(&mut elf, o + 24, off);
            wr_u64(&mut elf, o + 32, sz);
            wr_u64(&mut elf, o + 40, sz);
            wr_u64(&mut elf, o + 48, align);
        };
        write_phdr(0, PT_PHDR, phoff as u64, (phnum * PHDR_SIZE) as u64, 8);
        write_phdr(1, PT_INTERP, interp_off as u64, interp_len as u64, 1);
        write_phdr(2, PT_LOAD, 0, total as u64, 0x1000);
        write_phdr(
            3,
            PT_DYNAMIC,
            dyn_off as u64,
            (dyn_entries.len() * 16) as u64,
            8,
        );
        elf[interp_off..interp_off + interp.len()].copy_from_slice(interp.as_bytes());
        elf[dynstr_off + 1..dynstr_off + 1 + rpath.len()].copy_from_slice(rpath.as_bytes());
        for (i, (tag, val)) in dyn_entries.iter().enumerate() {
            wr_u64(&mut elf, dyn_off + i * 16, *tag as u64);
            wr_u64(&mut elf, dyn_off + i * 16 + 8, *val);
        }
        elf
    }

    fn read_linkage(content: &[u8]) -> (String, String) {
        let phdrs = read_phdrs(content).unwrap();
        let interp = phdrs.iter().find(|p| p.p_type == PT_INTERP).unwrap();
        let interp_str = read_cstr(content, interp.p_offset as usize).unwrap();
        let dyn_seg = phdrs.iter().find(|p| p.p_type == PT_DYNAMIC).unwrap();
        let mut strtab = 0;
        let mut rpath_idx = 0;
        let mut off = dyn_seg.p_offset as usize;
        loop {
            let tag = rd_u64(content, off).unwrap() as i64;
            let val = rd_u64(content, off + 8).unwrap();
            match tag {
                DT_NULL => break,
                DT_STRTAB => strtab = val,
                DT_RPATH => rpath_idx = val,
                _ => {}
            }
            off += 16;
        }
        let strtab_off = vaddr_to_offset(&phdrs, strtab).unwrap();
        let rpath = read_cstr(content, strtab_off + rpath_idx as usize).unwrap();
        (interp_str, rpath)
    }

    #[test]
    fn test_patch_growing_appends_segment() {
        let mut elf = synthetic_elf(
            "@@HOMEBREW_PREFIX@@/lib/ld.so",
            "@@HOMEBREW_PREFIX@@/Cellar/xz/5.8.3/lib:@@HOMEBREW_PREFIX@@/opt/gcc/lib/gcc/current:@@HOMEBREW_PREFIX@@/lib",
        );
        let phnum_before = rd_u16(&elf, 56).unwrap();
        let changed = patch(&mut elf, &test_opts(), Path::new("test")).unwrap();
        assert!(changed);
        assert_eq!(rd_u16(&elf, 56).unwrap(), phnum_before + 1);
        let (interp, rpath) = read_linkage(&elf);
        assert_eq!(interp, format!("{PREFIX}/lib/ld.so"));
        assert_eq!(
            rpath,
            format!("{PREFIX}/Cellar/xz/5.8.3/lib:{PREFIX}/opt/gcc/lib/gcc/current:{PREFIX}/lib")
        );
        // the new segment is page-aligned and covered by a PT_LOAD
        let phdrs = read_phdrs(&elf).unwrap();
        let new_load = phdrs.iter().rev().find(|p| p.p_type == PT_LOAD).unwrap();
        let e_phoff = rd_u64(&elf, 32).unwrap();
        assert!(
            new_load.p_offset <= e_phoff && e_phoff < new_load.p_offset + new_load.p_filesz,
            "relocated phdr table must be covered by the new PT_LOAD"
        );
        assert_eq!(new_load.p_vaddr % new_load.p_align, 0);
        assert_eq!(new_load.p_offset % new_load.p_align, 0);
    }

    #[test]
    fn test_patch_shrinking_stays_in_place() {
        // a short prefix shrinks both strings: nothing moves
        let opts = LinkageOpts {
            prefix: "/hb".to_string(),
            cellar: "/hb/Cellar".to_string(),
            gcc_current: true,
        };
        let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", "@@HOMEBREW_PREFIX@@/lib");
        let len_before = elf.len();
        let phnum_before = rd_u16(&elf, 56).unwrap();
        let changed = patch(&mut elf, &opts, Path::new("test")).unwrap();
        assert!(changed);
        assert_eq!(elf.len(), len_before);
        assert_eq!(rd_u16(&elf, 56).unwrap(), phnum_before);
        let (interp, rpath) = read_linkage(&elf);
        assert_eq!(interp, "/hb/lib/ld.so");
        assert_eq!(rpath, "/hb/lib");
    }

    #[test]
    fn test_patch_noop_without_placeholders() {
        let mut elf = synthetic_elf("/lib64/ld-linux-x86-64.so.2", "/usr/lib");
        let before = elf.clone();
        let changed = patch(&mut elf, &test_opts(), Path::new("test")).unwrap();
        assert!(!changed);
        assert_eq!(elf, before);
    }

    #[test]
    fn test_patch_skips_non_elf() {
        let mut content = b"#!/bin/bash\necho hi\n".to_vec();
        let changed = patch(&mut content, &test_opts(), Path::new("test")).unwrap();
        assert!(!changed);
    }

    #[test]
    fn test_new_rpath_rules() {
        let opts = test_opts();
        // foreign components dropped, gcc versioned dir rewritten, lib appended
        assert_eq!(
            new_rpath(
                "@@HOMEBREW_PREFIX@@/opt/gcc/lib/gcc/15:/usr/lib:$ORIGIN/../lib",
                &opts
            ),
            format!("{PREFIX}/opt/gcc/lib/gcc/current:$ORIGIN/../lib:{PREFIX}/lib")
        );
        // lib not duplicated
        assert_eq!(
            new_rpath("@@HOMEBREW_PREFIX@@/lib", &opts),
            format!("{PREFIX}/lib")
        );
    }

    #[test]
    fn test_is_elf() {
        assert!(is_elf(&[0x7f, b'E', b'L', b'F', 2, 1]));
        assert!(!is_elf(b"#!/bin/bash"));
        assert!(!is_elf(&0xfeedfacf_u32.to_be_bytes()));
    }
}
