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

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use eyre::{Context, Result};

use super::parse_nightly_manifest;
use crate::file;

const CHANNEL_MANIFEST: &str = "lib/rustlib/multirust-channel-manifest.toml";
const COMPONENTS: &str = "lib/rustlib/components";
/// Records the modification time of rustup's `components` file when mise wrote
/// the copy. rustup rewrites that file on every component or target change, so
/// a matching time means everything in the copy came from mise. rustup ignores
/// this file.
const SEEDED_MARKER: &str = ".mise-seeded";

/// Makes `toolchains/<alias>` a copy of `toolchains/<dated>` when it is
/// missing, or when it is an older nightly (or the same nightly lacking some of
/// the dated toolchain's components and targets) and replacing it would not
/// drop anything added to it with rustup. Returns whether the alias was written.
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
    let observed = AliasState::read(&alias_path);
    match fs::symlink_metadata(&alias_path) {
        // Components or targets added with `rustup component add` or
        // `rustup target add` would be lost, so rustup keeps owning updates.
        Ok(meta) if meta.is_dir() && !keeps_rustup_additions(&alias_path, &source) => {
            debug!(
                "leaving rustup {alias} alone: it has components or targets mise did not install"
            );
            return Ok(false);
        }
        Ok(meta) if meta.is_dir() => match toolchain_nightly(&alias_path) {
            // Both are validated `nightly-YYYY-MM-DD` names, so string order is
            // date order.
            Some(existing) if existing < dated_version => {}
            // Reinstalling the same nightly with more components or targets
            // adds them to the dated toolchain only.
            Some(existing)
                if existing == dated_version && !has_components_of(&alias_path, &source) => {}
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
    if let Some(mtime) = components_mtime(&staged) {
        fs::write(staged.join(SEEDED_MARKER), mtime.to_string())?;
    }
    if alias_path.exists() {
        if !replace_if_unchanged(staging, alias, &alias_path, &observed)? {
            debug!("leaving rustup {alias} alone: rustup changed it while mise was copying");
            return Ok(false);
        }
    } else {
        fs::rename(&staged, &alias_path)
            .wrap_err_with(|| format!("failed to move {}", file::display_path(&alias_path)))?;
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

/// rustup's record of what a toolchain holds, compared to notice rustup
/// changing it while mise copies the dated toolchain. rustup has no lock mise
/// could take instead.
#[derive(Debug, PartialEq, Eq)]
struct AliasState {
    components: Option<Vec<u8>>,
    manifest: Option<Vec<u8>>,
}

impl AliasState {
    fn read(toolchain: &Path) -> Self {
        Self {
            components: fs::read(toolchain.join(COMPONENTS)).ok(),
            manifest: fs::read(toolchain.join(CHANNEL_MANIFEST)).ok(),
        }
    }
}

/// Replaces `dest` with the copy staged at `staging/<alias>`, but only if
/// `dest` still matches `observed`, the state the refresh decision was based
/// on. Returns false, leaving `dest` as it is, when rustup changed it in the
/// meantime. Whenever rustup's old toolchain cannot be put back, the staging
/// directory holding it is kept rather than deleted.
fn replace_if_unchanged(
    staging: tempfile::TempDir,
    alias: &str,
    dest: &Path,
    observed: &AliasState,
) -> Result<bool> {
    let staged = staging.path().join(alias);
    let previous = staging.path().join("previous");
    if AliasState::read(dest) != *observed {
        return Ok(false);
    }
    let keep = |staging: tempfile::TempDir, err: eyre::Report, name: &str| {
        let kept = staging.keep();
        err.wrap_err(format!(
            "rustup's previous {alias} toolchain was left at {}",
            file::display_path(kept.join(name))
        ))
    };
    if let Err(err) = replace_dir(&staged, dest, &previous) {
        if previous.exists() && !dest.exists() {
            return Err(keep(staging, err, "previous"));
        }
        return Err(err);
    }
    match put_back_if_changed(&staged, &previous, dest, observed) {
        Ok(put_back) => Ok(!put_back),
        Err(err) => {
            let name = if previous.exists() { "previous" } else { alias };
            Err(keep(staging, err.into(), name))
        }
    }
}

/// Puts rustup's replaced toolchain back at `dest` if rustup finished a change
/// to it after `observed` was read. It is at `staged` after an atomic swap and
/// at `previous` after the rename fallback. Returns whether it was put back.
fn put_back_if_changed(
    staged: &Path,
    previous: &Path,
    dest: &Path,
    observed: &AliasState,
) -> io::Result<bool> {
    if staged.exists() {
        if AliasState::read(staged) == *observed {
            return Ok(false);
        }
        exchange(staged, dest)?;
    } else {
        if AliasState::read(previous) == *observed {
            return Ok(false);
        }
        fs::rename(dest, staged)?;
        if let Err(err) = fs::rename(previous, dest) {
            // Leave rustup a working nightly, the new copy, rather than none.
            let _ = fs::rename(staged, dest);
            return Err(err);
        }
    }
    Ok(true)
}

/// Moves `staged` to `dest`. The old `dest` ends up at `staged` or `previous`,
/// both inside the staging directory, so it is deleted with it. Where the OS
/// can swap two paths atomically, `dest` never goes missing, so a concurrent
/// `cargo +nightly` or an interrupted mise always sees a toolchain.
fn replace_dir(staged: &Path, dest: &Path, previous: &Path) -> Result<()> {
    match exchange(staged, dest) {
        // The replacement is complete; nothing is left that could fail.
        Ok(()) => return Ok(()),
        Err(err) => debug!("atomic swap unavailable, renaming instead: {err}"),
    }
    fs::rename(dest, previous)?;
    if let Err(err) = fs::rename(staged, dest) {
        // Put rustup's old toolchain back rather than deleting it with the
        // staging directory.
        let _ = fs::rename(previous, dest);
        return Err(err).wrap_err_with(|| format!("failed to move {}", file::display_path(dest)));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn exchange(a: &Path, b: &Path) -> io::Result<()> {
    use nix::libc;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let a = CString::new(a.as_os_str().as_bytes())?;
    let b = CString::new(b.as_os_str().as_bytes())?;
    // The raw syscall works on musl builds, which lack a renameat2 wrapper.
    // SAFETY: both paths are NUL-terminated and outlive the call.
    let res = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if res == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn exchange(a: &Path, b: &Path) -> io::Result<()> {
    use nix::libc;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let a = CString::new(a.as_os_str().as_bytes())?;
    let b = CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: both paths are NUL-terminated and outlive the call.
    let res = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
    if res == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn exchange(_a: &Path, _b: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

/// The components and targets `toolchain` has. rustup lists targets as
/// `rust-std-<target>` components.
fn read_components(toolchain: &Path) -> BTreeSet<String> {
    fs::read_to_string(toolchain.join(COMPONENTS))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// Whether `toolchain` has every component and target listed for `source`.
fn has_components_of(toolchain: &Path, source: &Path) -> bool {
    read_components(source).is_subset(&read_components(toolchain))
}

/// Whether replacing `alias` with a copy of `source` keeps every component
/// added to it through rustup. A copy rustup has not touched since mise wrote
/// it holds only what mise put there, which may be dropped, for example after
/// a profile change. Otherwise, including for a nightly rustup installed
/// itself, every component it has must be in `source`.
fn keeps_rustup_additions(alias: &Path, source: &Path) -> bool {
    let seeded = fs::read_to_string(alias.join(SEEDED_MARKER))
        .ok()
        .and_then(|mtime| mtime.trim().parse::<u128>().ok());
    if seeded.is_some() && seeded == components_mtime(alias) {
        return true;
    }
    has_components_of(source, alias)
}

fn components_mtime(toolchain: &Path) -> Option<u128> {
    let modified = fs::metadata(toolchain.join(COMPONENTS))
        .ok()?
        .modified()
        .ok()?;
    Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos(),
    )
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
            copy_symlink(entry.path(), &target)?;
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

#[cfg(unix)]
fn copy_symlink(link: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(link)?, target)
}

#[cfg(windows)]
fn copy_symlink(link: &Path, target: &Path) -> io::Result<()> {
    let destination = fs::read_link(link)?;
    // `link.is_dir()` follows the link, so it reports what it points at.
    if link.is_dir() {
        std::os::windows::fs::symlink_dir(destination, target)
    } else {
        std::os::windows::fs::symlink_file(destination, target)
    }
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

    /// Rewrites a toolchain's components file the way `rustup component add`
    /// and `remove` do, with a modification time distinct from any before.
    fn rustup_writes_components(toolchain: &Path, components: &str) {
        let path = toolchain.join(COMPONENTS);
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, components).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(before + std::time::Duration::from_secs(1))
            .unwrap();
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
    fn refresh_replaces_a_same_date_alias_missing_components() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");
        write_toolchain(&toolchains, &alias(), "2026-09-26");
        assert!(!refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());

        fs::write(
            toolchains.join(dated("2026-09-26")).join(COMPONENTS),
            "rustc\nrust-src\n",
        )
        .unwrap();
        assert!(refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());

        assert_eq!(
            fs::read_to_string(toolchains.join(alias()).join(COMPONENTS)).unwrap(),
            "rustc\nrust-src\n"
        );
    }

    #[test]
    fn replace_dir_keeps_the_previous_directory() {
        let dir = tempfile::tempdir().unwrap();
        let (staged, dest, previous) = (
            dir.path().join("staged"),
            dir.path().join("dest"),
            dir.path().join("previous"),
        );
        fs::create_dir(&staged).unwrap();
        fs::write(staged.join("new"), "").unwrap();
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("old"), "").unwrap();

        replace_dir(&staged, &dest, &previous).unwrap();

        assert!(dest.join("new").exists());
        assert!(!dest.join("old").exists());
        // An atomic swap leaves the old directory at `staged`, the rename
        // fallback at `previous`.
        assert!(staged.join("old").exists() || previous.join("old").exists());
    }

    #[test]
    fn replace_if_unchanged_leaves_an_alias_rustup_changed() {
        let dir = tempfile::tempdir().unwrap();
        write_toolchain(dir.path(), &dated("2026-09-26"), "2026-09-26");
        write_toolchain(dir.path(), &alias(), "2026-07-04");
        let dest = dir.path().join(alias());
        let observed = AliasState::read(&dest);
        // `rustup component add rust-src --toolchain nightly` finishes after
        // the refresh decision.
        fs::write(dest.join(COMPONENTS), "rustc\nrust-src\n").unwrap();

        let staging = tempfile::tempdir_in(dir.path()).unwrap();
        clone_toolchain(
            &dir.path().join(dated("2026-09-26")),
            &staging.path().join(alias()),
            true,
        )
        .unwrap();
        let replaced = replace_if_unchanged(staging, &alias(), &dest, &observed).unwrap();

        assert!(!replaced);
        assert_eq!(
            fs::read_to_string(dest.join(COMPONENTS)).unwrap(),
            "rustc\nrust-src\n"
        );
    }

    #[test]
    fn put_back_if_changed_restores_after_the_rename_fallback() {
        let dir = tempfile::tempdir().unwrap();
        write_toolchain(dir.path(), "previous", "2026-07-04");
        let observed = AliasState::read(&dir.path().join("previous"));
        write_toolchain(dir.path(), "dest", "2026-09-26");
        // rustup finished `component add` just before the old toolchain was
        // moved aside.
        fs::write(
            dir.path().join("previous").join(COMPONENTS),
            "rustc\nrust-src\n",
        )
        .unwrap();
        let (staged, previous, dest) = (
            dir.path().join("staged"),
            dir.path().join("previous"),
            dir.path().join("dest"),
        );

        assert!(put_back_if_changed(&staged, &previous, &dest, &observed).unwrap());

        assert_eq!(
            fs::read_to_string(dest.join(COMPONENTS)).unwrap(),
            "rustc\nrust-src\n"
        );
        assert!(!previous.exists());
    }

    #[test]
    fn put_back_if_changed_keeps_an_unchanged_replacement() {
        let dir = tempfile::tempdir().unwrap();
        write_toolchain(dir.path(), "previous", "2026-07-04");
        write_toolchain(dir.path(), "dest", "2026-09-26");
        let observed = AliasState::read(&dir.path().join("previous"));
        let dest = dir.path().join("dest");

        assert!(
            !put_back_if_changed(
                &dir.path().join("staged"),
                &dir.path().join("previous"),
                &dest,
                &observed
            )
            .unwrap()
        );
        assert_eq!(
            toolchain_nightly(&dest).as_deref(),
            Some("nightly-2026-09-26")
        );
    }

    #[test]
    fn refresh_keeps_an_older_alias_with_components_added_through_rustup() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");
        write_toolchain(&toolchains, &alias(), "2026-07-04");
        fs::write(
            toolchains.join(alias()).join(COMPONENTS),
            "rustc\nrust-std-wasm32-unknown-unknown\n",
        )
        .unwrap();

        assert!(!refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());

        assert_eq!(
            toolchain_nightly(&toolchains.join(alias())).as_deref(),
            Some("nightly-2026-07-04")
        );
    }

    #[test]
    fn refresh_drops_components_mise_seeded_under_an_earlier_profile() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-25"), "2026-09-25");
        fs::write(
            toolchains.join(dated("2026-09-25")).join(COMPONENTS),
            "rustc\nrust-docs\n",
        )
        .unwrap();
        assert!(refresh(&toolchains, &dated("2026-09-25"), &alias()).unwrap());

        // A later minimal-profile nightly lacks rust-docs.
        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");
        assert!(refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());
        assert_eq!(
            toolchain_nightly(&toolchains.join(alias())).as_deref(),
            Some("nightly-2026-09-26")
        );
    }

    #[test]
    fn refresh_keeps_components_added_to_a_seeded_alias() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-25"), "2026-09-25");
        assert!(refresh(&toolchains, &dated("2026-09-25"), &alias()).unwrap());
        // `rustup component add clippy --toolchain nightly`
        rustup_writes_components(&toolchains.join(alias()), "rustc\nclippy-preview\n");

        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");
        assert!(!refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());
        assert_eq!(
            toolchain_nightly(&toolchains.join(alias())).as_deref(),
            Some("nightly-2026-09-25")
        );
    }

    #[test]
    fn refresh_keeps_a_component_removed_and_re_added_through_rustup() {
        let dir = tempfile::tempdir().unwrap();
        let toolchains = dir.path().join("toolchains");
        write_toolchain(&toolchains, &dated("2026-09-25"), "2026-09-25");
        fs::write(
            toolchains.join(dated("2026-09-25")).join(COMPONENTS),
            "rustc\nrust-docs\n",
        )
        .unwrap();
        assert!(refresh(&toolchains, &dated("2026-09-25"), &alias()).unwrap());
        // `rustup component remove rust-docs`, then `rustup component add
        // rust-docs`: the same components mise seeded, now chosen by the user.
        rustup_writes_components(&toolchains.join(alias()), "rustc\n");
        rustup_writes_components(&toolchains.join(alias()), "rustc\nrust-docs\n");

        // A later minimal-profile nightly lacks rust-docs.
        write_toolchain(&toolchains, &dated("2026-09-26"), "2026-09-26");
        assert!(!refresh(&toolchains, &dated("2026-09-26"), &alias()).unwrap());
        assert_eq!(
            toolchain_nightly(&toolchains.join(alias())).as_deref(),
            Some("nightly-2026-09-25")
        );
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
