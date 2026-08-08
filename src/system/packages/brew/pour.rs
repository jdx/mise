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
const KEG_ONLY_MARKER: &str = ".mise-keg-only";

struct RecordRepair {
    version: String,
    keg: PathBuf,
    destination: PathBuf,
}

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
    record_keg(name, &opt).map(|(version, _)| version)
}

/// Return the active keg version and whether one of its active records can be repaired locally.
pub(super) fn linked_state(name: &str) -> Option<(String, bool)> {
    let opt = prefix::prefix().join("opt").join(name);
    let active = record_keg(name, &opt).or_else(|| {
        record_needs_replacement(name, &opt)
            .then(|| record_keg(name, &prefix::linked_keg_record(name)))?
    })?;
    Some((active.0, pending_record_repair(name).is_some()))
}

/// Restore one missing or dangling mise-owned active-keg record without relinking the keg.
pub(super) fn repair_link_record(name: &str, dry_run: bool) -> Result<bool> {
    let Some(repair) = pending_record_repair(name) else {
        return Ok(false);
    };
    let record = if repair.destination == prefix::linked_keg_record(name) {
        "linked-keg record"
    } else {
        "opt record"
    };
    if dry_run {
        miseprintln!("repair {name}/{}: {record}", repair.version);
        return Ok(true);
    }
    crate::file::create_dir_all(repair.destination.parent().unwrap())?;
    crate::file::make_symlink(
        &relative_target(&repair.keg, &repair.destination),
        &repair.destination,
    )
    .wrap_err_with(|| {
        format!(
            "failed to repair Homebrew {record}: {}",
            repair.destination.display()
        )
    })?;
    Ok(true)
}

/// Find a single active record that can be reconstructed from its valid counterpart.
fn pending_record_repair(name: &str) -> Option<RecordRepair> {
    let opt = prefix::prefix().join("opt").join(name);
    let linked = prefix::linked_keg_record(name);
    if let Some((version, keg)) = record_keg(name, &opt) {
        if keg.join(KEG_ONLY_MARKER).is_file() {
            return None;
        }
        if record_needs_replacement(name, &linked) && has_public_link_into(&keg) {
            return Some(RecordRepair {
                version,
                keg,
                destination: linked,
            });
        }
        return None;
    }
    if record_needs_replacement(name, &opt)
        && let Some((version, keg)) = record_keg(name, &linked)
    {
        return Some(RecordRepair {
            version,
            keg,
            destination: opt,
        });
    }
    None
}

/// Resolve a record only when it targets an existing direct child of the formula rack.
fn record_keg(name: &str, record: &Path) -> Option<(String, PathBuf)> {
    let target = record_target(name, record)?.canonicalize().ok()?;
    let rack = prefix::cellar().join(name).canonicalize().ok()?;
    if target.parent()? != rack || !target.is_dir() {
        return None;
    }
    let version = target.file_name()?.to_string_lossy().to_string();
    Some((version.clone(), keg_path(name, &version)))
}

/// Resolve a record target within the formula rack without requiring the keg to exist.
fn record_target(name: &str, record: &Path) -> Option<PathBuf> {
    let target = resolved_symlink_target(record)?;
    let rack = prefix::cellar().join(name).canonicalize().ok()?;
    (target.parent()? == rack).then_some(target)
}

/// Return true only for an absent path or a dangling symlink owned by this formula rack.
fn record_needs_replacement(name: &str, path: &Path) -> bool {
    match path.symlink_metadata() {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Ok(metadata) if metadata.file_type().is_symlink() => {
            record_target(name, path).is_some() && record_keg(name, path).is_none()
        }
        Err(_) | Ok(_) => false,
    }
}

/// Check for the standard public-link shape created from a non-keg-only keg.
fn has_public_link_into(keg: &Path) -> bool {
    LINK_DIRS.iter().any(|dir| {
        let root = keg.join(dir);
        root.exists()
            && walkdir::WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .any(|entry| {
                    entry
                        .path()
                        .strip_prefix(keg)
                        .ok()
                        .map(|relative| prefix::prefix().join(relative))
                        .is_some_and(|link| symlink_points_to(&link, entry.path()))
                })
    })
}

