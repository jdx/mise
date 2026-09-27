//! Keeps rustup's rolling `nightly` toolchain usable when mise installs the
//! rolling channel as a dated toolchain.
//!
//! mise resolves `rust = "nightly"` to a dated toolchain such as
//! `nightly-2026-09-26` so installs are reproducible, but `cargo +nightly`
//! asks rustup for a toolchain literally named `nightly-<host>`. rustup refuses
//! to link a custom toolchain under a channel name, so mise gives rustup its own
//! copy of the dated toolchain instead. Files are reflinked when the filesystem
//! supports copy-on-write clones and hardlinked otherwise, so the copy costs
//! almost no disk space.
//!
//! The copy belongs to rustup once it exists: `rustup update nightly` and
//! `rustup toolchain uninstall nightly` must never reach the dated toolchain
//! mise installed. rustup replaces component files by deleting and recreating
//! them, which leaves hardlinked originals alone, but it rewrites its metadata
//! directly under `lib/rustlib/` in place, so those files are always copied.

use std::fs;
use std::io;
use std::path::Path;

use eyre::{Context, Result};

use super::parse_nightly_manifest;
use crate::file;

const CHANNEL_MANIFEST: &str = "lib/rustlib/multirust-channel-manifest.toml";

/// Makes `toolchains/<alias>` a copy of `toolchains/<dated>` unless rustup
/// already has that toolchain at the same or a newer nightly. Returns whether
/// the alias was written.
pub(super) fn refresh(toolchains: &Path, dated: &str, alias: &str) -> Result<bool> {
    let source = toolchains.join(dated);
    let alias_path = toolchains.join(alias);
    let Some(dated_version) = toolchain_nightly(&source) else {
        debug!(
            "not linking rustup {alias}: {} has no readable channel manifest",
            file::display_path(&source)
        );
        return Ok(false);
    };
    match fs::symlink_metadata(&alias_path) {
        Ok(meta) if meta.is_dir() => match toolchain_nightly(&alias_path) {
            // Both are validated `nightly-YYYY-MM-DD` names, so string order is
            // date order.
            Some(existing) if existing < dated_version => {}
            Some(existing) => {
                debug!("rustup {alias} is already at {existing}");
                return Ok(false);
            }
            None => {
                debug!("leaving rustup {alias} alone: it has no readable channel manifest");
                return Ok(false);
            }
        },
        // A symlink or file here was not made by rustup's installer.
        Ok(_) => {
            debug!("leaving rustup {alias} alone: it is not a toolchain directory");
            return Ok(false);
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    let staging = tempfile::Builder::new()
        .prefix(".mise-")
        .tempdir_in(toolchains)?;
    let staged = staging.path().join(alias);
    clone_toolchain(&source, &staged, true)?;
    let previous = staging.path().join("previous");
    let replacing = alias_path.exists();
    if replacing {
        fs::rename(&alias_path, &previous)?;
    }
    if let Err(err) = fs::rename(&staged, &alias_path) {
        if replacing {
            // Put rustup's old toolchain back rather than deleting it with the
            // staging directory.
            let _ = fs::rename(&previous, &alias_path);
        }
        return Err(err)
            .wrap_err_with(|| format!("failed to move {}", file::display_path(&alias_path)));
    }

    // rustup skips `rustup update` when this hash matches the channel's, which
    // would otherwise leave the copy on whatever nightly it replaced.
    if let Some(rustup_home) = toolchains.parent() {
        match fs::remove_file(rustup_home.join("update-hashes").join(alias)) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err.into()),
            _ => {}
        }
    }
    Ok(true)
}

fn toolchain_nightly(toolchain: &Path) -> Option<String> {
    let manifest = fs::read_to_string(toolchain.join(CHANNEL_MANIFEST)).ok()?;
    parse_nightly_manifest(&manifest).ok()
}

