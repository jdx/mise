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
    let (interp, rpath) = read_linkage_bytes(content);
    (
        String::from_utf8(interp).unwrap(),
        String::from_utf8(rpath).unwrap(),
    )
}

fn read_linkage_bytes(content: &[u8]) -> (Vec<u8>, Vec<u8>) {
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
    let p_align = rd_u64(&elf, e_phoff as usize + (phdrs.len() - 1) * PHDR_SIZE + 48).unwrap();
    assert_eq!(p_align, NEW_SEGMENT_ALIGN);
    assert_eq!(new_load.p_vaddr % p_align, 0);
    assert_eq!(new_load.p_offset % p_align, 0);
}

/// file offset of program header `i` in a `synthetic_elf`
fn phdr_off(i: usize) -> usize {
    EHDR_SIZE + i * PHDR_SIZE
}

/// file offset of the d_val of the dynamic entry tagged `tag`
fn dyn_val_off(elf: &[u8], tag: i64) -> usize {
    let dyn_off = rd_u64(elf, phdr_off(3) + 8).unwrap() as usize;
    (0..)
        .map(|i| dyn_off + i * 16)
        .find(|off| rd_u64(elf, *off).unwrap() as i64 == tag)
        .unwrap()
        + 8
}

const GROWING_RPATH: &str = "@@HOMEBREW_PREFIX@@/Cellar/xz/5.8.3/lib:@@HOMEBREW_PREFIX@@/lib";

#[test]
fn test_patch_growing_pads_to_a_fixed_alignment() {
    // the file claims 16 MiB alignment for its PT_LOAD; the appended segment
    // is still padded to 64 KiB only
    let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", GROWING_RPATH);
    wr_u64(&mut elf, phdr_off(2) + 48, 1 << 24);
    assert!(patch(&mut elf, &test_opts(), Path::new("test")).unwrap());
    assert!(
        elf.len() < 2 * NEW_SEGMENT_ALIGN as usize,
        "{} bytes",
        elf.len()
    );
    let (interp, rpath) = read_linkage(&elf);
    assert_eq!(interp, format!("{PREFIX}/lib/ld.so"));
    assert_eq!(rpath, format!("{PREFIX}/Cellar/xz/5.8.3/lib:{PREFIX}/lib"));
}

#[test]
fn test_patch_growing_rejects_a_string_table_past_the_end() {
    for strsz in [1 << 24, u64::MAX] {
        let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", GROWING_RPATH);
        let off = dyn_val_off(&elf, DT_STRSZ);
        wr_u64(&mut elf, off, strsz);
        let len = elf.len();
        let err = patch(&mut elf, &test_opts(), Path::new("test")).unwrap_err();
        assert!(
            err.to_string().contains("runs past the end of the file"),
            "{err}"
        );
        assert_eq!(elf.len(), len);
    }
}

fn short_prefix_opts() -> LinkageOpts {
    LinkageOpts {
        prefix: "/hb".to_string(),
        cellar: "/hb/Cellar".to_string(),
        gcc_current: true,
    }
}

#[test]
fn test_patch_rejects_an_interpreter_past_the_end() {
    // a short prefix rewrites in place, the default prefix moves the string
    for opts in [short_prefix_opts(), test_opts()] {
        for filesz in [4096, u64::MAX] {
            let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", "@@HOMEBREW_PREFIX@@/lib");
            wr_u64(&mut elf, phdr_off(1) + 32, filesz);
            let before = elf.clone();
            let err = patch(&mut elf, &opts, Path::new("test")).unwrap_err();
            assert!(
                err.to_string().contains("PT_INTERP runs past the end"),
                "{err}"
            );
            assert_eq!(elf, before);
        }
    }
}

