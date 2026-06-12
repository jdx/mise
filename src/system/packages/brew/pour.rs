//! Pour a bottle: extract -> relocate -> codesign -> receipt -> link.

use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};
use serde_json::json;

use super::api::BottleFile;
use super::prefix;
use super::relocate;
use super::resolve::ResolvedFormula;
use crate::file::{TarFormat, TarOptions};
use crate::result::Result;

/// directories linked from a keg into the prefix (brew's Keg::KEG_LINK_DIRECTORIES,
/// minus etc/var which brew handles specially and we defer)
const LINK_DIRS: &[&str] = &["bin", "sbin", "include", "lib", "share", "Frameworks"];

pub fn keg_path(name: &str, pkg_version: &str) -> PathBuf {
    prefix::cellar().join(name).join(pkg_version)
}

/// is this keg fully poured? (incomplete pours live in .mise-tmp-* dirs)
pub fn keg_installed(name: &str, pkg_version: &str) -> bool {
    keg_path(name, pkg_version).exists()
}

/// installed versions of this formula; the active keg (per the `opt`
/// symlink, like brew) first, the rest name-sorted
pub fn installed_versions(name: &str) -> Vec<String> {
    let dir = prefix::cellar().join(name);
    let mut versions: Vec<String> = crate::file::ls(&dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().to_string();
            (!name.starts_with(".mise-")).then_some(name)
        })
        .collect();
    versions.sort();
    let opt_target = std::fs::read_link(prefix::prefix().join("opt").join(name))
        .ok()
        .and_then(|t| t.file_name().map(|f| f.to_string_lossy().to_string()));
    if let Some(active) = opt_target
        && let Some(pos) = versions.iter().position(|v| v == &active)
    {
        versions.swap(0, pos);
    }
    versions
}

pub async fn pour(
    rf: &ResolvedFormula,
    tag: &str,
    bottle: &BottleFile,
    tarball: &Path,
    closure: &[ResolvedFormula],
) -> Result<()> {
    let name = &rf.formula.name;
    let pkg_version = rf.formula.pkg_version()?;
    let keg = keg_path(name, &pkg_version);
    let rack = keg.parent().unwrap().to_path_buf();
    let tmp = rack.join(format!(".mise-tmp-{pkg_version}"));
    let scratch = rack.join(format!(".mise-extract-{pkg_version}"));
    for dir in [&tmp, &scratch] {
        if dir.exists() {
            crate::file::remove_all(dir)?;
        }
    }
    crate::file::create_dir_all(&scratch)?;

    // bottle tarballs contain <name>/<pkg_version>/...
    crate::file::untar(
        tarball,
        &scratch,
        &TarOptions {
            format: TarFormat::TarGz,
            strip_components: 0,
            pr: None,
            preserve_mtime: true,
        },
    )
    .wrap_err_with(|| format!("failed to extract bottle for {name}"))?;
    let inner = scratch.join(name).join(&pkg_version);
    if !inner.exists() {
        bail!("unexpected bottle layout for {name}: missing {name}/{pkg_version} in archive");
    }
    crate::file::rename(&inner, &tmp)?;
    crate::file::remove_all(&scratch)?;

    // ":any_skip_relocation" bottles contain no embedded paths at all
    let report = if bottle.cellar == ":any_skip_relocation" {
        relocate::RelocationReport::default()
    } else {
        relocate::relocate_keg(&tmp)?
    };
    // arm64 macOS kills binaries whose signature doesn't match; Linux ELF
    // files have no signatures to fix
    if cfg!(target_os = "macos") && !report.changed_machos.is_empty() {
        relocate::codesign(&report.changed_machos)
            .wrap_err_with(|| format!("failed to re-sign relocated binaries for {name}"))?;
    }

    write_receipt(rf, tag, &tmp, &report, closure)?;

    if keg.exists() {
        crate::file::remove_all(&keg)?;
    }
    crate::file::rename(&tmp, &keg)?;
    // never leave a half-installed keg: if linking fails (conflicts, IO),
    // remove the keg so the next install retries from scratch
    if let Err(err) = link_keg(name, &pkg_version, rf.formula.keg_only) {
        let _ = crate::file::remove_all(&keg);
        return Err(err);
    }
    Ok(())
}

