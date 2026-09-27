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
pub(crate) fn synthetic_elf(interp: &str, rpath: &str) -> Vec<u8> {
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

pub(crate) fn read_linkage(content: &[u8]) -> (String, String) {
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
