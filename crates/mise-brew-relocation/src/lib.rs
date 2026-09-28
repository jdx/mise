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

#![cfg(unix)]

mod elf;
mod macho;

use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};

use eyre::Result;

struct Replacement {
    pub placeholder: &'static [u8],
    pub value: Vec<u8>,
}

fn standard_replacements(prefix: &Path, repository: &Path) -> Vec<Replacement> {
    let prefix = prefix.to_string_lossy();
    let repository = repository.to_string_lossy();
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
    prefix: &Path,
    repository: &Path,
) -> Result<RelocationReport> {
    relocate_keg_with_replacements(
        keg,
        formula_name,
        skip_linkage,
        prefix,
        &standard_replacements(prefix, repository),
    )
}

fn relocate_keg_with_replacements(
    keg: &Path,
    formula_name: &str,
    skip_linkage: bool,
    prefix: &Path,
    replacements: &[Replacement],
) -> Result<RelocationReport> {
    let elf_opts = elf::LinkageOpts::for_formula(formula_name, prefix);
    // brew never patches glibc's own files — rewriting the dynamic linker
    // breaks it (extend/os/linux/keg_relocate.rb)
    let patch_elf = formula_name != "glibc" && !formula_name.starts_with("glibc@");
    let mut report = RelocationReport::default();
    let mut changed_macho_inodes = HashSet::new();
    for entry in walkdir::WalkDir::new(keg).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let metadata = path.metadata()?;
        let inode = (metadata.dev(), metadata.ino());
        // codesign can replace an inode, so every alias needs its own signature
        if changed_macho_inodes.contains(&inode) {
            report.changed_machos.push(path.to_path_buf());
            report.changed_files.push(path.to_path_buf());
            continue;
        }
        let content =
            std::fs::read(path).wrap_err_with(|| format!("failed read: {}", path.display()))?;
        if !contains_any_placeholder(&content, replacements) {
            continue;
        }
        let macho = is_macho(&content);
        let elf = cfg!(target_os = "linux") && elf::is_elf(&content);
        let shebang_end = text_executable_shebang_end(&content);
        if skip_linkage && (macho || elf || (content.contains(&0) && shebang_end.is_none())) {
            continue;
        }
        let perms = metadata.permissions();
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
            let mut changed = macho && macho::patch(&mut content, replacements, path)?;
            changed |= replace_in_binary(&mut content, replacements, path)?;
            if changed {
                std::fs::write(path, &content)
                    .wrap_err_with(|| format!("failed write: {}", path.display()))?;
                if macho {
                    changed_macho_inodes.insert(inode);
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
                if elf::patch(&mut content, &elf_opts, path)? {
                    std::fs::write(path, &content)
                        .wrap_err_with(|| format!("failed write: {}", path.display()))?;
                    report.changed_files.push(path.to_path_buf());
                }
            }
        } else {
            let new_content = if content.contains(&0) {
                // A valid shebang is the only way a NUL-backed file reaches
                // this branch. Preserve the opaque binary payload byte-for-byte.
                replace_shebang(&content, shebang_end.unwrap(), replacements)
            } else {
                replace_text(&content, replacements)
            };
            if new_content != content {
                std::fs::write(path, &new_content)
                    .wrap_err_with(|| format!("failed write: {}", path.display()))?;
                report.changed_files.push(path.to_path_buf());
            }
        }
        std::fs::set_permissions(path, perms)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