#[test]
fn test_patch_rejects_an_interpreter_without_a_nul_in_its_segment() {
    // PT_INTERP ends just before the NUL, so the string's end lies past the
    // segment; both the in-place and the moving rewrite must refuse it
    for opts in [short_prefix_opts(), test_opts()] {
        let interp = "@@HOMEBREW_PREFIX@@/lib/ld.so";
        let mut elf = synthetic_elf(interp, "@@HOMEBREW_PREFIX@@/lib");
        wr_u64(&mut elf, phdr_off(1) + 32, interp.len() as u64);
        let before = elf.clone();
        let err = patch(&mut elf, &opts, Path::new("test")).unwrap_err();
        assert!(
            err.to_string()
                .contains("interpreter does not lie inside PT_INTERP"),
            "{err}"
        );
        assert_eq!(elf, before);
    }
}

#[test]
fn test_patch_rejects_a_truncated_program_header() {
    // the table moved to the end of the file, cut 8 bytes short: the last
    // entry's p_align is missing
    let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", GROWING_RPATH);
    let phnum = rd_u16(&elf, 56).unwrap() as usize;
    let table = elf[phdr_off(0)..phdr_off(phnum) - 8].to_vec();
    let phoff = elf.len();
    elf.extend_from_slice(&table);
    wr_u64(&mut elf, 32, phoff as u64);
    let before = elf.clone();
    let err = patch(&mut elf, &test_opts(), Path::new("test")).unwrap_err();
    assert!(
        err.to_string().contains("truncated ELF program header"),
        "{err}"
    );
    assert_eq!(elf, before);
}

#[test]
fn test_patch_skips_a_section_table_whose_offset_wraps() {
    let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", GROWING_RPATH);
    wr_u64(&mut elf, 40, u64::MAX - 19); // e_shoff
    wr_u16(&mut elf, 58, SHDR_SIZE as u16); // e_shentsize
    wr_u16(&mut elf, 60, 1); // e_shnum
    assert!(patch(&mut elf, &test_opts(), Path::new("test")).unwrap());
    let (interp, _) = read_linkage(&elf);
    assert_eq!(interp, format!("{PREFIX}/lib/ld.so"));
}

#[test]
fn test_patch_rejects_an_rpath_offset_that_wraps() {
    let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", "@@HOMEBREW_PREFIX@@/lib");
    let off = dyn_val_off(&elf, DT_RPATH);
    wr_u64(&mut elf, off, u64::MAX);
    let err = patch(&mut elf, &test_opts(), Path::new("test")).unwrap_err();
    assert!(err.to_string().contains("out of bounds"), "{err}");
}

#[test]
fn test_patch_rejects_an_rpath_that_ends_past_the_string_table() {
    // DT_STRSZ ends inside the rpath, so its NUL and the bytes before it lie
    // past the table; an in-place rewrite would change them
    for opts in [short_prefix_opts(), test_opts()] {
        let mut elf = synthetic_elf("/lib64/ld-linux-x86-64.so.2", "@@HOMEBREW_PREFIX@@/lib");
        let off = dyn_val_off(&elf, DT_STRSZ);
        wr_u64(&mut elf, off, 5);
        let before = elf.clone();
        let err = patch(&mut elf, &opts, Path::new("test")).unwrap_err();
        assert!(
            err.to_string()
                .contains("rpath does not lie inside the dynamic string table"),
            "{err}"
        );
        assert_eq!(elf, before);
    }
}

#[test]
fn test_patch_leaves_a_truncated_section_table_unchanged() {
    // two section headers, of which only the first is in the file; it
    // describes the string table that moves
    let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", GROWING_RPATH);
    let dynstr_off = rd_u64(&elf, dyn_val_off(&elf, DT_STRTAB)).unwrap();
    let shoff = elf.len();
    elf.resize(shoff + SHDR_SIZE, 0);
    wr_u64(&mut elf, shoff + 24, dynstr_off);
    wr_u64(&mut elf, 40, shoff as u64); // e_shoff
    wr_u16(&mut elf, 58, SHDR_SIZE as u16); // e_shentsize
    wr_u16(&mut elf, 60, 2); // e_shnum
    let header = elf[shoff..shoff + SHDR_SIZE].to_vec();
    assert!(patch(&mut elf, &test_opts(), Path::new("test")).unwrap());
    assert_eq!(elf[shoff..shoff + SHDR_SIZE], header[..]);
    let (_, rpath) = read_linkage(&elf);
    assert_eq!(rpath, format!("{PREFIX}/Cellar/xz/5.8.3/lib:{PREFIX}/lib"));
}