/// Copies `source` to `dest` sharing file contents with it where possible.
/// `try_reflink` is false only in tests that exercise the hardlink fallback.
fn clone_toolchain(source: &Path, dest: &Path, try_reflink: bool) -> Result<()> {
    // clonefile(2) clones a whole directory tree in one call.
    if cfg!(target_os = "macos") && try_reflink && reflink_copy::reflink(source, dest).is_ok() {
        return Ok(());
    }
    if dest.exists() {
        file::remove_all(dest)?;
    }
    let rustup_metadata_dir = Path::new("lib").join("rustlib");
    let mut reflink = try_reflink;
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = dest.join(relative);
        let file_type = entry.file_type();
        if file_type.is_dir() {
            fs::create_dir(&target)?;
        } else if file_type.is_symlink() {
            file::make_symlink(&fs::read_link(entry.path())?, &target)?;
        } else {
            if reflink {
                if reflink_copy::reflink(entry.path(), &target).is_ok() {
                    continue;
                }
                // The first failed reflink means the filesystem cannot clone,
                // so stop trying for the rest of the tree.
                reflink = false;
            }
            if relative.parent() == Some(rustup_metadata_dir.as_path()) {
                fs::copy(entry.path(), &target)?;
                continue;
            }
            fs::hard_link(entry.path(), &target).wrap_err_with(|| {
                format!(
                    "failed to hardlink {} to {}",
                    file::display_path(entry.path()),
                    file::display_path(&target)
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "aarch64-apple-darwin";

    fn write_toolchain(toolchains: &Path, name: &str, date: &str) {
        let root = toolchains.join(name);
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("lib/rustlib").join(HOST).join("lib")).unwrap();
        fs::write(root.join("bin/rustc"), format!("rustc {date}")).unwrap();
        fs::write(
            root.join("lib/rustlib").join(HOST).join("lib/libstd.rlib"),
            "std",
        )
        .unwrap();
        fs::write(
            root.join(CHANNEL_MANIFEST),
            format!("manifest-version = \"2\"\ndate = \"{date}\"\n"),
        )
        .unwrap();
        fs::write(root.join("lib/rustlib/components"), "rustc\n").unwrap();
    }

    fn dated(date: &str) -> String {
        format!("nightly-{date}-{HOST}")
    }

    fn alias() -> String {
        format!("nightly-{HOST}")
    }

    #[test]
    fn hardlink_fallback_copies_rustup_metadata() {
        let dir = tempfile::tempdir().unwrap();
        write_toolchain(dir.path(), &dated("2026-09-26"), "2026-09-26");
        let source = dir.path().join(dated("2026-09-26"));
        let dest = dir.path().join(alias());

        clone_toolchain(&source, &dest, false).unwrap();

        assert!(same_file::is_same_file(source.join("bin/rustc"), dest.join("bin/rustc")).unwrap());
        for metadata in [CHANNEL_MANIFEST, "lib/rustlib/components"] {
            assert!(!same_file::is_same_file(source.join(metadata), dest.join(metadata)).unwrap());
        }
        // Files below the per-target directories are component files, which
        // rustup replaces rather than rewrites.
        let std = Path::new("lib/rustlib").join(HOST).join("lib/libstd.rlib");
        assert!(same_file::is_same_file(source.join(&std), dest.join(&std)).unwrap());
    }

    #[test]
    fn in_place_metadata_writes_do_not_reach_the_dated_toolchain() {
        for try_reflink in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            write_toolchain(dir.path(), &dated("2026-09-26"), "2026-09-26");
            let source = dir.path().join(dated("2026-09-26"));
            let dest = dir.path().join(alias());

            clone_toolchain(&source, &dest, try_reflink).unwrap();
            fs::write(dest.join(CHANNEL_MANIFEST), "date = \"2026-09-27\"\n").unwrap();

            assert_eq!(
                toolchain_nightly(&source).as_deref(),
                Some("nightly-2026-09-26")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn clone_preserves_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        write_toolchain(dir.path(), &dated("2026-09-26"), "2026-09-26");
        let source = dir.path().join(dated("2026-09-26"));
        std::os::unix::fs::symlink("rustc", source.join("bin/rustc-link")).unwrap();
        let dest = dir.path().join(alias());

        clone_toolchain(&source, &dest, false).unwrap();

        assert_eq!(
            fs::read_link(dest.join("bin/rustc-link")).unwrap(),
            Path::new("rustc")
        );
    }

    #[test]
    fn refresh_creates_a_missing_alias() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");

        assert!(refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());

        assert_eq!(
            toolchain_nightly(&toolchains.join(alias())).as_deref(),
            Some("nightly-2026-09-26")
        );
        let leftovers: Vec<_> = fs::read_dir(&toolchains)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(".mise-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn refresh_replaces_an_older_alias_and_its_update_hash() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");
        write_toolchain(&toolchains, &alias(), "2026-07-04");
        let update_hash = dir.path().join("update-hashes").join(alias());
        fs::create_dir_all(update_hash.parent().unwrap()).unwrap();
        fs::write(&update_hash, "hash").unwrap();

        assert!(refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());

        assert_eq!(
            fs::read_to_string(toolchains.join(alias()).join("bin/rustc")).unwrap(),
            "rustc 2026-09-26"
        );
        assert!(!update_hash.exists());
    }

    #[test]
    fn refresh_keeps_a_newer_alias() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");
        write_toolchain(&toolchains, &alias(), "2026-09-27");

        assert!(!refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());

        assert_eq!(
            toolchain_nightly(&toolchains.join(alias())).as_deref(),
            Some("nightly-2026-09-27")
        );
    }

    #[test]
    fn refresh_keeps_an_alias_without_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");
        fs::create_dir_all(toolchains.join(alias()).join("bin")).unwrap();

        assert!(!refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());
        assert!(!toolchains.join(alias()).join("bin/rustc").exists());
    }
}
