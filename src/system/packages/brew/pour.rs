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

/// Does this symlink point into our Cellar or opt? Resolved exactly one hop
/// and lexically, like brew's own ownership checks (keg.rb: "we only want to
/// resolve one symlink") — whether a link is ours is a property of the link
/// itself, and `canonicalize` would fail on dangling links and resolve
/// relative targets against the CWD.
fn points_into_cellar(link: &Path) -> bool {
    let Ok(target) = std::fs::read_link(link) else {
        return false;
    };
    let resolved = lexical_normalize(&link.parent().unwrap().join(target));
    resolved.starts_with(prefix::cellar()) || resolved.starts_with(prefix::prefix().join("opt"))
}

/// Normalize `.` and `..` components without touching the filesystem.
fn lexical_normalize(path: &Path) -> PathBuf {
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
    use std::sync::Mutex;

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
