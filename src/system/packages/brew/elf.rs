//! Patch the dynamic linkage (PT_INTERP and DT_RPATH/DT_RUNPATH) of ELF
//! binaries when pouring Linux bottles — the same work `brew` does via its
//! PatchELF gem (Library/Homebrew/extend/os/linux/keg_relocate.rb).
//!
//! Linux bottles are built at /home/linuxbrew/.linuxbrew and bottled with
//! `@@HOMEBREW_PREFIX@@` placeholders written into the ELF interpreter and
//! rpath. Restoring the real prefix grows those strings (19 -> 26 bytes), and
//! unlike Mach-O there is no header padding to grow into. Homebrew uses the
//! PatchELF gem's `patchelf_compatible` saver. It keeps the program-header
//! table at the ELF header, moves every section that would overlap the grown
//! table plus the requested sections into a new PT_LOAD, then rewrites every
//! affected section, symbol, dynamic, and segment reference. This module
//! follows that layout so a mise pour and a Homebrew pour have the same ELF
//! topology.
//!
//! Scope: 64-bit little-endian ELF only (x86_64/aarch64 — the only Linux
//! bottle architectures).

use std::collections::BTreeMap;
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
const PT_NOTE: u32 = 4;
const PT_GNU_PROPERTY: u32 = 0x6474_e553;
const PF_R: u32 = 4;
const PF_W: u32 = 2;

const ET_DYN: u16 = 3;
const EM_AARCH64: u16 = 183;

const SHT_SYMTAB: u32 = 2;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8;
const SHT_REL: u32 = 9;
const SHT_DYNSYM: u32 = 11;
const SHT_NOTE: u32 = 7;
const STT_SECTION: u8 = 3;

const SHN_LORESERVE: u16 = 0xff00;

const DT_NULL: i64 = 0;
const DT_HASH: i64 = 4;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_STRSZ: i64 = 10;
const DT_RPATH: i64 = 15;
const DT_REL: i64 = 17;
const DT_JMPREL: i64 = 23;
const DT_RUNPATH: i64 = 29;
const DT_GNU_HASH: i64 = 0x6ffffef5;
const DT_VERSYM: i64 = 0x6ffffff0;
const DT_VERDEF: i64 = 0x6ffffffc;
const DT_VERNEED: i64 = 0x6ffffffe;

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
    pub(super) fn for_formula(name: &str) -> Self {
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

fn wr_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn wr_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

#[derive(Clone, Copy, Debug)]
struct Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

impl Phdr {
    fn write(self, content: &mut [u8], off: usize) {
        wr_u32(content, off, self.p_type);
        wr_u32(content, off + 4, self.p_flags);
        wr_u64(content, off + 8, self.p_offset);
        wr_u64(content, off + 16, self.p_vaddr);
        wr_u64(content, off + 24, self.p_paddr);
        wr_u64(content, off + 32, self.p_filesz);
        wr_u64(content, off + 40, self.p_memsz);
        wr_u64(content, off + 48, self.p_align);
    }
}

#[derive(Clone, Debug)]
struct Shdr {
    name: String,
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u64,
    sh_entsize: u64,
}

impl Shdr {
    fn write(&self, content: &mut [u8], off: usize) {
        wr_u32(content, off, self.sh_name);
        wr_u32(content, off + 4, self.sh_type);
        wr_u64(content, off + 8, self.sh_flags);
        wr_u64(content, off + 16, self.sh_addr);
        wr_u64(content, off + 24, self.sh_offset);
        wr_u64(content, off + 32, self.sh_size);
        wr_u32(content, off + 40, self.sh_link);
        wr_u32(content, off + 44, self.sh_info);
        wr_u64(content, off + 48, self.sh_addralign);
        wr_u64(content, off + 56, self.sh_entsize);
    }
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
            p_flags: rd_u32(content, off + 4)?,
            p_offset: rd_u64(content, off + 8)?,
            p_vaddr: rd_u64(content, off + 16)?,
            p_paddr: rd_u64(content, off + 24)?,
            p_filesz: rd_u64(content, off + 32)?,
            p_memsz: rd_u64(content, off + 40)?,
            p_align: rd_u64(content, off + 48)?,
        });
    }
    Ok(phdrs)
}

