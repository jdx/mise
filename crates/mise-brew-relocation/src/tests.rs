use super::*;
use std::io::{Cursor, Read, Write};
use std::os::unix::fs::PermissionsExt;

/// fixed macOS-style replacements so tests behave the same on all hosts
pub(crate) fn test_replacements() -> Vec<Replacement> {
    vec![
        Replacement {
            placeholder: b"@@HOMEBREW_PREFIX@@",
            value: b"/opt/homebrew".to_vec(),
        },
        Replacement {
            placeholder: b"@@HOMEBREW_CELLAR@@",
            value: b"/opt/homebrew/Cellar".to_vec(),
        },
    ]
}

#[test]
fn test_replace_text() {
    let replacements = test_replacements();
    let content = b"#!@@HOMEBREW_PREFIX@@/bin/bash\nCELLAR=@@HOMEBREW_CELLAR@@/foo\n";
    let out = replace_text(content, &replacements);
    assert_eq!(
        String::from_utf8_lossy(&out),
        "#!/opt/homebrew/bin/bash\nCELLAR=/opt/homebrew/Cellar/foo\n"
    );
}

#[test]
fn test_relocate_keg_uses_caller_paths() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let text = tmp.path().join("config");
    std::fs::write(
        &text,
        "prefix=@@HOMEBREW_PREFIX@@\nrepository=@@HOMEBREW_REPOSITORY@@\n",
    )?;

    let report = relocate_keg(
        tmp.path(),
        "formula",
        false,
        Path::new("/custom/brew"),
        Path::new("/custom/repo"),
    )?;

    assert_eq!(
        std::fs::read_to_string(&text)?,
        "prefix=/custom/brew\nrepository=/custom/repo\n"
    );
    assert_eq!(report.changed_files, vec![text]);
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn test_relocate_keg_uses_caller_prefix_for_elf_linkage() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let binary = tmp.path().join("binary");
    std::fs::write(
        &binary,
        elf::tests::synthetic_elf(
            "@@HOMEBREW_PREFIX@@/lib/ld.so",
            "@@HOMEBREW_PREFIX@@/Cellar/xz/lib",
        ),
    )?;

    let report = relocate_keg(
        tmp.path(),
        "xz",
        false,
        Path::new("/custom/linuxbrew"),
        Path::new("/custom/linuxbrew/Homebrew"),
    )?;

    assert_eq!(report.changed_files, vec![binary.clone()]);
    let (interpreter, rpath) = elf::tests::read_linkage(&std::fs::read(binary)?);
    assert_eq!(interpreter, "/custom/linuxbrew/lib/ld.so");
    assert_eq!(
        rpath,
        "/custom/linuxbrew/Cellar/xz/lib:/custom/linuxbrew/lib"
    );
    Ok(())
}

#[test]
fn test_skip_linkage_still_relocates_text_files() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let text = tmp.path().join("script");
    let binary = tmp.path().join("binary");
    std::fs::write(&text, "CELLAR=@@HOMEBREW_CELLAR@@/formula/1.0\n")?;
    let mut binary_content = 0xfeedfacf_u32.to_be_bytes().to_vec();
    binary_content.extend_from_slice(b"@@HOMEBREW_PREFIX@@/lib/libformula.dylib\0");
    std::fs::write(&binary, &binary_content)?;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o444))?;

    let report = relocate_keg_with_replacements(
        tmp.path(),
        "formula",
        true,
        Path::new("/opt/homebrew"),
        &test_replacements(),
    )?;

    assert_eq!(
        std::fs::read_to_string(&text)?,
        "CELLAR=/opt/homebrew/Cellar/formula/1.0\n"
    );
    assert_eq!(std::fs::read(&binary)?, binary_content);
    assert_eq!(binary.metadata()?.permissions().mode() & 0o777, 0o444);
    assert_eq!(report.changed_files, vec![text]);
    assert!(report.changed_machos.is_empty());
    Ok(())
}

#[test]
fn test_relocate_macho_hard_links() -> Result<()> {
    for skip_linkage in [false, true] {
        let tmp = tempfile::tempdir()?;
        let paths = ["fish", "fish_indent", "fish_key_reader"].map(|name| tmp.path().join(name));
        let mut content = vec![0; 32];
        content[..4].copy_from_slice(&0xfeedfacf_u32.to_le_bytes());
        content.extend_from_slice(b"@@HOMEBREW_PREFIX@@/lib/libpcre2.dylib\0");
        std::fs::write(&paths[0], &content)?;
        for path in &paths[1..] {
            std::fs::hard_link(&paths[0], path)?;
        }
        std::fs::set_permissions(&paths[0], std::fs::Permissions::from_mode(0o555))?;
        std::os::unix::fs::symlink(&paths[0], tmp.path().join("symlink"))?;
        let untouched = tmp.path().join("untouched");
        std::fs::write(&untouched, &content[..32])?;

        let mut report = relocate_keg_with_replacements(
            tmp.path(),
            "fish",
            skip_linkage,
            Path::new("/opt/homebrew"),
            &test_replacements(),
        )?;

        let expected = if skip_linkage { vec![] } else { paths.to_vec() };
        report.changed_files.sort();
        report.changed_machos.sort();
        assert_eq!(report.changed_files, expected);
        assert_eq!(report.changed_machos, expected);
        for path in &paths {
            let relocated = std::fs::read(path)?;
            if skip_linkage {
                assert_eq!(relocated, content);
            } else {
                assert!(!contains_any_placeholder(&relocated, &test_replacements()));
                assert!(memmem(&relocated, b"/opt/homebrew/lib/libpcre2.dylib").is_some());
            }
            assert_eq!(path.metadata()?.permissions().mode() & 0o777, 0o555);
        }
        assert_eq!(std::fs::read(&untouched)?, content[..32]);
    }
    Ok(())
}