/// Compare a symlink's one-hop target with a destination using resolved parent paths.
fn symlink_points_to(link: &Path, target: &Path) -> bool {
    resolved_symlink_target(link).as_ref() == Some(&resolved_path(target))
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

    // ":any_skip_relocation" skips binary linkage relocation, but Homebrew
    // still replaces placeholders in text files. On Linux, bottles built by
    // Homebrew < 5.1.15 are incorrectly tagged and still need ELF linkage
    // relocation (brew applies the same version check in
    // extend/os/linux/bottle_specification.rb).
    let skip_linkage = bottle.cellar == ":any_skip_relocation"
        && (cfg!(target_os = "macos") || bottled_by_homebrew_at_least(&tmp, (5, 1, 15)));
    pr.set_message("relocate".to_string());
    let report = relocate::relocate_keg(&tmp, name, skip_linkage)?;
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
/// or opt (i.e. something brew/mise created and can re-create), or anything
/// underneath a directory symlink brew created — brew links a directory it
/// owns entirely as a single symlink, so the regular files and the keg's own
/// symlinks inside are still brew's.
fn can_overwrite(dest: &Path) -> bool {
    let Ok(meta) = dest.symlink_metadata() else {
        return true; // doesn't exist
    };
    if brew_owned_ancestor(dest).is_some() {
        return true;
    }
    if !meta.is_symlink() {
        return false;
    }
    points_into_cellar(dest)
}

/// Does this symlink point into our Cellar or opt? Resolve the link itself once,
/// then canonicalize its parent so nested relative links retain their final
/// component while using the Cellar's filesystem spelling.
fn points_into_cellar(link: &Path) -> bool {
    let Some(target) = resolved_symlink_target(link) else {
        return false;
    };
    let cellar = prefix::cellar()
        .canonicalize()
        .unwrap_or_else(|_| prefix::cellar());
    let opt = prefix::prefix()
        .join("opt")
        .canonicalize()
        .unwrap_or_else(|_| prefix::prefix().join("opt"));
    target.starts_with(cellar) || target.starts_with(opt)
}

/// Resolve one symlink hop relative to its parent without chasing the final component.
fn resolved_symlink_target(link: &Path) -> Option<PathBuf> {
    let target = std::fs::read_link(link).ok()?;
    let target = if target.is_absolute() {
        target
    } else {
        link.parent()?.join(target)
    };
    Some(resolved_path(&target))
}

/// Canonicalize the parent of a lexically normalized path while preserving its final component.
fn resolved_path(path: &Path) -> PathBuf {
    let target = lexical_normalize(path);
    match (target.parent(), target.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf())
            .join(name),
        _ => target,
    }
}

/// Normalize `.` and `..` components without touching the filesystem.
pub(super) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The outermost ancestor of `dest` (strictly below the prefix) that is a
/// symlink pointing into the Cellar — i.e. a directory brew linked wholesale.
fn brew_owned_ancestor(dest: &Path) -> Option<PathBuf> {
    let prefix_path = prefix::prefix();
    let mut ancestors: Vec<&Path> = dest
        .ancestors()
        .skip(1)
        .take_while(|p| *p != prefix_path && p.starts_with(&prefix_path))
        .collect();
    ancestors.reverse(); // outermost first
    for anc in ancestors {
        if anc
            .symlink_metadata()
            .map(|m| m.is_symlink())
            .unwrap_or(false)
        {
            return points_into_cellar(anc).then(|| anc.to_path_buf());
        }
    }
    None
}

