//! Replace Homebrew's bottling placeholders with real paths — the same work
//! `brew` does when pouring a bottle (Library/Homebrew/keg_relocate.rb).
//!
//! Because we always install at the canonical prefix, placeholder
//! replacements shrink or stay nearly the same size:
//!   @@HOMEBREW_PREFIX@@ (19) -> /opt/homebrew (13)
//!   @@HOMEBREW_CELLAR@@ (19) -> /opt/homebrew/Cellar (20)
//!
//! Text files get plain string replacement. Mach-O binaries get in-place
//! C-string replacement: the new string must fit in the existing string's
//! slot (its bytes plus any trailing NUL padding, keeping one terminator).
//! Replacements that shrink always fit; the +1-byte Cellar case fits unless
//! the original string ended exactly at its slot boundary, which we detect
//! and report as an error rather than corrupt the binary.

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
            value: prefix.as_bytes().to_vec(),
        },
        Replacement {
            placeholder: b"@@HOMEBREW_LIBRARY@@",
            value: format!("{prefix}/Library").into_bytes(),
        },
        Replacement {
            placeholder: b"@@HOMEBREW_PERL@@",
            value: b"/usr/bin/perl".to_vec(),
        },
        Replacement {
            placeholder: b"@@HOMEBREW_JAVA@@",
            value: format!("{prefix}/opt/openjdk/libexec/openjdk.jdk/Contents/Home").into_bytes(),
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

/// Walk a poured keg and replace placeholders in all files.
pub fn relocate_keg(keg: &Path) -> Result<RelocationReport> {
    let replacements = standard_replacements();
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
        let perms = path.metadata()?.permissions();
        // bottle files are often read-only; lift that while we patch
        let mut writable = perms.clone();
        std::os::unix::fs::PermissionsExt::set_mode(
            &mut writable,
            std::os::unix::fs::PermissionsExt::mode(&perms) | 0o200,
        );
        std::fs::set_permissions(path, writable)?;
        let macho = is_macho(&content);
        // any file containing NUL bytes is treated as binary: in-place
        // replacement that can't shift offsets
        if macho || content.contains(&0) {
            let mut content = content;
            // Mach-O load commands first: proper rewriting that can grow a
            // command when the replacement is longer; then the generic
            // in-place pass for strings in data sections
            let mut changed = macho && super::macho::patch(&mut content, &replacements, path)?;
            changed |= replace_in_binary(&mut content, &replacements, path)?;
            if changed {
                crate::file::write(path, &content)?;
                if macho {
                    report.changed_machos.push(path.to_path_buf());
                }
                report.changed_files.push(path.to_path_buf());
            }
        } else {
            let new_content = replace_text(&content, &replacements);
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
mod tests {
    use super::*;

    #[test]
    fn test_replace_text() {
        let replacements = standard_replacements();
        let content = b"#!@@HOMEBREW_PREFIX@@/bin/bash\nCELLAR=@@HOMEBREW_CELLAR@@/foo\n";
        let out = replace_text(content, &replacements);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "#!/opt/homebrew/bin/bash\nCELLAR=/opt/homebrew/Cellar/foo\n"
        );
    }

    #[test]
    fn test_replace_in_binary_shrinking() {
        let replacements = standard_replacements();
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
        let replacements = standard_replacements();
        // cellar replacement grows by 1 byte, fits because of trailing NUL padding
        let mut content = b"@@HOMEBREW_CELLAR@@/foo\0\0\0after".to_vec();
        let changed = replace_in_binary(&mut content, &replacements, Path::new("test")).unwrap();
        assert!(changed);
        assert_eq!(&content[..], b"/opt/homebrew/Cellar/foo\0\0after");
    }

    #[test]
    fn test_replace_in_binary_growing_does_not_fit() {
        let replacements = standard_replacements();
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