#[test]
fn test_patch_rejects_header_fields_that_overflow() {
    // e_phoff, then the PT_LOAD's p_vaddr, p_filesz and p_memsz, then the
    // PT_DYNAMIC's p_filesz
    let fields = [
        32,
        phdr_off(2) + 16,
        phdr_off(2) + 32,
        phdr_off(2) + 40,
        phdr_off(3) + 32,
    ];
    for field in fields {
        let mut elf = synthetic_elf("@@HOMEBREW_PREFIX@@/lib/ld.so", GROWING_RPATH);
        wr_u64(&mut elf, field, u64::MAX);
        // an error or a relocated file, never a panic
        let _ = patch(&mut elf, &test_opts(), Path::new("test"));
    }
}

/// `synthetic_elf` whose rpath is `rpath` followed by 100 bytes of 0xff, and
/// the offset of those bytes
fn elf_with_non_utf8_rpath(rpath: &str) -> (Vec<u8>, usize) {
    let tail = "a".repeat(100);
    let mut elf = synthetic_elf("/lib64/ld-linux-x86-64.so.2", &format!("{rpath}{tail}"));
    let at = elf
        .windows(tail.len())
        .position(|w| w == tail.as_bytes())
        .unwrap();
    elf[at..at + tail.len()].fill(0xff);
    (elf, at)
}

#[test]
fn test_patch_rewrites_a_non_utf8_rpath_within_its_slot() {
    // the 0xff component is dropped, so the new rpath fits in place
    let (mut elf, at) = elf_with_non_utf8_rpath("@@HOMEBREW_PREFIX@@/lib:");
    let after = elf[at + 100..].to_vec();
    assert!(patch(&mut elf, &short_prefix_opts(), Path::new("test")).unwrap());
    assert_eq!(elf[at + 100..], after[..], "bytes after the rpath are kept");
    let (interp, rpath) = read_linkage(&elf);
    assert_eq!(interp, "/lib64/ld-linux-x86-64.so.2");
    assert_eq!(rpath, "/hb/lib");
}

#[test]
fn test_patch_keeps_non_utf8_rpath_bytes() {
    // the $ORIGIN component is kept byte for byte in the moved rpath
    let (mut elf, _) = elf_with_non_utf8_rpath("@@HOMEBREW_PREFIX@@/lib:$ORIGIN/");
    assert!(patch(&mut elf, &test_opts(), Path::new("test")).unwrap());
    let mut want = format!("\0{PREFIX}/lib:$ORIGIN/").into_bytes();
    want.extend([0xff; 100]);
    want.push(0);
    assert!(elf.windows(want.len()).any(|w| w == want));
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
            b"@@HOMEBREW_PREFIX@@/opt/gcc/lib/gcc/15:/usr/lib:$ORIGIN/../lib",
            &opts
        ),
        format!("{PREFIX}/opt/gcc/lib/gcc/current:$ORIGIN/../lib:{PREFIX}/lib").into_bytes()
    );
    // lib not duplicated
    assert_eq!(
        new_rpath(b"@@HOMEBREW_PREFIX@@/lib", &opts),
        format!("{PREFIX}/lib").into_bytes()
    );
}

#[test]
fn test_is_elf() {
    assert!(is_elf(&[0x7f, b'E', b'L', b'F', 2, 1]));
    assert!(!is_elf(b"#!/bin/bash"));
    assert!(!is_elf(&0xfeedfacf_u32.to_be_bytes()));
}
