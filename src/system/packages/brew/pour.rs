//! Pour a bottle: extract -> relocate -> codesign -> receipt -> link.

use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};
use serde_json::json;

use super::api::BottleFile;
use super::prefix;
use super::relocate;
use super::resolve::ResolvedFormula;
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::result::Result;
use crate::ui::progress_report::SingleReport;

/// directories linked from a keg into the prefix (brew's Keg::KEG_LINK_DIRECTORIES,
/// minus etc/var which brew handles specially and we defer)
pub(super) const LINK_DIRS: &[&str] = &["bin", "sbin", "include", "lib", "share", "Frameworks"];

pub fn keg_path(name: &str, pkg_version: &str) -> PathBuf {
    prefix::cellar().join(name).join(pkg_version)
}

/// is this keg fully poured and linked? Every pour ends by creating the
/// `opt/<name>` symlink (even for keg-only formulae), so a Cellar directory
/// without it is a remnant of a failed install and must not block a retry.
pub fn keg_installed(name: &str, pkg_version: &str) -> bool {
    keg_path(name, pkg_version).exists() && linked_version(name).as_deref() == Some(pkg_version)
}

/// the version `opt/<name>` points at, if the symlink resolves to an
/// existing keg
pub fn linked_version(name: &str) -> Option<String> {
    let opt = prefix::prefix().join("opt").join(name);
    let target = std::fs::read_link(&opt).ok()?;
    let resolved = opt.parent().unwrap().join(target);
    if !resolved.is_dir() {
        return None;
    }
    resolved
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
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
    pr: &dyn SingleReport,
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
    pr.set_message("extract".to_string());
    crate::file::untar(
        tarball,
        &scratch,
        ExtractionFormat::TarGz,
        &ExtractOptions {
            strip_components: 0,
            pr: Some(pr),
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

    // ":any_skip_relocation" bottles need no relocation — except on Linux,
    // where bottles built by Homebrew < 5.1.15 are incorrectly tagged and
    // still carry placeholder ELF linkage (brew applies the same version
    // check in extend/os/linux/bottle_specification.rb)
    let skip_relocation = bottle.cellar == ":any_skip_relocation"
        && (cfg!(target_os = "macos") || bottled_by_homebrew_at_least(&tmp, (5, 1, 15)));
    let report = if skip_relocation {
        relocate::RelocationReport::default()
    } else {
        pr.set_message("relocate".to_string());
        relocate::relocate_keg(&tmp, name)?
    };
    // arm64 macOS kills binaries whose signature doesn't match; Linux ELF
    // files have no signatures to fix
    if cfg!(target_os = "macos") && !report.changed_machos.is_empty() {
        pr.set_message("codesign".to_string());
        relocate::codesign(&report.changed_machos)
            .wrap_err_with(|| format!("failed to re-sign relocated binaries for {name}"))?;
    }

    write_receipt(rf, tag, &tmp, &report, closure, true)?;

    pr.set_message("link".to_string());
    if keg.exists() {
        crate::file::remove_all(&keg)?;
    }
    crate::file::rename(&tmp, &keg)?;
    // never leave a half-installed keg: if linking fails (conflicts, IO),
    // remove the keg so the next install retries from scratch
    if let Err(err) = link_keg(name, &pkg_version, rf.formula.keg_only) {
        if let Err(rm_err) = crate::file::remove_all(&keg) {
            // a keg left behind here is unlinked but looks installed, so
            // future installs would skip it — make that state visible
            warn!(
                "failed to remove {} after link failure: {rm_err}\n\
                 remove it manually, then re-run `mise bootstrap packages apply`",
                keg.display()
            );
        }
        return Err(err);
    }
    Ok(())
}

/// Was this bottle built by Homebrew >= `min`? Read from the receipt the
/// bottle ships with (brew calls it the tab), before we overwrite it with our
/// own. This mirrors brew's own `parsed_homebrew_version >= "5.1.15"` check —
/// brew's version format is dotted numerics, not an arbitrary tool version.
fn bottled_by_homebrew_at_least(keg: &Path, min: (u64, u64, u64)) -> bool {
    let Ok(receipt) = crate::file::read_to_string(keg.join("INSTALL_RECEIPT.json")) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&receipt) else {
        return false;
    };
    let Some(version) = json.get("homebrew_version").and_then(|v| v.as_str()) else {
        return false;
    };
    // "5.1.16-31-ga1b2c3d" -> (5, 1, 16); unparseable -> (0, 0, 0) = old
    let mut parts = version
        .split(['.', '-', ' '])
        .map(|p| p.parse::<u64>().unwrap_or(0));
    let v = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    v >= min
}

/// brew-compatible INSTALL_RECEIPT.json so a later-installed real Homebrew
/// adopts these kegs (brew list/upgrade/uninstall all work). Written for
/// both poured bottles and source-built kegs; `poured_from_bottle`
/// distinguishes them the same way brew's own tab does.
pub fn write_receipt(
    rf: &ResolvedFormula,
    tag: &str,
    keg: &Path,
    report: &relocate::RelocationReport,
    closure: &[ResolvedFormula],
    poured_from_bottle: bool,
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
        // must stay >= 5.1.15: bottled_by_homebrew_at_least gates Linux ELF
        // relocation on the receipt's homebrew_version, and a poured keg's
        // linkage is already final
        "homebrew_version": "5.1.15 (mise)",
        "used_options": [],
        "unused_options": [],
        "built_as_bottle": poured_from_bottle,
        "poured_from_bottle": poured_from_bottle,
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
            "tap": rf.formula.tap.as_deref().unwrap_or("homebrew/core"),
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
/// into the prefix. Conflicts are detected before anything is touched, and a
/// failure partway through removes the links already created — the caller
/// rolls the keg back on error, and nothing may be left dangling into it.
pub fn link_keg(name: &str, pkg_version: &str, keg_only: bool) -> Result<()> {
    let prefix_path = prefix::prefix();
    let keg = keg_path(name, pkg_version);
    // <prefix>/opt/<name> -> ../Cellar/<name>/<version> (always, even keg-only)
    let opt_link = prefix_path.join("opt").join(name);

    let mut conflicts: Vec<PathBuf> = vec![];
    // (dest in prefix, target in keg); opt first
    let mut links: Vec<(PathBuf, PathBuf)> = vec![(opt_link.clone(), keg.clone())];
    if keg_only {
        debug!(
            "{name} is keg-only, not linking into {}",
            prefix_path.display()
        );
    } else {
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
    }
    if !conflicts.is_empty() {
        // nothing has been linked yet, and the caller rolls the keg back on
        // this error — so don't claim it remains usable
        bail!(
            "cannot link {name}: these files already exist and were not created by mise or brew:\n{}\n\
             Remove or rename them, then re-run `mise bootstrap packages apply`",
            conflicts
                .iter()
                .map(|p| format!("  {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    // remember every symlink we overwrite (upgrades replace the previous
    // version's links, opt included) so a failed link restores all of them
    let mut created: Vec<PathBuf> = vec![];
    let mut replaced: Vec<(PathBuf, PathBuf)> = vec![];
    let mut failure: Option<eyre::Report> = None;
    for (dest, target) in &links {
        let made = (|| -> Result<()> {
            crate::file::create_dir_all(dest.parent().unwrap())?;
            if dest.symlink_metadata().is_ok() {
                if let Ok(prev) = std::fs::read_link(dest) {
                    replaced.push((dest.clone(), prev));
                }
                crate::file::remove_file(dest)?;
            }
            crate::file::make_symlink(&relative_target(target, dest), dest)?;
            Ok(())
        })();
        if let Err(err) = made {
            failure = Some(err);
            break;
        }
        created.push(dest.clone());
    }
    if let Some(err) = failure {
        for dest in created {
            let _ = crate::file::remove_file(&dest);
        }
        for (dest, prev) in replaced {
            let _ = crate::file::make_symlink(&prev, &dest);
        }
        return Err(err);
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