/// Replace brew-created directory symlinks on the way to `dest` with real
/// directories of symlinks to their old contents — the same expansion brew
/// performs when another keg needs to place files inside a wholesale-linked
/// directory (resolve_any_conflicts). The replacement is fully staged before
/// the symlink is swapped out, so a failure leaves the tree unchanged.
fn materialize_brew_dirs(dest: &Path) -> Result<()> {
    while let Some(link_dir) = brew_owned_ancestor(dest) {
        let raw_target = std::fs::read_link(&link_dir)?;
        let staging = link_dir.parent().unwrap().join(format!(
            ".mise-materialize-{}",
            link_dir.file_name().unwrap().to_string_lossy()
        ));
        let staged = (|| -> Result<()> {
            if staging.exists() {
                crate::file::remove_all(&staging)?;
            }
            crate::file::create_dir_all(&staging)?;
            // a dangling dir symlink (keg already pruned) has nothing to preserve
            let target = lexical_normalize(&link_dir.parent().unwrap().join(&raw_target));
            if target.is_dir() {
                for entry in std::fs::read_dir(&target)? {
                    let entry = entry?;
                    // targets are relative to the link's final location
                    let child_link = link_dir.join(entry.file_name());
                    crate::file::make_symlink(
                        &relative_target(&entry.path(), &child_link),
                        &staging.join(entry.file_name()),
                    )?;
                }
            }
            Ok(())
        })();
        if let Err(err) = staged {
            let _ = crate::file::remove_all(&staging);
            return Err(err);
        }
        // swap: a directory cannot be renamed over a symlink, so remove the
        // link first; if the rename then fails, put the symlink back
        if let Err(err) = crate::file::remove_file(&link_dir) {
            let _ = crate::file::remove_all(&staging);
            return Err(err);
        }
        if let Err(err) = crate::file::rename(&staging, &link_dir) {
            let _ = crate::file::make_symlink(&raw_target, &link_dir);
            let _ = crate::file::remove_all(&staging);
            return Err(err);
        }
    }
    Ok(())
}

