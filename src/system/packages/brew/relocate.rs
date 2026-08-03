//! Replace Homebrew's bottling placeholders with real paths — the same work
//! `brew` does when pouring a bottle (Library/Homebrew/keg_relocate.rb).
//!
//! Because we always install at the canonical prefix, placeholder
//! replacements shrink or stay nearly the same size:
//!   @@HOMEBREW_PREFIX@@ (19) -> /opt/homebrew (13)
//!   @@HOMEBREW_CELLAR@@ (19) -> /opt/homebrew/Cellar (20)
//!
//! Text files get plain string replacement. For shebang executables with binary
//! payloads, such as zipapps, only the shebang is replaced so offsets and
//! checksums in the payload remain intact. Mach-O binaries get in-place C-string
//! replacement: the new string must fit in the existing string's slot (its
//! bytes plus any trailing NUL padding, keeping one terminator). Replacements
//! that shrink always fit; the +1-byte Cellar case fits unless the original
//! string ended exactly at its slot boundary, which we detect and report as an
//! error rather than corrupt the binary.

use std::path::{Path, PathBuf};

use eyre::bail;

use crate::result::Result;

pub struct Replacement {
    pub placeholder: &'static [u8],
    pub value: Vec<u8>,
}

pub fn standard_replacements() -> Vec<Replacement> {
    let prefix_buf = super::prefix::prefix();
    let prefix = prefix_buf.to_string_lossy();
    let repository_buf = super::prefix::repository();
    let repository = repository_buf.to_string_lossy();
    let macos = cfg!(target_os = "macos");
    vec![
        Replacement {
            placeholder: b"@@HOMEBREW_PREFIX@@",
            value: prefix.as_bytes().to_vec(),
        },
        Replacement {
            placeholder: b"@@HOMEBREW_CELLAR@@",
            value: format!("{prefix}/Cellar").into_bytes(),
        },
        Replacement {
            placeholder: b"@@HOMEBREW_REPOSITORY@@",
            value: repository.as_bytes().to_vec(),
        },
        Replacement {
            placeholder: b"@@HOMEBREW_LIBRARY@@",
            value: format!("{repository}/Library").into_bytes(),
        },
        Replacement {
            placeholder: b"@@HOMEBREW_PERL@@",
            // matches brew: system perl on macOS, brewed perl on Linux
            value: if macos {
                b"/usr/bin/perl".to_vec()
            } else {
                format!("{prefix}/opt/perl/bin/perl").into_bytes()
            },
        },
        Replacement {
            placeholder: b"@@HOMEBREW_JAVA@@",
            value: if macos {
                format!("{prefix}/opt/openjdk/libexec/openjdk.jdk/Contents/Home").into_bytes()
            } else {
                format!("{prefix}/opt/openjdk/libexec").into_bytes()
            },
        },
    ]
}

#[derive(Debug, Default)]
pub struct RelocationReport {
    /// files whose contents were modified
    pub changed_files: Vec<PathBuf>,
    /// modified Mach-O binaries that must be re-codesigned
    pub changed_machos: Vec<PathBuf>,
}

fn is_macho(content: &[u8]) -> bool {
    if content.len() < 4 {
        return false;
    }
    matches!(
        u32::from_be_bytes([content[0], content[1], content[2], content[3]]),
        0xfeedface | 0xcefaedfe | 0xfeedfacf | 0xcffaedfe | 0xcafebabe | 0xbebafeca
    )
}

/// Return the end of a valid shebang interpreter within Homebrew's 1 KiB
/// inspection window. The line ending is excluded from the returned range.
fn text_executable_shebang_end(content: &[u8]) -> Option<usize> {
    let prefix = content.get(..1024.min(content.len()))?;
    let rest = prefix.strip_prefix(b"#!")?;
    let line_end = rest
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(rest.len());
    let interpreter = &rest[..line_end];
    (!interpreter.contains(&0) && interpreter.iter().any(|b| !b.is_ascii_whitespace()))
        .then_some(2 + line_end)
}

fn contains_any_placeholder(content: &[u8], replacements: &[Replacement]) -> bool {
    replacements
        .iter()
        .any(|r| memmem(content, r.placeholder).is_some())
}