#[test]
fn test_replace_text_executable_zipapp_with_long_prefix() {
    let shebang = b"#!@@HOMEBREW_PREFIX@@/opt/python@3.14/bin/python3.14\n";
    let mut cursor = Cursor::new(shebang.to_vec());
    cursor.set_position(shebang.len() as u64);
    let mut writer = zip::ZipWriter::new(cursor);
    writer
        .start_file(
            "__main__.py",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    let script = b"print('@@HOMEBREW_PREFIX@@ watchman-diag')\n";
    writer.write_all(script).unwrap();
    let content = writer.finish().unwrap().into_inner();

    assert!(content.contains(&0));
    let shebang_end = text_executable_shebang_end(&content).unwrap();
    let archive_before = &content[shebang_end..];

    let replacements = vec![Replacement {
        placeholder: b"@@HOMEBREW_PREFIX@@",
        value: b"/home/linuxbrew/.linuxbrew".to_vec(),
    }];
    let relocated = replace_shebang(&content, shebang_end, &replacements);
    assert!(
        relocated.starts_with(b"#!/home/linuxbrew/.linuxbrew/opt/python@3.14/bin/python3.14\n")
    );
    assert_eq!(relocated.len(), content.len() + 7);
    assert_eq!(&relocated[shebang_end + 7..], archive_before);

    let mut archive = zip::ZipArchive::new(Cursor::new(relocated)).unwrap();
    let mut relocated_script = Vec::new();
    archive
        .by_name("__main__.py")
        .unwrap()
        .read_to_end(&mut relocated_script)
        .unwrap();
    assert_eq!(relocated_script, script);
}

#[test]
fn test_text_executable_requires_shebang_interpreter() {
    assert_eq!(
        text_executable_shebang_end(b"#!/bin/sh\n\0payload"),
        Some(9)
    );
    assert!(text_executable_shebang_end(b"#!  /usr/bin/env python\n").is_some());
    assert!(text_executable_shebang_end(b"plain text\n").is_none());
    assert!(text_executable_shebang_end(b"#!   \t").is_none());
    assert!(text_executable_shebang_end(b"#!\n\0payload").is_none());
    assert!(text_executable_shebang_end(b"#!/bin/\0python\npayload").is_none());
}

#[test]
fn test_replace_in_binary_shrinking() {
    let replacements = test_replacements();
    // "@@HOMEBREW_PREFIX@@/lib/libx.dylib\0\0..." — replacement shrinks
    let mut content = b"@@HOMEBREW_PREFIX@@/lib/libx.dylib\0\0\0\0after".to_vec();
    let changed = replace_in_binary(&mut content, &replacements, Path::new("test")).unwrap();
    assert!(changed);
    assert_eq!(
        &content[..],
        b"/opt/homebrew/lib/libx.dylib\0\0\0\0\0\0\0\0\0\0after"
    );
}

#[test]
fn test_replace_in_binary_growing_fits_in_padding() {
    let replacements = test_replacements();
    // cellar replacement grows by 1 byte, fits because of trailing NUL padding
    let mut content = b"@@HOMEBREW_CELLAR@@/foo\0\0\0after".to_vec();
    let changed = replace_in_binary(&mut content, &replacements, Path::new("test")).unwrap();
    assert!(changed);
    assert_eq!(&content[..], b"/opt/homebrew/Cellar/foo\0\0after");
}

#[test]
fn test_replace_in_binary_growing_does_not_fit() {
    let replacements = test_replacements();
    // only one trailing NUL — the grown string + terminator can't fit
    let mut content = b"@@HOMEBREW_CELLAR@@/foo\0after".to_vec();
    let res = replace_in_binary(&mut content, &replacements, Path::new("test"));
    assert!(res.is_err());
}

#[test]
fn test_is_macho() {
    assert!(is_macho(&0xfeedfacf_u32.to_be_bytes()));
    assert!(is_macho(&0xcafebabe_u32.to_be_bytes()));
    assert!(!is_macho(b"#!/bin/bash"));
}