/// Create the opt symlink and (unless keg-only) link the keg's public dirs
/// into the prefix. Conflicts are detected before anything is touched, and a
/// failure partway through removes the links already created — the caller
/// rolls the keg back on error, and nothing may be left dangling into it.
pub fn link_keg(name: &str, pkg_version: &str, keg_only: bool) -> Result<()> {
    let prefix_path = prefix::prefix();
    let keg = keg_path(name, pkg_version);
    if keg_only {
        crate::file::write(keg.join(KEG_ONLY_MARKER), "")?;
    }
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
        let linked = prefix::linked_keg_record(name);
        if can_overwrite(&linked) {
            links.push((linked, keg.clone()));
        } else {
            conflicts.push(linked);
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
            // a parent that is a brew directory symlink must become a real
            // directory first — otherwise the link below would be created
            // inside (and delete files from) the old keg it points to
            materialize_brew_dirs(dest)?;
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
    use tokio::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::const_new(());

    struct BrewPrefixGuard {
        previous: Option<String>,
    }

    impl BrewPrefixGuard {
        fn set(prefix: &Path) -> Self {
            let previous = crate::env::var("MISE_SYSTEM_BREW_PREFIX").ok();
            crate::env::set_var("MISE_SYSTEM_BREW_PREFIX", prefix);
            Self { previous }
        }
    }

    impl Drop for BrewPrefixGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => crate::env::set_var("MISE_SYSTEM_BREW_PREFIX", previous),
                None => crate::env::remove_var("MISE_SYSTEM_BREW_PREFIX"),
            }
        }
    }

    /// keg with a versioned dylib and its unversioned alias (the relative
    /// symlink chain every brew library bottle ships), plus a header dir
    fn write_lib_keg(prefix: &Path, name: &str, version: &str) -> Result<PathBuf> {
        let keg = prefix.join("Cellar").join(name).join(version);
        crate::file::create_dir_all(keg.join("lib"))?;
        crate::file::write(keg.join("lib").join(format!("lib{name}.1.dylib")), version)?;
        crate::file::make_symlink(
            Path::new(&format!("lib{name}.1.dylib")),
            &keg.join("lib").join(format!("lib{name}.dylib")),
        )?;
        crate::file::create_dir_all(keg.join("include").join(name))?;
        crate::file::write(keg.join("include").join(name).join("header.h"), version)?;
        // keg-internal relative symlink inside the dir brew links wholesale
        crate::file::make_symlink(
            Path::new("header.h"),
            &keg.join("include").join(name).join("alias.h"),
        )?;
        Ok(keg)
    }

    /// link a keg the way real brew does: file symlinks for files whose
    /// parent dir is shared, one directory symlink for a dir the keg owns
    fn brew_style_link(prefix: &Path, name: &str, version: &str) -> Result<()> {
        let cellar_rel = Path::new("../Cellar").join(name).join(version);
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(
            &Path::new("../Cellar").join(name).join(version),
            &prefix.join("opt").join(name),
        )?;
        crate::file::create_dir_all(prefix.join("lib"))?;
        for lib in [format!("lib{name}.dylib"), format!("lib{name}.1.dylib")] {
            crate::file::make_symlink(
                &cellar_rel.join("lib").join(&lib),
                &prefix.join("lib").join(&lib),
            )?;
        }
        crate::file::create_dir_all(prefix.join("include"))?;
        crate::file::make_symlink(
            &cellar_rel.join("include").join(name),
            &prefix.join("include").join(name),
        )?;
        Ok(())
    }

    fn canonical_tempdir() -> Result<(tempfile::TempDir, PathBuf)> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().canonicalize()?;
        Ok((tmp, path))
    }

    /// the unversioned dylib alias resolves through a relative symlink chain
    /// inside the Cellar and must still be recognized as brew's
    #[test]
    fn test_upgrade_over_brew_file_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        write_lib_keg(&prefix, "foo", "2.0")?;

        link_keg("foo", "2.0", false)?;

        let lib_link = prefix.join("lib").join("libfoo.dylib");
        assert!(lib_link.symlink_metadata()?.is_symlink());
        assert_eq!(std::fs::read_to_string(&lib_link)?, "2.0");
        Ok(())
    }

    /// everything under a brew directory-level symlink is brew's and must
    /// relink without conflicts or modifying the old keg
    #[test]
    fn test_upgrade_over_brew_dir_symlink() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let old_keg = write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        write_lib_keg(&prefix, "foo", "2.0")?;

        link_keg("foo", "2.0", false)?;

        let header = prefix.join("include").join("foo").join("header.h");
        assert_eq!(std::fs::read_to_string(&header)?, "2.0");
        // a keg-internal relative symlink under the dir symlink is brew's too
        assert_eq!(
            std::fs::read_to_string(prefix.join("include").join("foo").join("alias.h"))?,
            "2.0"
        );
        // the old keg's own files survive untouched
        assert_eq!(
            std::fs::read_to_string(old_keg.join("include").join("foo").join("header.h"))?,
            "1.0"
        );
        Ok(())
    }

    /// a link into the Cellar whose target continues outside it (bottles
    /// ship symlinks to system libraries) is still brew's own link
    #[test]
    fn test_upgrade_over_link_whose_cellar_target_leaves_the_cellar() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        for version in ["1.0", "2.0"] {
            let keg = prefix.join("Cellar").join("foo").join(version);
            crate::file::create_dir_all(keg.join("lib"))?;
            crate::file::make_symlink(
                Path::new("/usr/lib/libSystem.B.dylib"),
                &keg.join("lib").join("libsys.dylib"),
            )?;
        }
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(
            Path::new("../Cellar/foo/1.0"),
            &prefix.join("opt").join("foo"),
        )?;
        crate::file::create_dir_all(prefix.join("lib"))?;
        crate::file::make_symlink(
            Path::new("../Cellar/foo/1.0/lib/libsys.dylib"),
            &prefix.join("lib").join("libsys.dylib"),
        )?;

        link_keg("foo", "2.0", false)?;

        assert_eq!(
            std::fs::read_link(prefix.join("lib").join("libsys.dylib"))?,
            PathBuf::from("../Cellar/foo/2.0/lib/libsys.dylib")
        );
        Ok(())
    }

    /// a regular file that is NOT under a brew directory symlink is foreign
    /// and must still be reported as a conflict
    #[test]
    fn test_foreign_regular_file_still_conflicts() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "2.0")?;
        crate::file::create_dir_all(prefix.join("include").join("foo"))?;
        crate::file::write(prefix.join("include").join("foo").join("header.h"), "mine")?;

        let err = link_keg("foo", "2.0", false).unwrap_err();
        assert!(err.to_string().contains("not created by mise or brew"));
        assert_eq!(
            std::fs::read_to_string(prefix.join("include").join("foo").join("header.h"))?,
            "mine"
        );
        Ok(())
    }

    /// a shared dir linked wholesale to another keg is expanded into a real
    /// directory keeping that keg's entries visible, like brew does
    #[test]
    fn test_materialize_shared_dir_owned_by_other_keg() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        // other keg owns share/xml via a dir symlink
        let other = prefix.join("Cellar").join("other").join("1.0");
        crate::file::create_dir_all(other.join("share").join("xml"))?;
        crate::file::write(other.join("share").join("xml").join("other.dtd"), "other")?;
        crate::file::create_dir_all(prefix.join("share"))?;
        crate::file::make_symlink(
            Path::new("../Cellar/other/1.0/share/xml"),
            &prefix.join("share").join("xml"),
        )?;
        // new keg wants a file inside share/xml
        let keg = prefix.join("Cellar").join("foo").join("2.0");
        crate::file::create_dir_all(keg.join("share").join("xml"))?;
        crate::file::write(keg.join("share").join("xml").join("foo.dtd"), "foo")?;

        link_keg("foo", "2.0", false)?;

        let xml = prefix.join("share").join("xml");
        assert!(!xml.symlink_metadata()?.is_symlink());
        assert_eq!(std::fs::read_to_string(xml.join("other.dtd"))?, "other");
        assert_eq!(std::fs::read_to_string(xml.join("foo.dtd"))?, "foo");
        // the other keg must not have been polluted
        assert!(!other.join("share").join("xml").join("foo.dtd").exists());
        Ok(())
    }

    #[test]
    fn test_nested_relative_link_is_brew_owned() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = prefix.join("Cellar/foo/1.0/lib");
        crate::file::create_dir_all(&keg)?;
        crate::file::write(keg.join("libfoo.1.dylib"), "")?;
        let lib = prefix.join("lib");
        crate::file::create_dir_all(&lib)?;
        crate::file::make_symlink(Path::new("../Cellar/foo/1.0/lib"), &lib.join("foo"))?;
        let nested = lib.join("libfoo.dylib");
        crate::file::make_symlink(Path::new("foo/libfoo.1.dylib"), &nested)?;

        assert!(can_overwrite(&nested));
        Ok(())
    }

    #[test]
    fn test_link_keg_maintains_homebrew_linked_record() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        link_keg("foo", "1.0", false)?;
        let linked = prefix::linked_keg_record("foo");
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );

        write_lib_keg(&prefix, "foo", "2.0")?;
        link_keg("foo", "2.0", false)?;
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("../../../Cellar/foo/2.0")
        );

        let bar_keg = write_lib_keg(&prefix, "bar", "1.0")?;
        link_keg("bar", "1.0", true)?;
        assert!(prefix.join("opt/bar").is_symlink());
        assert!(prefix::linked_keg_record("bar").symlink_metadata().is_err());
        assert_eq!(linked_state("bar"), Some(("1.0".to_string(), false)));
        crate::file::make_symlink(
            &bar_keg.join("lib/libbar.1.dylib"),
            &prefix.join("lib/libbar.1.dylib"),
        )?;
        assert!(!repair_link_record("bar", false)?);
        assert!(prefix::linked_keg_record("bar").symlink_metadata().is_err());

        let linked = prefix::linked_keg_record("bar");
        crate::file::create_dir_all(linked.parent().unwrap())?;
        crate::file::make_symlink(Path::new("../../../Cellar/bar/1.0"), &linked)?;
        std::fs::remove_file(prefix.join("opt/bar"))?;
        assert_eq!(linked_state("bar"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("bar", false)?);
        assert_eq!(
            std::fs::read_link(prefix.join("opt/bar"))?,
            PathBuf::from("../Cellar/bar/1.0")
        );
        Ok(())
    }

    #[test]
    fn test_repairs_active_records_without_relinking_the_keg() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        let public = prefix.join("lib/libfoo.dylib");
        let public_target = std::fs::read_link(&public)?;

        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("foo", false)?);
        assert_eq!(std::fs::read_link(&public)?, public_target);
        assert_eq!(
            std::fs::read_link(prefix::linked_keg_record("foo"))?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );

        crate::file::remove_file(prefix.join("opt/foo"))?;
        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("foo", false)?);
        assert_eq!(
            std::fs::read_link(prefix.join("opt/foo"))?,
            PathBuf::from("../Cellar/foo/1.0")
        );
        assert_eq!(std::fs::read_link(&public)?, public_target);
        Ok(())
    }

    #[test]
    fn test_repairs_dangling_owned_records_but_not_foreign_records() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        let linked = prefix::linked_keg_record("foo");
        crate::file::create_dir_all(linked.parent().unwrap())?;
        crate::file::make_symlink(Path::new("../../../Cellar/foo/0.9"), &linked)?;

        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("foo", false)?);
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );

        let opt = prefix.join("opt/foo");
        crate::file::make_symlink(Path::new("../Cellar/foo/0.9"), &opt)?;
        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("foo", false)?);
        assert_eq!(
            std::fs::read_link(&opt)?,
            PathBuf::from("../Cellar/foo/1.0")
        );

        crate::file::make_symlink(Path::new("/custom/missing-foo"), &linked)?;
        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), false)));
        assert!(!repair_link_record("foo", false)?);
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("/custom/missing-foo")
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_manager_repairs_linked_record_without_repouring() -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        use crate::system::packages::{
            InstallOpts, PackageRequest, PackageState, SystemPackageManager,
        };

        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_lib_keg(&prefix, "foo", "1.0")?;
        crate::file::write(keg.join("INSTALL_RECEIPT.json"), "{}")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        let public = prefix.join("lib/libfoo.dylib");
        let request = PackageRequest {
            name: "foo".to_string(),
            version: None,
            tap_url: None,
            os: None,
        };
        let keg_inode = keg.metadata()?.ino();
        let receipt_modified = keg.join("INSTALL_RECEIPT.json").metadata()?.modified()?;
        let public_inode = public.symlink_metadata()?.ino();

        let manager = super::super::BrewManager::new();
        let mismatched = PackageRequest {
            version: Some("2.0".to_string()),
            ..request.clone()
        };
        let err = manager
            .install(std::slice::from_ref(&mismatched), &InstallOpts::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("pin via the formula name"));
        assert!(prefix::linked_keg_record("foo").symlink_metadata().is_err());

        let status = manager.installed(std::slice::from_ref(&request)).await?;
        assert_eq!(
            status[0].state,
            PackageState::NeedsRepair {
                installed: "1.0".to_string()
            }
        );

        manager
            .install(std::slice::from_ref(&request), &InstallOpts::default())
            .await?;

        assert_eq!(keg.metadata()?.ino(), keg_inode);
        assert_eq!(
            keg.join("INSTALL_RECEIPT.json").metadata()?.modified()?,
            receipt_modified
        );
        assert_eq!(public.symlink_metadata()?.ino(), public_inode);
        assert_eq!(
            std::fs::read_link(prefix::linked_keg_record("foo"))?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );
        assert_eq!(
            manager.installed(std::slice::from_ref(&request)).await?[0].state,
            PackageState::Installed {
                version: "1.0".to_string()
            }
        );
        Ok(())
    }

    #[test]
    fn test_does_not_infer_a_linked_record_without_public_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(Path::new("../Cellar/foo/1.0"), &prefix.join("opt/foo"))?;

        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), false)));
        assert!(!repair_link_record("foo", false)?);
        assert!(prefix::linked_keg_record("foo").symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn test_runtime_loader_does_not_make_glibc_look_linked() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = prefix.join("Cellar/glibc/1.0");
        crate::file::create_dir_all(keg.join("lib"))?;
        crate::file::write(keg.join("lib/ld-linux-x86-64.so.2"), "")?;
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(Path::new("../Cellar/glibc/1.0"), &prefix.join("opt/glibc"))?;
        crate::file::create_dir_all(prefix.join("lib"))?;
        crate::file::make_symlink(
            &keg.join("lib/ld-linux-x86-64.so.2"),
            &prefix.join("lib/ld.so"),
        )?;

        assert_eq!(linked_state("glibc"), Some(("1.0".to_string(), false)));
        assert!(!repair_link_record("glibc", false)?);
        assert!(
            prefix::linked_keg_record("glibc")
                .symlink_metadata()
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_foreign_linked_record_blocks_linking_before_changes() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        let linked = prefix::linked_keg_record("foo");
        crate::file::create_dir_all(linked.parent().unwrap())?;
        crate::file::write(&linked, "foreign")?;

        let err = link_keg("foo", "1.0", false).unwrap_err();

        assert!(err.to_string().contains("not created by mise or brew"));
        assert_eq!(crate::file::read_to_string(&linked)?, "foreign");
        assert!(prefix.join("opt/foo").symlink_metadata().is_err());
        Ok(())
    }

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