fn memmem(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Plain replacement for text files
fn replace_text(content: &[u8], replacements: &[Replacement]) -> Vec<u8> {
    let mut out = content.to_vec();
    for r in replacements {
        let mut result = Vec::with_capacity(out.len());
        let mut rest: &[u8] = &out;
        while let Some(pos) = memmem(rest, r.placeholder) {
            result.extend_from_slice(&rest[..pos]);
            result.extend_from_slice(&r.value);
            rest = &rest[pos + r.placeholder.len()..];
        }
        result.extend_from_slice(rest);
        out = result;
    }
    out
}

/// Replace placeholders only in the shebang preamble of a binary-backed text
/// executable. ZIP readers permit a variable-length preamble, while keeping
/// the archive bytes untouched preserves its offsets, sizes, and checksums.
fn replace_shebang(content: &[u8], shebang_end: usize, replacements: &[Replacement]) -> Vec<u8> {
    let shebang = replace_text(&content[..shebang_end], replacements);
    let mut out = Vec::with_capacity(shebang.len() + content.len() - shebang_end);
    out.extend_from_slice(&shebang);
    out.extend_from_slice(&content[shebang_end..]);
    out
}

/// In-place C-string replacement for binaries. Returns whether anything
/// changed; errors if a replacement can't fit in its slot.
fn replace_in_binary(
    content: &mut [u8],
    replacements: &[Replacement],
    path: &Path,
) -> Result<bool> {
    let mut changed = false;
    for r in replacements {
        let mut search_from = 0;
        while let Some(rel_pos) = memmem(&content[search_from..], r.placeholder) {
            let start = search_from + rel_pos;
            // the C-string containing this placeholder: backtrack is not
            // needed (placeholders start strings or follow path separators we
            // keep); find the end at the next NUL
            let str_end = content[start..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| start + p)
                .unwrap_or(content.len());
            // available slot: the string plus the run of NULs after it,
            // minus one NUL that must remain as terminator
            let slot_end = content[str_end..]
                .iter()
                .position(|&b| b != 0)
                .map(|p| str_end + p)
                .unwrap_or(content.len());
            let old = content[start..str_end].to_vec();
            let mut new = r.value.clone();
            new.extend_from_slice(&old[r.placeholder.len()..]);
            let slot = slot_end.saturating_sub(start);
            if new.len() + 1 > slot {
                bail!(
                    "cannot relocate {}: replacement for {} does not fit ({} > {} bytes)",
                    path.display(),
                    String::from_utf8_lossy(r.placeholder),
                    new.len() + 1,
                    slot,
                );
            }
            content[start..start + new.len()].copy_from_slice(&new);
            for b in &mut content[start + new.len()..slot_end] {
                *b = 0;
            }
            changed = true;
            search_from = start + new.len();
        }
    }
    Ok(changed)
}

/// Walk a poured keg and replace placeholders. `skip_linkage` leaves binary
/// linkage untouched while still relocating text files, matching Homebrew's
/// handling of `:any_skip_relocation` bottles.
pub fn relocate_keg(
    keg: &Path,
    formula_name: &str,
    skip_linkage: bool,
) -> Result<RelocationReport> {
    relocate_keg_with_replacements(keg, formula_name, skip_linkage, &standard_replacements())
}

fn relocate_keg_with_replacements(
    keg: &Path,
    formula_name: &str,
    skip_linkage: bool,
    replacements: &[Replacement],
) -> Result<RelocationReport> {
    let elf_opts = super::elf::LinkageOpts::for_formula(formula_name);
    // brew never patches glibc's own files — rewriting the dynamic linker
    // breaks it (extend/os/linux/keg_relocate.rb)
    let patch_elf = formula_name != "glibc" && !formula_name.starts_with("glibc@");
    let mut report = RelocationReport::default();
    for entry in walkdir::WalkDir::new(keg).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let content = crate::file::read(path)?;
        if !contains_any_placeholder(&content, &replacements) {
            continue;
        }
        let macho = is_macho(&content);
        let elf = cfg!(target_os = "linux") && super::elf::is_elf(&content);
        let shebang_end = text_executable_shebang_end(&content);
        if skip_linkage && (macho || elf || (content.contains(&0) && shebang_end.is_none())) {
            continue;
        }
        let perms = path.metadata()?.permissions();
        // bottle files are often read-only; lift that while we patch
        let mut writable = perms.clone();
        std::os::unix::fs::PermissionsExt::set_mode(
            &mut writable,
            std::os::unix::fs::PermissionsExt::mode(&perms) | 0o200,
        );
        std::fs::set_permissions(path, writable)?;
        if macho || (!elf && content.contains(&0) && shebang_end.is_none()) {
            // Non-ELF files containing NUL bytes are treated as binaries unless
            // their shebang makes them text executables (for example zipapps).
            // Binary replacement cannot shift offsets. Mach-O load commands
            // first: proper rewriting that can grow a command when the
            // replacement is longer; then the generic in-place pass for
            // strings in data sections.
            let mut content = content;
            let mut changed = macho && super::macho::patch(&mut content, &replacements, path)?;
            changed |= replace_in_binary(&mut content, &replacements, path)?;
            if changed {
                crate::file::write(path, &content)?;
                if macho {
                    report.changed_machos.push(path.to_path_buf());
                }
                report.changed_files.push(path.to_path_buf());
            }
        } else if elf {
            // Linux: patch the ELF interpreter and rpath, like brew's
            // relocate_dynamic_linkage. brew does not rewrite other strings
            // inside ELF binaries at pour time and neither do we — leftover
            // placeholder copies in abandoned string tables are unreferenced.
            if patch_elf {
                let mut content = content;
                if super::elf::patch(&mut content, &elf_opts, path)? {
                    crate::file::write(path, &content)?;
                    report.changed_files.push(path.to_path_buf());
                }
            }
        } else {
            let new_content = if content.contains(&0) {
                // A valid shebang is the only way a NUL-backed file reaches
                // this branch. Preserve the opaque binary payload byte-for-byte.
                replace_shebang(&content, shebang_end.unwrap(), &replacements)
            } else {
                replace_text(&content, &replacements)
            };
            if new_content != content {
                crate::file::write(path, &new_content)?;
                report.changed_files.push(path.to_path_buf());
            }
        }
        std::fs::set_permissions(path, perms)?;
    }
    Ok(report)
}