fn read_shdrs(content: &[u8]) -> Result<Vec<Shdr>> {
    let e_shoff = rd_u64(content, 40)? as usize;
    let e_shentsize = rd_u16(content, 58)? as usize;
    let e_shnum = rd_u16(content, 60)? as usize;
    let e_shstrndx = rd_u16(content, 62)? as usize;
    if e_shoff == 0 || e_shnum == 0 {
        bail!("ELF has no section table required for compatible relocation");
    }
    if e_shentsize != SHDR_SIZE {
        bail!("unexpected ELF e_shentsize {e_shentsize}");
    }
    if e_shstrndx >= e_shnum || e_shstrndx >= SHN_LORESERVE as usize {
        bail!("ELF uses an unsupported section-name table index");
    }
    e_shoff
        .checked_add(e_shnum * SHDR_SIZE)
        .filter(|end| *end <= content.len())
        .ok_or_else(|| eyre::eyre!("truncated ELF section table"))?;

    let string_header = e_shoff + e_shstrndx * SHDR_SIZE;
    let string_offset = rd_u64(content, string_header + 24)? as usize;
    let string_size = rd_u64(content, string_header + 32)? as usize;
    let string_end = string_offset
        .checked_add(string_size)
        .filter(|end| *end <= content.len())
        .ok_or_else(|| eyre::eyre!("truncated ELF section-name table"))?;
    let strings = &content[string_offset..string_end];

    let mut sections = Vec::with_capacity(e_shnum);
    for index in 0..e_shnum {
        let off = e_shoff + index * SHDR_SIZE;
        let sh_name = rd_u32(content, off)?;
        let name = if sh_name == 0 {
            String::new()
        } else {
            read_cstr(strings, sh_name as usize)?
        };
        sections.push(Shdr {
            name,
            sh_name,
            sh_type: rd_u32(content, off + 4)?,
            sh_flags: rd_u64(content, off + 8)?,
            sh_addr: rd_u64(content, off + 16)?,
            sh_offset: rd_u64(content, off + 24)?,
            sh_size: rd_u64(content, off + 32)?,
            sh_link: rd_u32(content, off + 40)?,
            sh_info: rd_u32(content, off + 44)?,
            sh_addralign: rd_u64(content, off + 48)?,
            sh_entsize: rd_u64(content, off + 56)?,
        });
    }
    Ok(sections)
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

fn section_index(sections: &[Shdr], name: &str) -> Option<usize> {
    sections.iter().position(|section| section.name == name)
}

fn section_data(content: &[u8], section: &Shdr) -> Result<Vec<u8>> {
    if section.sh_type == SHT_NOBITS {
        bail!("cannot materialize NOBITS section '{}'", section.name);
    }
    let start = section.sh_offset as usize;
    let end = start
        .checked_add(section.sh_size as usize)
        .filter(|end| *end <= content.len())
        .ok_or_else(|| eyre::eyre!("truncated ELF section '{}'", section.name))?;
    Ok(content[start..end].to_vec())
}

fn page_size(e_machine: u16) -> u64 {
    if e_machine == EM_AARCH64 {
        0x10000
    } else {
        0x1000
    }
}

fn sync_corresponding_segment(
    phdrs: &mut [Phdr],
    section: &Shdr,
    old_offset: u64,
    old_size: u64,
) -> Result<()> {
    let segment_type = match section.name.as_str() {
        ".interp" => Some(PT_INTERP),
        ".dynamic" => Some(PT_DYNAMIC),
        ".note.gnu.property" => Some(PT_GNU_PROPERTY),
        _ => None,
    };
    if let Some(segment_type) = segment_type {
        for phdr in phdrs.iter_mut().filter(|phdr| phdr.p_type == segment_type) {
            phdr.p_offset = section.sh_offset;
            phdr.p_vaddr = section.sh_addr;
            phdr.p_paddr = section.sh_addr;
            phdr.p_filesz = section.sh_size;
            phdr.p_memsz = section.sh_size;
        }
    }
    if section.sh_type == SHT_NOTE {
        for phdr in phdrs.iter_mut().filter(|phdr| phdr.p_type == PT_NOTE) {
            let segment_end = phdr.p_offset + phdr.p_filesz;
            let section_end = old_offset + old_size;
            let overlaps = (old_offset >= phdr.p_offset && old_offset < segment_end)
                || (section_end > phdr.p_offset && section_end <= segment_end);
            if !overlaps {
                continue;
            }
            if phdr.p_offset != old_offset || segment_end != section_end {
                bail!("unsupported overlap of SHT_NOTE and PT_NOTE");
            }
            phdr.p_offset = section.sh_offset;
            phdr.p_vaddr = section.sh_addr;
            phdr.p_paddr = section.sh_addr;
            phdr.p_filesz = section.sh_size;
            phdr.p_memsz = section.sh_size;
        }
    }
    Ok(())
}

fn dynamic_section_for_tag(sections: &[Shdr], tag: i64) -> Option<&Shdr> {
    let names: &[&str] = match tag {
        DT_STRTAB | DT_STRSZ => &[".dynstr"],
        DT_SYMTAB => &[".dynsym"],
        DT_HASH => &[".hash"],
        DT_GNU_HASH => &[".gnu.hash"],
        DT_JMPREL => &[".rel.plt", ".rela.plt", ".rela.IA_64.pltoff"],
        DT_REL => &[".rel.dyn", ".rel.got"],
        DT_RELA => &[".rela.dyn"],
        DT_VERDEF => &[".gnu.version_d"],
        DT_VERNEED => &[".gnu.version_r"],
        DT_VERSYM => &[".gnu.version"],
        _ => return None,
    };
    names
        .iter()
        .find_map(|name| section_index(sections, name).map(|index| &sections[index]))
}

fn normalize_note_segments(phdrs: &mut Vec<Phdr>, sections: &[Shdr]) -> Result<()> {
    let note_indices = phdrs
        .iter()
        .enumerate()
        .filter_map(|(index, phdr)| (phdr.p_type == PT_NOTE).then_some(index))
        .collect::<Vec<_>>();
    let mut additions = Vec::new();
    for index in note_indices {
        let original = phdrs[index];
        let start = original.p_offset;
        let end = start + original.p_filesz;
        let mut notes = sections
            .iter()
            .filter(|section| {
                section.sh_type == SHT_NOTE && section.sh_offset >= start && section.sh_offset < end
            })
            .collect::<Vec<_>>();
        if notes.is_empty() {
            continue;
        }
        notes.sort_by_key(|section| section.sh_offset);
        let mut current = start;
        for (note_index, section) in notes.into_iter().enumerate() {
            let alignment = section.sh_addralign.max(1);
            if section.sh_offset != round_up(current, alignment)
                || section.sh_size == 0
                || section.sh_offset + section.sh_size > end
            {
                bail!("cannot normalize non-contiguous PT_NOTE sections");
            }
            let mut normalized = original;
            normalized.p_offset = section.sh_offset;
            normalized.p_vaddr = original.p_vaddr + (section.sh_offset - start);
            normalized.p_paddr = original.p_paddr + (section.sh_offset - start);
            normalized.p_filesz = section.sh_size;
            normalized.p_memsz = section.sh_size;
            if note_index == 0 {
                phdrs[index] = normalized;
            } else {
                additions.push(normalized);
            }
            current = section.sh_offset + section.sh_size;
        }
        if current != end {
            bail!("cannot normalize partially mapped PT_NOTE sections");
        }
    }
    phdrs.extend(additions);
    Ok(())
}

fn sync_dynamic_tags(content: &mut [u8], sections: &[Shdr]) -> Result<()> {
    let Some(dynamic_index) = section_index(sections, ".dynamic") else {
        return Ok(());
    };
    let dynamic = &sections[dynamic_index];
    if dynamic.sh_type == SHT_NOBITS {
        return Ok(());
    }
    let start = dynamic.sh_offset as usize;
    let end = start
        .checked_add(dynamic.sh_size as usize)
        .filter(|end| *end <= content.len())
        .ok_or_else(|| eyre::eyre!("truncated ELF dynamic section"))?;
    let mut off = start;
    while off + 16 <= end {
        let tag = rd_u64(content, off)? as i64;
        if tag == DT_NULL {
            break;
        }
        if let Some(section) = dynamic_section_for_tag(sections, tag) {
            let value = if tag == DT_STRSZ {
                section.sh_size
            } else {
                section.sh_addr
            };
            wr_u64(content, off + 8, value);
        }
        off += 16;
    }
    Ok(())
}

fn rewrite_symbol_section_indices(
    content: &mut [u8],
    old_sections: &[Shdr],
    sections: &[Shdr],
) -> Result<()> {
    for section in sections
        .iter()
        .filter(|section| matches!(section.sh_type, SHT_SYMTAB | SHT_DYNSYM))
    {
        if section.sh_entsize != 24 || section.sh_size % section.sh_entsize != 0 {
            bail!("unsupported ELF symbol table layout in '{}'", section.name);
        }
        let start = section.sh_offset as usize;
        let end = start
            .checked_add(section.sh_size as usize)
            .filter(|end| *end <= content.len())
            .ok_or_else(|| eyre::eyre!("truncated ELF symbol table '{}'", section.name))?;
        let mut off = start;
        while off < end {
            let old_index = rd_u16(content, off + 6)?;
            if old_index != 0 && old_index < SHN_LORESERVE {
                let old_section = old_sections.get(old_index as usize).ok_or_else(|| {
                    eyre::eyre!("symbol refers to missing ELF section {old_index}")
                })?;
                let new_index = section_index(sections, &old_section.name)
                    .ok_or_else(|| eyre::eyre!("ELF section '{}' disappeared", old_section.name))?;
                let new_index = u16::try_from(new_index)?;
                wr_u16(content, off + 6, new_index);
                if content[off + 4] & 0xf == STT_SECTION {
                    wr_u64(content, off + 8, sections[new_index as usize].sh_addr);
                }
            }
            off += section.sh_entsize as usize;
        }
    }
    Ok(())
}

fn sort_and_write_sections(
    content: &mut [u8],
    old_sections: &[Shdr],
    sections: &mut [Shdr],
) -> Result<()> {
    let refs = old_sections
        .iter()
        .map(|section| {
            let link = (section.sh_link != 0)
                .then(|| {
                    old_sections
                        .get(section.sh_link as usize)
                        .map(|s| s.name.clone())
                })
                .flatten();
            let info = (section.sh_info != 0 && matches!(section.sh_type, SHT_REL | SHT_RELA))
                .then(|| {
                    old_sections
                        .get(section.sh_info as usize)
                        .map(|s| s.name.clone())
                })
                .flatten();
            (section.name.clone(), link, info)
        })
        .collect::<Vec<_>>();

    sections.sort_by_key(|section| section.sh_offset);
    let indices = sections
        .iter()
        .enumerate()
        .map(|(index, section)| (section.name.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for section in sections.iter_mut() {
        let (_, link, info) = refs
            .iter()
            .find(|(name, _, _)| name == &section.name)
            .ok_or_else(|| eyre::eyre!("ELF section '{}' disappeared", section.name))?;
        if section.sh_link != 0 {
            let name = link
                .as_ref()
                .ok_or_else(|| eyre::eyre!("invalid ELF sh_link in '{}'", section.name))?;
            section.sh_link = u32::try_from(
                *indices
                    .get(name)
                    .ok_or_else(|| eyre::eyre!("ELF linked section '{name}' disappeared"))?,
            )?;
        }
        if section.sh_info != 0 && matches!(section.sh_type, SHT_REL | SHT_RELA) {
            let name = info
                .as_ref()
                .ok_or_else(|| eyre::eyre!("invalid ELF sh_info in '{}'", section.name))?;
            section.sh_info = u32::try_from(
                *indices
                    .get(name)
                    .ok_or_else(|| eyre::eyre!("ELF info section '{name}' disappeared"))?,
            )?;
        }
    }

    let shstrndx = section_index(sections, ".shstrtab")
        .ok_or_else(|| eyre::eyre!("ELF section-name table disappeared"))?;
    wr_u16(content, 62, u16::try_from(shstrndx)?);
    let e_shoff = rd_u64(content, 40)? as usize;
    for (index, section) in sections.iter().enumerate() {
        section.write(content, e_shoff + index * SHDR_SIZE);
    }
    Ok(())
}

fn compatible_grow(
    content: &mut Vec<u8>,
    move_interp: Option<&str>,
    move_dynstr: Option<&str>,
    old_rpath: Option<(usize, usize)>,
    rpath_val_offs: &[usize],
    path: &Path,
) -> Result<()> {
    if rd_u16(content, 16)? != ET_DYN {
        bail!("cannot compatibly grow non-ET_DYN ELF {}", path.display());
    }
    let mut phdrs = read_phdrs(content)?;
    let mut sections = read_shdrs(content)?;
    let old_sections = sections.clone();
    let mut replacements = BTreeMap::<String, Vec<u8>>::new();

    if let Some(interp) = move_interp {
        let mut data = interp.as_bytes().to_vec();
        data.push(0);
        replacements.insert(".interp".to_string(), data);
    }
    if let Some(rpath) = move_dynstr {
        let dynstr_index = section_index(&sections, ".dynstr")
            .ok_or_else(|| eyre::eyre!("ELF is missing .dynstr"))?;
        let old_size = sections[dynstr_index].sh_size as usize;
        let (rpath_start, rpath_size) =
            old_rpath.ok_or_else(|| eyre::eyre!("ELF rpath location is unknown"))?;
        content[rpath_start..rpath_start + rpath_size].fill(b'X');
        let mut data = section_data(content, &sections[dynstr_index])?;
        data.extend_from_slice(rpath.as_bytes());
        data.push(0);
        replacements.insert(".dynstr".to_string(), data);
        for offset in rpath_val_offs {
            wr_u64(content, *offset, old_size as u64);
        }
    }

    let note_count = sections
        .iter()
        .filter(|section| section.sh_type == SHT_NOTE)
        .count();
    let pht_size = EHDR_SIZE + (phdrs.len() + note_count + 1) * PHDR_SIZE;
    for (index, section) in sections.iter().enumerate() {
        if index == 0 || replacements.contains_key(&section.name) {
            continue;
        }
        if section.sh_offset > pht_size as u64 {
            break;
        }
        replacements.insert(section.name.clone(), section_data(content, section)?);
    }
    if replacements.is_empty() {
        bail!("compatible ELF growth requested without replacement sections");
    }
    let replaces_notes = replacements.keys().any(|name| {
        section_index(&sections, name).is_some_and(|index| sections[index].sh_type == SHT_NOTE)
    });

    // Elf64_Off is eight bytes; PatchELF uses that field width for the
    // replacement-section alignment.
    let section_alignment = 8;
    let needed_space = replacements.values().try_fold(0u64, |size, data| {
        size.checked_add(round_up(data.len() as u64, section_alignment))
            .ok_or_else(|| eyre::eyre!("ELF replacement size overflow"))
    })?;
    let page = page_size(rd_u16(content, 18)?);
    let start_offset = round_up(content.len() as u64, page);
    let mut start_page = phdrs
        .iter()
        .map(|phdr| round_up(phdr.p_vaddr.saturating_add(phdr.p_memsz), page))
        .max()
        .unwrap_or(0);
    let first_page = phdrs
        .iter()
        .find(|phdr| phdr.p_type == PT_PHDR)
        .map(|phdr| phdr.p_vaddr.saturating_sub(phdr.p_offset))
        .unwrap_or(0);
    if phdrs.iter().any(|phdr| phdr.p_type == PT_INTERP) && start_offset > start_page {
        start_page = start_offset;
    }
    let final_size = start_offset
        .checked_add(needed_space)
        .and_then(|size| usize::try_from(size).ok())
        .ok_or_else(|| eyre::eyre!("ELF replacement size overflow"))?;
    content.resize(final_size, 0);

    phdrs.push(Phdr {
        p_type: PT_LOAD,
        p_flags: PF_R | PF_W,
        p_offset: start_offset,
        p_vaddr: start_page,
        p_paddr: start_page,
        p_filesz: needed_space,
        p_memsz: needed_space,
        p_align: page,
    });
    if replaces_notes {
        normalize_note_segments(&mut phdrs, &sections)?;
    }
    wr_u64(content, 32, EHDR_SIZE as u64);
    wr_u16(content, 56, u16::try_from(phdrs.len())?);

    for name in replacements.keys() {
        let index = section_index(&sections, name)
            .ok_or_else(|| eyre::eyre!("ELF replacement section '{name}' is missing"))?;
        let section = &sections[index];
        if section.sh_type != SHT_NOBITS {
            let start = section.sh_offset as usize;
            let end = start + section.sh_size as usize;
            content[start..end].fill(b'X');
        }
    }

    let mut current_offset = start_offset;
    for (name, data) in replacements {
        let index = section_index(&sections, &name)
            .ok_or_else(|| eyre::eyre!("ELF replacement section '{name}' is missing"))?;
        let start = current_offset as usize;
        content[start..start + data.len()].copy_from_slice(&data);
        let section = &mut sections[index];
        let old_offset = section.sh_offset;
        let old_size = section.sh_size;
        section.sh_offset = current_offset;
        section.sh_addr = start_page + (current_offset - start_offset);
        section.sh_size = data.len() as u64;
        if section.sh_type != SHT_NOTE || section.sh_addralign > section_alignment {
            section.sh_addralign = section_alignment;
        }
        sync_corresponding_segment(&mut phdrs, section, old_offset, old_size)?;
        current_offset += round_up(data.len() as u64, section_alignment);
    }
    if current_offset != start_offset + needed_space {
        bail!("ELF replacement layout size mismatch");
    }

    let phdr_table_size = (phdrs.len() * PHDR_SIZE) as u64;
    for phdr in phdrs.iter_mut().filter(|phdr| phdr.p_type == PT_PHDR) {
        phdr.p_offset = EHDR_SIZE as u64;
        phdr.p_vaddr = first_page + EHDR_SIZE as u64;
        phdr.p_paddr = phdr.p_vaddr;
        phdr.p_filesz = phdr_table_size;
        phdr.p_memsz = phdr.p_filesz;
    }
    phdrs.sort_by(|left, right| {
        if right.p_type == PT_PHDR {
            std::cmp::Ordering::Greater
        } else if left.p_type == PT_PHDR {
            std::cmp::Ordering::Less
        } else {
            left.p_paddr.cmp(&right.p_paddr)
        }
    });
    for (index, phdr) in phdrs.iter().copied().enumerate() {
        phdr.write(content, EHDR_SIZE + index * PHDR_SIZE);
    }

    sort_and_write_sections(content, &old_sections, &mut sections)?;
    sync_dynamic_tags(content, &sections)?;
    rewrite_symbol_section_indices(content, &old_sections, &sections)?;
    Ok(())
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
        Some(s) if s.contains(PLACEHOLDER_PREFIX) => Some(format!("{}/lib/ld.so", opts.prefix)),
        _ => None,
    };

    // current rpath via the dynamic section
    let dynamic = phdrs.iter().find(|p| p.p_type == PT_DYNAMIC).copied();
    let mut strtab_vaddr = None;
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
    // Homebrew's patchelf-compatible saver always relocates a changed
    // interpreter section, even when the new path is shorter.
    let interp_in_place = new_interp.is_none();
    let rpath_in_place = match (&old_rpath, &new_rpath_str) {
        (Some(old), Some(new)) => new.len() <= old.len(),
        _ => true,
    };

    if rpath_in_place
        && let (Some(old), Some(new)) = (&old_rpath, &new_rpath_str)
        && let (Some(st), Some(rp)) = (strtab_off, rpath_strtab_off)
    {
        let start = st + rp as usize;
        content[start..start + old.len()].fill(b'X');
        content[start..start + new.len()].copy_from_slice(new.as_bytes());
        content[start + new.len()] = 0;
    }
    if interp_in_place && rpath_in_place {
        return Ok(true);
    }

    let move_interp = !interp_in_place;
    let move_dynstr = !rpath_in_place;
    if move_dynstr && strtab_off.is_none() {
        bail!(
            "cannot relocate {}: rpath must grow but the dynamic string table \
             could not be located",
            path.display()
        );
    }

    let old_rpath_location = match (&old_rpath, strtab_off, rpath_strtab_off) {
        (Some(old), Some(strtab), Some(rpath)) => Some((strtab + rpath as usize, old.len())),
        _ => None,
    };
    compatible_grow(
        content,
        move_interp.then_some(new_interp.as_deref()).flatten(),
        move_dynstr.then_some(new_rpath_str.as_deref()).flatten(),
        old_rpath_location,
        &rpath_val_offs,
        path,
    )?;

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

    /// Minimal 64-bit LE ET_DYN ELF with the sections required by
    /// Homebrew's patchelf-compatible saver.
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
        let dynamic_size = dyn_entries.len() * 16;
        let shstr = b"\0.interp\0.dynstr\0.dynamic\0.shstrtab\0";
        let shstr_off = dyn_off + dynamic_size;
        let shoff = round_up((shstr_off + shstr.len()) as u64, 8) as usize;
        let shnum = 5;
        let total = shoff + shnum * SHDR_SIZE;
        let mut elf = vec![0u8; total];
        elf[..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        elf[4] = 2; // 64-bit
        elf[5] = 1; // little-endian
        elf[6] = 1;
        wr_u16(&mut elf, 16, 3); // ET_DYN
        wr_u16(&mut elf, 18, 0xb7); // aarch64
        wr_u64(&mut elf, 32, phoff as u64);
        wr_u64(&mut elf, 40, shoff as u64);
        wr_u16(&mut elf, 52, EHDR_SIZE as u16);
        wr_u16(&mut elf, 54, PHDR_SIZE as u16);
        wr_u16(&mut elf, 56, phnum as u16);
        wr_u16(&mut elf, 58, SHDR_SIZE as u16);
        wr_u16(&mut elf, 60, shnum as u16);
        wr_u16(&mut elf, 62, 4);
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
        elf[shstr_off..shstr_off + shstr.len()].copy_from_slice(shstr);
        let mut write_shdr = |index: usize,
                              name: u32,
                              sh_type: u32,
                              flags: u64,
                              addr: u64,
                              offset: u64,
                              size: u64,
                              link: u32,
                              align: u64,
                              entsize: u64| {
            let off = shoff + index * SHDR_SIZE;
            wr_u32(&mut elf, off, name);
            wr_u32(&mut elf, off + 4, sh_type);
            wr_u64(&mut elf, off + 8, flags);
            wr_u64(&mut elf, off + 16, addr);
            wr_u64(&mut elf, off + 24, offset);
            wr_u64(&mut elf, off + 32, size);
            wr_u32(&mut elf, off + 40, link);
            wr_u64(&mut elf, off + 48, align);
            wr_u64(&mut elf, off + 56, entsize);
        };
        write_shdr(
            1,
            1,
            1,
            2,
            interp_off as u64,
            interp_off as u64,
            interp_len as u64,
            0,
            1,
            0,
        );
        write_shdr(
            2,
            9,
            3,
            2,
            dynstr_off as u64,
            dynstr_off as u64,
            dynstr_len as u64,
            0,
            1,
            0,
        );
        write_shdr(
            3,
            17,
            6,
            3,
            dyn_off as u64,
            dyn_off as u64,
            dynamic_size as u64,
            2,
            8,
            16,
        );
        write_shdr(
            4,
            26,
            3,
            0,
            0,
            shstr_off as u64,
            shstr.len() as u64,
            0,
            1,
            0,
        );
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
        // The program-header table stays at the ELF header. Replacement
        // sections live in a new page-aligned PT_LOAD.
        let phdrs = read_phdrs(&elf).unwrap();
        let new_load = phdrs.iter().rev().find(|p| p.p_type == PT_LOAD).unwrap();
        let e_phoff = rd_u64(&elf, 32).unwrap();
        assert_eq!(e_phoff, EHDR_SIZE as u64);
        assert!(e_phoff < new_load.p_offset);
        assert_eq!(new_load.p_vaddr % new_load.p_align, 0);
        assert_eq!(new_load.p_offset % new_load.p_align, 0);
    }

    #[test]
    fn test_patch_shorter_interpreter_uses_compatible_layout() {
        // PatchELF's compatible saver relocates a changed interpreter even
        // when the replacement is shorter.
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
        assert!(elf.len() > len_before);
        assert_eq!(rd_u16(&elf, 56).unwrap(), phnum_before + 1);
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