/// brew-compatible INSTALL_RECEIPT.json so a later-installed real Homebrew
/// adopts these kegs (brew list/upgrade/uninstall all work)
fn write_receipt(
    rf: &ResolvedFormula,
    tag: &str,
    keg: &Path,
    report: &relocate::RelocationReport,
    closure: &[ResolvedFormula],
) -> Result<()> {
    let runtime_dependencies: Vec<serde_json::Value> = closure
        .iter()
        .filter(|other| {
            rf.formula
                .dependencies_for(tag)
                .iter()
                .any(|d| d == &other.formula.name || other.formula.aliases.contains(d))
        })
        .filter_map(|dep| {
            let pkg_version = dep.formula.pkg_version().ok()?;
            Some(json!({
                "full_name": dep.formula.name,
                "version": dep.formula.versions.stable,
                "revision": dep.formula.revision,
                "pkg_version": pkg_version,
                "declared_directly": true,
            }))
        })
        .collect();
    let changed_files: Vec<String> = report
        .changed_files
        .iter()
        .filter_map(|p| p.strip_prefix(keg).ok())
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let receipt = json!({
        "homebrew_version": "4.0.0 (mise)",
        "used_options": [],
        "unused_options": [],
        "built_as_bottle": true,
        "poured_from_bottle": true,
        "loaded_from_api": true,
        "installed_as_dependency": !rf.on_request,
        "installed_on_request": rf.on_request,
        "changed_files": changed_files,
        "time": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "source_modified_time": 0,
        "compiler": "clang",
        "aliases": rf.formula.aliases,
        "runtime_dependencies": runtime_dependencies,
        "source": {
            "spec": "stable",
            "versions": {
                "stable": rf.formula.versions.stable,
                "head": null,
                "version_scheme": 0,
            },
            "path": null,
            "tap": "homebrew/core",
            "tap_git_head": null,
        },
        "arch": if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" },
        "built_on": {},
    });
    crate::file::write(
        keg.join("INSTALL_RECEIPT.json"),
        serde_json::to_string(&receipt)?,
    )?;
    Ok(())
}

/// relative symlink target from `link` to `dest`
fn relative_target(dest: &Path, link: &Path) -> PathBuf {
    let link_dir = link.parent().unwrap();
    let mut common = 0;
    let dest_parts: Vec<_> = dest.components().collect();
    let link_parts: Vec<_> = link_dir.components().collect();
    while common < dest_parts.len()
        && common < link_parts.len()
        && dest_parts[common] == link_parts[common]
    {
        common += 1;
    }
    let mut out = PathBuf::new();
    for _ in common..link_parts.len() {
        out.push("..");
    }
    for part in &dest_parts[common..] {
        out.push(part);
    }
    out
}

/// May we overwrite `dest`? Only if it's a symlink pointing into our Cellar
/// or opt (i.e. something brew/mise created and can re-create).
fn can_overwrite(dest: &Path) -> bool {
    let Ok(meta) = dest.symlink_metadata() else {
        return true; // doesn't exist
    };
    if !meta.is_symlink() {
        return false;
    }
    let target = match std::fs::read_link(dest) {
        Ok(t) => t,
        Err(err) => {
            // treat as a conflict (never clobber what we can't identify)
            debug!("failed to read symlink {}: {err}", dest.display());
            return false;
        }
    };
    let resolved = crate::file::desymlink_path(&dest.parent().unwrap().join(target));
    resolved.starts_with(prefix::cellar()) || resolved.starts_with(prefix::prefix().join("opt"))
}

/// Create the opt symlink and (unless keg-only) link the keg's public dirs
/// into the prefix.
pub fn link_keg(name: &str, pkg_version: &str, keg_only: bool) -> Result<()> {
    let prefix_path = prefix::prefix();
    let keg = keg_path(name, pkg_version);

    // <prefix>/opt/<name> -> ../Cellar/<name>/<version> (always, even keg-only)
    let opt_link = prefix_path.join("opt").join(name);
    crate::file::create_dir_all(opt_link.parent().unwrap())?;
    if opt_link.symlink_metadata().is_ok() {
        crate::file::remove_file(&opt_link)?;
    }
    crate::file::make_symlink(&relative_target(&keg, &opt_link), &opt_link)?;

    if keg_only {
        debug!(
            "{name} is keg-only, not linking into {}",
            prefix_path.display()
        );
        return Ok(());
    }

    let mut conflicts: Vec<PathBuf> = vec![];
    let mut links: Vec<(PathBuf, PathBuf)> = vec![]; // (dest in prefix, target in keg)
    for dir in LINK_DIRS {
        let src_root = keg.join(dir);
        if !src_root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&src_root).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_dir() {
                continue;
            }
            let rel = entry.path().strip_prefix(&keg)?;
            let dest = prefix_path.join(rel);
            if !can_overwrite(&dest) {
                conflicts.push(dest);
            } else {
                links.push((dest, entry.path().to_path_buf()));
            }
        }
    }
    if !conflicts.is_empty() {
        bail!(
            "cannot link {name}: these files already exist and were not created by mise or brew:\n{}\n\
             The keg is installed and usable at {}",
            conflicts
                .iter()
                .map(|p| format!("  {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n"),
            keg.display(),
        );
    }
    for (dest, target) in links {
        crate::file::create_dir_all(dest.parent().unwrap())?;
        if dest.symlink_metadata().is_ok() {
            crate::file::remove_file(&dest)?;
        }
        crate::file::make_symlink(&relative_target(&target, &dest), &dest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_target() {
        assert_eq!(
            relative_target(
                Path::new("/opt/homebrew/Cellar/jq/1.7/bin/jq"),
                Path::new("/opt/homebrew/bin/jq"),
            ),
            PathBuf::from("../Cellar/jq/1.7/bin/jq")
        );
        assert_eq!(
            relative_target(
                Path::new("/opt/homebrew/Cellar/jq/1.7"),
                Path::new("/opt/homebrew/opt/jq"),
            ),
            PathBuf::from("../Cellar/jq/1.7")
        );
    }
}