/// Ad-hoc re-sign modified Mach-O files — mandatory on arm64 macOS, where
/// the kernel kills binaries whose signature doesn't match their contents.
pub fn codesign(files: &[PathBuf]) -> Result<()> {
    for file in files {
        let res = crate::cmd::cmd(
            "/usr/bin/codesign",
            [
                "--sign",
                "-",
                "--force",
                "--preserve-metadata=entitlements,requirements,flags,runtime",
                &file.to_string_lossy(),
            ],
        )
        .stderr_capture()
        .stdout_capture()
        .unchecked()
        .run()?;
        if !res.status.success() {
            bail!(
                "codesign failed for {}: {}",
                file.display(),
                String::from_utf8_lossy(&res.stderr).trim()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    use std::os::unix::fs::PermissionsExt;

    /// fixed macOS-style replacements so tests behave the same on all hosts
    pub(in super::super) fn test_replacements() -> Vec<Replacement> {
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
    fn test_skip_linkage_still_relocates_text_files() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let text = tmp.path().join("script");
        let binary = tmp.path().join("binary");
        crate::file::write(&text, "CELLAR=@@HOMEBREW_CELLAR@@/formula/1.0\n")?;
        let mut binary_content = 0xfeedfacf_u32.to_be_bytes().to_vec();
        binary_content.extend_from_slice(b"@@HOMEBREW_PREFIX@@/lib/libformula.dylib\0");
        crate::file::write(&binary, &binary_content)?;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o444))?;

        let report =
            relocate_keg_with_replacements(tmp.path(), "formula", true, &test_replacements())?;

        assert_eq!(
            crate::file::read_to_string(&text)?,
            "CELLAR=/opt/homebrew/Cellar/formula/1.0\n"
        );
        assert_eq!(crate::file::read(&binary)?, binary_content);
        assert_eq!(binary.metadata()?.permissions().mode() & 0o777, 0o444);
        assert_eq!(report.changed_files, vec![text]);
        assert!(report.changed_machos.is_empty());
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
}
