use crate::path::{Path, PathBuf, PathExt};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::os::unix::prelude::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bzip2::read::BzDecoder;
use eyre::bail;
use eyre::{Context, Result};
use filetime::{FileTime, set_file_times};
use flate2::read::GzDecoder;
use itertools::Itertools;
use jdx_tar::{Archive, EntryType, UnpackOptions};
use path_absolutize::Absolutize;
use sha2::{Digest, Sha256};
use std::sync::LazyLock as Lazy;
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::progress::SingleReport;
use crate::{dirs, env};
#[cfg(windows)]
use mise_settings::Settings;

pub fn open<P: AsRef<Path>>(path: P) -> Result<File> {
    let path = path.as_ref();
    trace!("open {}", display_path(path));
    File::open(path).wrap_err_with(|| format!("failed open: {}", display_path(path)))
}

pub fn read<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    let path = path.as_ref();
    trace!("cat {}", display_path(path));
    fs::read(path).wrap_err_with(|| format!("failed read: {}", display_path(path)))
}

pub fn size<P: AsRef<Path>>(path: P) -> Result<u64> {
    let path = path.as_ref();
    trace!("du -b {}", display_path(path));
    path.metadata()
        .map(|m| m.len())
        .wrap_err_with(|| format!("failed size: {}", display_path(path)))
}

pub fn append<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    trace!("append {}", display_path(path));
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut f| f.write_all(contents.as_ref()))
        .wrap_err_with(|| format!("failed append: {}", display_path(path)))
}

/// Windows' `MAX_PATH`. Not a limit mise imposes — the point below is that some paths hit it long
/// before others do. It counts UTF-16 code units and includes the terminating NUL, so a path of
/// exactly `MAX_PATH` is already one too many.
///
/// Shared with mise's `cli::self_update`, which needs the same number for the opposite reason:
/// the file APIs can be escaped past this with a `\\?\` prefix, which is why `std::fs` copes with
/// long paths, but `CreateProcess` has no such escape hatch and simply will not start an
/// executable whose path reaches it.
#[cfg(windows)]
pub const MAX_PATH: usize = 260;

/// Why a Windows file operation on `path` probably failed, when the error says something a reader
/// cannot act on.
///
/// `None` on unix, always: deleting a running binary succeeds there, and `MAX_PATH` does not
/// exist. Splitting the remedy by platform rather than the wording follows
/// [`make_executable_hint`], where `chmod +x` was not merely unavailable on Windows but the wrong
/// instruction.
///
/// The error codes are the ones already spoken elsewhere in this file — see [`do_rename`] and
/// `should_retry_atomic_persist`, which retry on the same pair. This answers a different question
/// about them: not whether to try again, but what to tell the user when trying again will not help.
#[cfg(windows)]
fn windows_io_hint(path: &Path, err: &std::io::Error) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;

    // ERROR_ACCESS_DENIED (5) / ERROR_SHARING_VIOLATION (32). Unlike the transient locks that
    // `do_rename` retries through, a file held open by a running process stays held, so this is a
    // message rather than a retry.
    let in_use = err.kind() == std::io::ErrorKind::PermissionDenied
        || matches!(err.raw_os_error(), Some(5) | Some(32));
    if in_use {
        return Some(
            "A file under it is in use. A program started from this directory — the tool itself, \
             an editor, or a shell sitting in it — is probably still running."
                .to_string(),
        );
    }
    // Only the errors Windows actually reports for an over-long path. Without this the branch
    // fires on anything that happens to occur deep in a tree -- a genuinely missing directory
    // would be answered with advice about path length.
    //
    // ERROR_PATH_NOT_FOUND (3) is what was measured; ERROR_INVALID_NAME (123) and
    // ERROR_FILENAME_EXCED_RANGE (206) are the other two Windows uses for the same cause. `3` is
    // ambiguous by nature -- a path can be both long and absent -- so the wording below suggests
    // rather than asserts.
    if !matches!(err.raw_os_error(), Some(3) | Some(123) | Some(206)) {
        return None;
    }
    // UTF-16 code units, which is what the limit counts. `OsStr::len()` is WTF-8 bytes on Windows,
    // so a path with any non-ASCII in it would measure long before Windows thought so.
    let units = path.as_os_str().encode_wide().count();
    if units < MAX_PATH - 16 {
        return None;
    }
    // Measured: `std::fs` itself copes well past `MAX_PATH` -- create_dir_all, write and rename all
    // succeeded at 490 units with long paths disabled -- so a "path not found" this close to the
    // limit is more likely the length than the missing directory it appears to be.
    Some(format!(
        "The path is {units} characters, at or near Windows' {MAX_PATH}-character limit, which \
         may be the real cause rather than a missing directory. mise writes through a temporary \
         file whose name is longer than the final one, so it crosses the limit first. Try a \
         shorter directory."
    ))
}

#[cfg(not(windows))]
fn windows_io_hint(_path: &Path, _err: &std::io::Error) -> Option<String> {
    None
}

/// Attach [`windows_io_hint`] to `err`, if it has anything to say about this path.
/// `msg`, plus whatever [`windows_io_hint`] can say about `err` at `path`.
///
/// `pub(crate)` because the operations that need it are not all in this module: `tempfile`'s
/// persist is used directly by the downloader too, and that is the call that fails first as a
/// path approaches `MAX_PATH`.
pub fn with_io_hint(msg: String, path: &Path, err: &std::io::Error) -> String {
    match windows_io_hint(path, err) {
        Some(hint) => format!("{msg}\n{hint}"),
        None => msg,
    }
}

pub fn remove_all<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    // `symlink_metadata`, not `metadata`: the latter resolves the entry before deciding what to do
    // with it, so a link that no longer points anywhere reports `NotFound` and falls through to
    // the no-op arm below — the entry stays on disk and the caller is told nothing. `mise link`
    // creates exactly such an entry whenever its target is moved or deleted. Resolving also made
    // the `is_symlink` test unreachable, since a followed link never reports as one.
    match fs::symlink_metadata(path).map(|m| m.file_type()) {
        // Removing the link, never what it points at. On Windows `make_symlink` writes a junction,
        // which is a directory carrying a reparse point: `remove_file` refuses it outright
        // (measured: `PermissionDenied`, live or dangling), so this goes through the helper that
        // deletes by handle after checking the reparse tag.
        Ok(x) if x.is_symlink() => {
            remove_symlink_or_junction(path)?;
        }
        Ok(x) if x.is_file() => {
            remove_file(path)?;
        }
        Ok(x) if x.is_dir() => {
            trace!("rm -rf {}", display_path(path));
            // `map_err` rather than `wrap_err_with`: the hint depends on the error, and
            // `wrap_err_with`'s closure is not given it.
            fs::remove_dir_all(path).map_err(|e| {
                let msg = with_io_hint(
                    // Not "rm -rf": mise calls `remove_dir_all`, and on Windows it is naming a
                    // command the reader does not have.
                    format!("failed to remove: {}", display_path(path)),
                    path,
                    &e,
                );
                eyre::eyre!(e).wrap_err(msg)
            })?;
        }
        _ => {}
    };
    Ok(())
}

/// Remove `name` under `parent` and, for a directory, everything below it,
/// without following a symlink anywhere in the tree. Every entry is resolved
/// relative to a descriptor for the directory holding it: a symlink is
/// unlinked, never descended into, and a directory is opened with
/// `O_NOFOLLOW`, so an entry swapped for a symlink mid-walk fails the open
/// instead of redirecting the removal. Directories are removed bottom-up. A
/// missing `name` is not an error.
#[cfg(unix)]
pub fn remove_all_at<Fd: std::os::fd::AsFd>(parent: Fd, name: &std::ffi::OsStr) -> Result<()> {
    let stat =
        match nix::sys::stat::fstatat(&parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(nix::errno::Errno::ENOENT) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
    // Compare the whole type field: sockets and block devices share the
    // directory bit.
    let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode & nix::libc::S_IFMT);
    if kind == nix::sys::stat::SFlag::S_IFDIR {
        let fd = nix::fcntl::openat(
            &parent,
            name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )?;
        let mut directory = nix::dir::Dir::from_fd(fd)?;
        let entries = directory
            .iter()
            .map(|entry| entry.map(|entry| entry.file_name().to_owned()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for entry in entries {
            if entry.as_bytes() != b"." && entry.as_bytes() != b".." {
                remove_all_at(&directory, std::ffi::OsStr::from_bytes(entry.to_bytes()))?;
            }
        }
        nix::unistd::unlinkat(parent, name, nix::unistd::UnlinkatFlags::RemoveDir)?;
    } else {
        nix::unistd::unlinkat(parent, name, nix::unistd::UnlinkatFlags::NoRemoveDir)?;
    }
    Ok(())
}

/// Removes a path, retrying when a concurrent writer recreates directory entries while
/// [`fs::remove_dir_all`] is running.
///
/// This remains a strict removal operation: if the directory is still non-empty after the
/// retries are exhausted, the final error is returned to the caller.
pub fn remove_all_with_retry<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    retry_remove_all(|| remove_all(path))
}

fn retry_remove_all(mut remove: impl FnMut() -> Result<()>) -> Result<()> {
    const MAX_RETRIES: u32 = 4;

    for retry in 0..MAX_RETRIES {
        match remove() {
            Ok(()) => return Ok(()),
            Err(err)
                if err
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|err| err.kind() == std::io::ErrorKind::DirectoryNotEmpty) =>
            {
                std::thread::sleep(Duration::from_millis(10 * (1 << retry)));
            }
            Err(err) => return Err(err),
        }
    }
    remove()
}

/// Removes a path whose kind the caller does not know, resolving it first.
///
/// Resolving is why this is not what a cache or install walk wants: a link is
/// judged by what it points at. The remaining caller knows it is handling a
/// staged Homebrew payload, which has no links pointing out of it.
#[cfg(unix)]
pub fn remove_file_or_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    match path.metadata().map(|m| m.file_type()) {
        Ok(x) if x.is_dir() => {
            remove_dir(path)?;
        }
        _ => {
            remove_file(path)?;
        }
    };
    Ok(())
}

pub fn remove_file<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("rm {}", display_path(path));
    fs::remove_file(path).wrap_err_with(|| format!("failed rm: {}", display_path(path)))
}

pub async fn remove_file_async_if_exists<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("rm {}", display_path(path));
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).wrap_err_with(|| format!("failed rm: {}", display_path(path))),
    }
}

pub fn remove_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    (|| -> Result<()> {
        if path.exists() && is_empty_dir(path)? {
            trace!("rmdir {}", display_path(path));
            fs::remove_dir(path)?;
        }
        Ok(())
    })()
    .wrap_err_with(|| format!("failed to remove_dir: {}", display_path(path)))
}

pub fn remove_dir_ignore<P: AsRef<Path>>(path: P, is_empty_ignore_files: Vec<&str>) -> Result<()> {
    let path = path.as_ref();
    (|| -> Result<()> {
        if path.exists() && is_empty_dir_ignore(path, is_empty_ignore_files)? {
            trace!("rm -rf {}", display_path(path));
            remove_all_with_warning(path)?;
        }
        Ok(())
    })()
    .wrap_err_with(|| format!("failed to remove_dir: {}", display_path(path)))
}

pub fn remove_all_with_warning<P: AsRef<Path>>(path: P) -> Result<()> {
    remove_all(&path).map_err(|e| {
        warn!("failed to remove {}: {}", path.as_ref().display(), e);
        e
    })
}

/// Whether a directory entry is there at all, without resolving it.
///
/// [`Path::exists`] answers about a link's *target*, so it is false for one whose target is gone —
/// and that entry is exactly the thing a caller asking "is there something here to remove, or to
/// tell the user about?" needs to find.
pub fn entry_exists<P: AsRef<Path>>(path: P) -> bool {
    fs::symlink_metadata(path.as_ref()).is_ok()
}

pub fn remove_all_with_progress<P: AsRef<Path>>(path: P, pr: &dyn SingleReport) -> Result<()> {
    let path = path.as_ref();
    // Not `exists()`: a link whose target is gone is still an entry to remove, and reporting
    // nothing to do would leave it behind exactly where a user went looking for it.
    if !entry_exists(path) {
        return Ok(());
    }
    pr.set_message(format!("remove {}", display_path(path)));
    remove_all_with_warning(path)
}

/// Renames `from` to `to`.
///
/// Warning: this is the raw `rename(2)`/`fs::rename` behavior. It is atomic on a
/// single filesystem, but it will fail if `from` and `to` are on different
/// mounts. If you need a cross-device-safe move, use [`move_file`] instead.
///
/// On Windows, retries transient failures (`ERROR_ACCESS_DENIED` / `ERROR_SHARING_VIOLATION`)
/// that commonly occur when antivirus or the OS still holds handles to files in the source
/// directory (e.g. after extracting an archive).
pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    try_rename(from, to).wrap_err_with(|| {
        format!(
            "failed rename: {} -> {}",
            display_path(from),
            display_path(to)
        )
    })
}

pub fn try_rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> std::io::Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    trace!("mv {} {}", from.display(), to.display());
    do_rename(from, to)
}

#[cfg(windows)]
fn do_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    const MAX_ATTEMPTS: u32 = 5;
    let mut last_err = None;
    for attempt in 0..MAX_ATTEMPTS {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if matches!(e.raw_os_error(), Some(5) | Some(32)) => {
                // ERROR_ACCESS_DENIED (5) or ERROR_SHARING_VIOLATION (32):
                // likely a transient lock from antivirus or the OS.
                // Exponential backoff: 50ms, 100ms, 200ms, 400ms, 800ms
                last_err = Some(e);
                if attempt + 1 < MAX_ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(50 * (1 << attempt)));
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap())
}

#[cfg(not(windows))]
fn do_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

/// Moves a path, falling back to copy+remove when source and destination are on different filesystems.
///
/// This preserves the normal `rename` behavior when possible, but avoids cross-device failures
/// (`ErrorKind::CrossesDevices`) when `from` and `to` live on separate mounts (for example, when
/// downloads are cached on one volume and installs are written to another). Directory fallbacks
/// preserve symlinks and file/directory permissions.
pub fn move_file<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();

    match try_rename(from, to) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
            if from.is_dir() {
                create_dir_all(to)?;
                copy_dir_all_preserve_symlinks(from, to)?;
                remove_all(from)?;
            } else {
                copy(from, to)?;
                remove_file(from)?;
            }
            Ok(())
        }
        Err(err) => Err(err).wrap_err_with(|| {
            format!(
                "failed move: {} -> {}",
                display_path(from),
                display_path(to)
            )
        }),
    }
}

pub fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    trace!("cp {} {}", from.display(), to.display());
    fs::copy(from, to)
        .wrap_err_with(|| {
            format!(
                "failed copy: {} -> {}",
                display_path(from),
                display_path(to)
            )
        })
        .map(|_| ())
}

/// Give `from`'s content a second name at `to` without duplicating it, falling back to a
/// copy.
///
/// A hard link needs both paths on one volume and a filesystem that supports them, so
/// failing is an ordinary outcome rather than an error worth surfacing.
pub fn hard_link_or_copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    match fs::hard_link(from, to) {
        Ok(()) => {
            trace!("ln {} {}", from.display(), to.display());
            Ok(())
        }
        Err(err) => {
            trace!(
                "ln {} {} failed ({err}), copying instead",
                from.display(),
                to.display()
            );
            copy(from, to)
        }
    }
}

pub fn copy_dir_all_preserve_symlinks(from: &Path, to: &Path) -> Result<()> {
    copy_dir_all_preserve_symlinks_skipping(from, to, &[])
}

/// Copies `from` into `to` like [`copy_dir_all_preserve_symlinks`], without
/// descending into top-level entries of `from` named in `skip_top_level`.
fn copy_dir_all_preserve_symlinks_skipping(
    from: &Path,
    to: &Path,
    skip_top_level: &[&str],
) -> Result<()> {
    trace!("cp -a {} {}", from.display(), to.display());
    let mut directory_permissions = vec![(to.to_path_buf(), fs::metadata(from)?.permissions())];
    let entries = WalkDir::new(from)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() != 1
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| skip_top_level.contains(&name))
        });
    for entry in entries {
        let entry = entry?;
        let relative = entry.path().strip_prefix(from)?;
        let dest = to.join(relative);
        if entry.file_type().is_dir() {
            create_dir_all(&dest)?;
            directory_permissions.push((dest, entry.metadata()?.permissions()));
        } else if entry.file_type().is_symlink() {
            create_dir_all(dest.parent().unwrap())?;
            make_symlink(&fs::read_link(entry.path())?, &dest)?;
        } else if entry.file_type().is_file() {
            create_dir_all(dest.parent().unwrap())?;
            copy(entry.path(), &dest)?;
        }
    }
    // Apply directory permissions after copying children so read-only source
    // directories do not prevent populating their destination.
    for (path, permissions) in directory_permissions.into_iter().rev() {
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    trace!("write {}", display_path(path));
    fs::write(path, contents).wrap_err_with(|| format!("failed write: {}", display_path(path)))
}

pub struct PreparedAtomicWrite {
    temporary: tempfile::NamedTempFile,
    target: PathBuf,
    parent: PathBuf,
}

impl PreparedAtomicWrite {
    /// Atomically renames this already-written replacement into place.
    pub fn commit(self) -> Result<()> {
        // The hint matters most here. `tempfile`'s persist does not get the extended-length path
        // handling `std::fs` applies, so this is the operation that fails first as a path
        // approaches `MAX_PATH` -- measured breaking at a 253-character target while `fs::rename`
        // on the same tree succeeded at 415.
        persist_atomic(self.temporary, &self.target).map_err(|e| {
            let msg = format!("failed atomic write: {}", display_path(&self.target));
            // Resolve the hint before `wrap_err` takes `e` by value: `downcast_ref` borrows it,
            // and doing both in one expression leaves the borrow alive across the move.
            let msg = match e.downcast_ref::<std::io::Error>() {
                Some(io) => with_io_hint(msg, &self.target, io),
                None => msg,
            };
            e.wrap_err(msg)
        })?;
        sync_dir(&self.parent)?;
        Ok(())
    }
}

/// Writes and syncs a complete replacement beside `path` without changing `path` yet.
///
/// New files use ordinary write permissions subject to the process umask. Replacements preserve
/// the destination's existing Unix permissions.
pub fn prepare_atomic_write<P: AsRef<Path>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
) -> Result<PreparedAtomicWrite> {
    let path = path.as_ref();
    trace!("prepare_atomic_write {}", display_path(path));
    let target = atomic_write_target(path)?;
    let path = target.as_path();
    let parent = path
        .parent()
        .ok_or_else(|| eyre::eyre!("path has no parent: {}", display_path(path)))?;
    let prefix = format!(
        ".{}.",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let mut builder = tempfile::Builder::new();
    builder.prefix(&prefix);

    #[cfg(unix)]
    let existing_permissions = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.into()),
    };

    #[cfg(unix)]
    if existing_permissions.is_none() {
        builder.permissions(fs::Permissions::from_mode(0o666));
    }

    let mut temporary = builder.tempfile_in(parent)?;

    #[cfg(unix)]
    if let Some(permissions) = existing_permissions {
        temporary.as_file().set_permissions(permissions)?;
    }

    temporary.write_all(contents.as_ref())?;

    temporary.as_file_mut().sync_all()?;
    let parent = parent.to_path_buf();
    Ok(PreparedAtomicWrite {
        temporary,
        target,
        parent,
    })
}

/// Writes a complete replacement beside `path`, then atomically renames it into place.
pub fn write_atomic<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    trace!("write_atomic {}", display_path(path));
    prepare_atomic_write(path, contents)?.commit()
}

pub fn atomic_write_target(path: &Path) -> Result<PathBuf> {
    const MAX_SYMLINKS: usize = 40;

    let mut target = path.to_path_buf();
    for followed in 0..=MAX_SYMLINKS {
        if !target.is_symlink() {
            return Ok(desymlink_path(&target));
        }
        if followed == MAX_SYMLINKS {
            break;
        }
        let link = fs::read_link(&target)
            .wrap_err_with(|| format!("failed to read symlink: {}", display_path(&target)))?;
        target = if link.is_absolute() {
            link
        } else {
            target.parent().unwrap_or_else(|| Path::new("")).join(link)
        };
    }

    bail!(
        "too many symlinks while resolving atomic write target: {}",
        display_path(path)
    )
}

pub fn persist_atomic(mut temporary: tempfile::NamedTempFile, path: &Path) -> Result<()> {
    const RETRIES: u32 = 20;

    for attempt in 0..=RETRIES {
        match temporary.persist(path) {
            Ok(_) => return Ok(()),
            Err(err) if should_retry_atomic_persist(&err.error) && attempt < RETRIES => {
                temporary = err.file;
                std::thread::sleep(Duration::from_millis(5 * u64::from(attempt + 1)));
            }
            Err(err) => return Err(err.error.into()),
        }
    }

    unreachable!("atomic persist retry loop should always return");
}

#[cfg(windows)]
fn should_retry_atomic_persist(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::PermissionDenied
        || matches!(err.raw_os_error(), Some(5) | Some(32))
}

#[cfg(not(windows))]
fn should_retry_atomic_persist(_err: &std::io::Error) -> bool {
    false
}

pub async fn write_async<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    trace!("write {}", display_path(path));
    tokio::fs::write(path, contents)
        .await
        .wrap_err_with(|| format!("failed write: {}", display_path(path)))
}

pub fn read_to_string<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    trace!("cat {}", path.display_user());
    fs::read_to_string(path)
        .wrap_err_with(|| format!("failed read_to_string: {}", path.display_user()))
}

/// The bytes of a UTF-8 byte-order mark, as [`decode_text`] matches them below.
#[cfg(windows)]
pub const UTF8_BOM_BYTES: [u8; 3] = [0xef, 0xbb, 0xbf];

/// `s` without a leading UTF-8 byte-order mark.
///
/// Needed wherever mise matches on the *start* of a line it read from disk. `str::trim` does not
/// help: U+FEFF does not carry the Unicode `White_Space` property, so a mark left in front of a
/// `#!` or a `#MISE` defeats every prefix test silently. The writers that leave one there are
/// ordinary on Windows -- see [`decode_text`], which names them.
pub fn strip_utf8_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// Decode text that may begin with a byte-order mark.
///
/// `std`'s UTF-8-only readers reject UTF-16 outright, which is how a checksum file sank an install
/// in #5399: PowerShell shipped `hashes.sha256` as UTF-16LE and mise stopped at "stream did not
/// contain valid UTF-8". Windows PowerShell 5.1's `Out-File` writes UTF-16LE by default, so any
/// project generating checksums that way produces the same thing.
///
/// Only a BOM switches the encoding. Detecting UTF-16 without one means guessing from the density
/// of NUL bytes, which can misfire on binary input; `Out-File` always writes a BOM, so the guess
/// buys nothing here. Input with no BOM is decoded as UTF-8, exactly as before.
pub fn decode_text(bytes: &[u8]) -> Result<String> {
    fn from_utf16(bytes: &[u8], to_u16: fn([u8; 2]) -> u16, label: &str) -> Result<String> {
        if !bytes.len().is_multiple_of(2) {
            bail!(
                "truncated {label} text: {} bytes is not a whole number of code units",
                bytes.len()
            );
        }
        let units = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| to_u16(*c))
            .collect_vec();
        String::from_utf16(&units).wrap_err_with(|| format!("invalid {label} text"))
    }

    match bytes {
        [0xef, 0xbb, 0xbf, rest @ ..] => {
            String::from_utf8(rest.to_vec()).wrap_err("invalid UTF-8 text after a UTF-8 BOM")
        }
        [0xff, 0xfe, rest @ ..] => from_utf16(rest, u16::from_le_bytes, "UTF-16LE"),
        [0xfe, 0xff, rest @ ..] => from_utf16(rest, u16::from_be_bytes, "UTF-16BE"),
        _ => String::from_utf8(bytes.to_vec())
            .wrap_err("invalid UTF-8 text, and no byte-order mark identifying another encoding"),
    }
}

/// The UTF-16 encoding `path` announces with a byte-order mark, if it announces one.
///
/// The marks are the ones [`decode_text`] matches, kept here rather than spelled out again at the
/// call site so there is one description of what a mark looks like.
///
/// A UTF-8 mark is deliberately not reported. `has_shebang` already looks past that one, so a
/// file carrying it is executable and never reaches a caller that needs to explain why it is not.
/// (Named without a link: that function is `#[cfg(windows)]` and this doc is built on unix too.)
///
/// Compiled for tests on every platform so the byte matching is checked everywhere, but only used
/// on Windows: on unix the execute bit decides what is executable, and a UTF-16 script with that
/// bit set fails at exec rather than being skipped.
#[cfg(any(windows, test))]
pub fn utf16_bom(path: &Path) -> Option<&'static str> {
    // `read_to_end` on a `take`, not `read_exact`: a one-byte file is a legitimate answer of
    // "no mark", and `read_exact` would fail on it. Same shape as `has_shebang` below.
    let bytes = std::fs::File::open(path)
        .and_then(|f| {
            use std::io::Read;
            let mut buf = Vec::with_capacity(2);
            f.take(2).read_to_end(&mut buf)?;
            Ok(buf)
        })
        .ok()?;
    match bytes[..] {
        [0xff, 0xfe] => Some("UTF-16LE"),
        [0xfe, 0xff] => Some("UTF-16BE"),
        _ => None,
    }
}

/// [`read_to_string`], but tolerant of a byte-order mark. See [`decode_text`].
///
/// Only reads *from disk* need this. Bodies fetched over HTTP already arrive decoded: reqwest's
/// `text()` goes through `text_with_charset` -> `encoding_rs::Encoding::decode`, which sniffs a BOM
/// and lets it override the declared charset. `std::fs::read_to_string` has no such step, and that
/// asymmetry is the only reason this function exists — reaching for it on an `HTTP.get_text` result
/// would be redundant.
pub fn read_to_string_bom<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    trace!("cat {}", path.display_user());
    let bytes = fs::read(path).wrap_err_with(|| format!("failed read: {}", path.display_user()))?;
    decode_text(&bytes).wrap_err_with(|| format!("failed to decode {}", path.display_user()))
}

pub async fn read_to_string_async<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    trace!("cat {}", path.display_user());
    tokio::fs::read_to_string(path)
        .await
        .wrap_err_with(|| format!("failed read_to_string: {}", path.display_user()))
}

pub fn create(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    trace!("touch {}", display_path(path));
    File::create(path).wrap_err_with(|| format!("failed create: {}", display_path(path)))
}

pub fn create_dir_all<P: AsRef<Path>>(path: P) -> Result<()> {
    static LOCK: Lazy<Mutex<u8>> = Lazy::new(Default::default);
    let _lock = LOCK.lock().unwrap();

    let path = path.as_ref();
    if !path.exists() {
        trace!("mkdir -p {}", display_path(path));
        if let Err(err) = fs::create_dir_all(path) {
            // if not exists error
            if err.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(err)
                    .wrap_err_with(|| format!("failed create_dir_all: {}", display_path(path)));
            }
        }
    }
    Ok(())
}

/// A path formatted for a person to read: `$HOME` becomes `~`, a Windows extended-length prefix
/// is dropped, and separators settle on the host's.
///
/// The separator step lives here rather than in [`PathExt::display_user`] because that one also
/// feeds strings mise *matches* rather than shows — see [`crate::path::settle_display_separators`].
pub fn display_path<P: AsRef<Path>>(path: P) -> String {
    crate::path::settle_display_separators(path.as_ref().display_user())
}

pub fn display_filename<P: AsRef<Path>>(path: P) -> String {
    let path = path.as_ref();
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

pub fn display_rel_path<P: AsRef<Path>>(path: P) -> String {
    let path = path.as_ref();
    match path.strip_prefix(dirs::CWD.as_ref().unwrap()) {
        // Both halves take the host's separator. Hardcoding `./` and printing the remainder raw
        // produced `./mise-tasks\build` on Windows. Byte-identical off Windows, where
        // `MAIN_SEPARATOR` is `/` and nothing is rewritten.
        Ok(rel) => format!(".{}{}", std::path::MAIN_SEPARATOR, display_path(rel)),
        Err(_) => display_path(path),
    }
}

/// replaces $HOME in a string with "~" and $PATH with "$PATH", generally used to clean up output
/// after it is rendered
pub fn replace_paths_in_string<S: Display>(input: S) -> String {
    let home = env::HOME.to_string_lossy().to_string();
    input.to_string().replace(&home, "~")
}

/// replaces "~" with $HOME
///
/// The remainder is re-joined one component at a time rather than pushed as a
/// single slice. `Path::strip_prefix` returns a raw subslice of the input — it
/// trims the remainder's leading/trailing separators but leaves interior ones
/// untouched — so `HOME.join(rest)` only prepends a separator. On Windows that
/// made `~/.local/share/mise` expand to `C:\Users\me\.local/share/mise`, and
/// since `MISE_DATA_DIR` flows into `dirs::DATA`/`INSTALLS`, that mixed-separator
/// path surfaced verbatim in `mise where`, `mise which`, `mise ls --json`,
/// `mise bin-paths`, shims, and error messages.
///
/// Rebuilding from `components()` also folds redundant separators and `.`
/// segments; `..` is preserved. On unix the result is byte-identical to the old
/// behavior for any ordinary input.
///
/// Paths without a `~/` prefix are returned unchanged: a user-supplied
/// `C:/mise/data` stays exactly as typed. This is a tilde expander, not a path
/// normalizer — one caller passes glob patterns through it
/// (`config::expand_task_include`).
pub fn replace_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    match path.strip_prefix("~/") {
        Ok(rest) => {
            let mut expanded = dirs::HOME.to_path_buf();
            for component in rest.components() {
                expanded.push(component.as_os_str());
            }
            expanded
        }
        Err(_) => path.to_path_buf(),
    }
}

/// Compare two paths for filesystem equivalence, taking platform conventions
/// into account. macOS volumes (HFS+/APFS) and Windows volumes are
/// case-insensitive by default, so a byte-equal comparison can fail when
/// inputs differ only by case (e.g. `/Users/Foo/...` vs `/Users/foo/...`
/// when `$HOME` is mixed-case in the user's environment but the resolved
/// path uses a different case).
///
/// On case-insensitive platforms, comparison is done over `Path::components()`
/// with each component lowercased — this also folds trailing slashes,
/// redundant separators, and (on Windows) `/` vs `\` since `Path::components`
/// treats both as separators.
///
/// This is the right comparator for "is this PATH entry the shims
/// directory?" checks, where a false negative leads to mise's shim being
/// inherited by a child process and recursing infinitely.
pub fn paths_eq(a: &Path, b: &Path) -> bool {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let normalize =
            |c: std::path::Component<'_>| c.as_os_str().to_string_lossy().to_lowercase();
        a.components()
            .map(normalize)
            .eq(b.components().map(normalize))
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        a == b
    }
}

/// Compare configured storage paths by both platform-aware spelling and
/// resolved filesystem identity. The latter matters when distributions
/// deliberately point user and system storage at the same directory through
/// different symlinks.
pub fn storage_paths_eq(a: &Path, b: &Path) -> bool {
    paths_eq(a, b) || same_file(a, b)
}

pub fn touch_file(file: &Path) -> Result<()> {
    if !file.exists() {
        create(file)?;
        return Ok(());
    }
    trace!("touch_file {}", file.display());
    let now = FileTime::now();
    set_file_times(file, now, now)
        .wrap_err_with(|| format!("failed to touch file: {}", display_path(file)))
}

pub fn touch_dir(dir: &Path) -> Result<()> {
    trace!("touch {}", dir.display());
    let now = FileTime::now();
    set_file_times(dir, now, now)
        .wrap_err_with(|| format!("failed to touch dir: {}", display_path(dir)))
}

/// Synchronizes a directory to disk, ensuring that filesystem metadata changes
/// (such as file creations or deletions) are persisted.
///
/// This is important after operations like removing files to ensure the changes
/// are immediately visible to other processes, e.g. to avoid race conditions.
///
/// # Platform-specific behavior
///
/// - **Unix/Linux**: Performs an fsync on the directory file descriptor, which
///   ensures directory metadata (like file listings) is written to disk.
/// - **Windows**: Not implemented (no-op).
///
/// # Errors
///
/// On Unix systems, returns an error if the directory cannot be opened or synced.
/// On Windows, always succeeds.
#[cfg(unix)]
pub fn sync_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("sync {}", display_path(path));
    let dir = File::open(path)
        .wrap_err_with(|| format!("failed to open dir for sync: {}", display_path(path)))?;
    dir.sync_all()
        .wrap_err_with(|| format!("failed to sync dir: {}", display_path(path)))
}

#[cfg(windows)]
pub fn sync_dir<P: AsRef<Path>>(_path: P) -> Result<()> {
    // Not implemented on Windows
    Ok(())
}

pub fn modified_duration(path: &Path) -> Result<Duration> {
    let metadata = path.metadata()?;
    let modified = metadata.modified()?;
    let duration = modified.elapsed().unwrap_or_default();
    Ok(duration)
}

pub fn find_up<FN: AsRef<str>>(from: &Path, filenames: &[FN]) -> Option<PathBuf> {
    let mut current = from.to_path_buf();
    loop {
        for filename in filenames {
            let path = current.join(filename.as_ref());
            if path.exists() {
                return Some(path);
            }
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Names of the directories in `dir`, including links that lead to one. A link whose target is
/// gone is dropped: it is not a directory a caller can read a plugin, a cached download or another
/// version manager's install out of. (Version listing keeps those; it has its own scan.)
pub fn dir_subdirs(dir: &Path) -> Result<BTreeSet<String>> {
    let mut output = Default::default();

    if !dir.exists() {
        return Ok(output);
    }

    for entry in dir.read_dir()? {
        let entry = entry?;
        // `entry.file_type()` describes the entry itself; `entry.path().is_dir()` resolves it.
        let ft = entry.file_type()?;
        if ft.is_dir() || (ft.is_symlink() && entry.path().is_dir()) {
            output.insert(entry.file_name().into_string().unwrap());
        }
    }

    Ok(output)
}

pub fn ls(dir: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut output = Default::default();

    if !dir.is_dir() {
        return Ok(output);
    }

    for entry in dir.read_dir()? {
        let entry = entry?;
        output.insert(entry.path());
    }

    Ok(output)
}

#[cfg(unix)]
pub fn make_symlink(target: &Path, link: &Path) -> Result<(PathBuf, PathBuf)> {
    trace!("ln -sf {} {}", target.display(), link.display());
    // Create the symlink at a unique temporary name in the same directory, then
    // atomically rename it over `link`. rename(2) replaces an existing path in a
    // single step, so concurrent mise processes racing to create the same link all
    // succeed (last writer wins) instead of one failing with EEXIST — which showed
    // up as spurious "failed to ln -sf ...: File exists (os error 17)" warnings
    // when several mise invocations start at once (e.g. spawning a git worktree,
    // #10292). Approach based on the closed PR #9701.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let file_name = link
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("symlink");
    let tmp = link.with_file_name(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_file(&tmp);
    symlink(target, &tmp)
        .wrap_err_with(|| format!("failed to ln -sf {} {}", target.display(), link.display()))?;
    if let Err(err) = fs::rename(&tmp, link) {
        let _ = fs::remove_file(&tmp);
        return Err(err)
            .wrap_err_with(|| format!("failed to ln -sf {} {}", target.display(), link.display()));
    }
    Ok((target.to_path_buf(), link.to_path_buf()))
}

#[cfg(unix)]
pub fn make_symlink_or_copy(target: &Path, link: &Path) -> Result<()> {
    make_symlink(target, link)?;
    Ok(())
}

#[cfg(windows)]
pub fn make_symlink_or_copy(target: &Path, link: &Path) -> Result<()> {
    copy(target, link)?;
    Ok(())
}

#[cfg(windows)]
pub fn is_unc_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(prefix))
            if matches!(
                prefix.kind(),
                std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..)
            )
    )
}

#[cfg(windows)]
fn create_windows_unc_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link).map_err(|err| {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            std::io::Error::new(
                err.kind(),
                format!(
                    "{err}. Creating directory symlinks on Windows may require administrator privileges or Developer Mode"
                ),
            )
        } else {
            err
        }
    })
}

#[cfg(windows)]
fn create_windows_dir_link(target: &Path, link: &Path) -> std::io::Result<()> {
    if is_unc_path(target) {
        return create_windows_unc_symlink(target, link);
    }
    let existed = fs::symlink_metadata(link).is_ok();
    let result = junction::create(target, link);
    // `junction::create` makes the directory before it sets the reparse point,
    // so a later failure (a target too long for the reparse buffer) leaves a
    // plain directory that would pass for an installed version. Remove only what
    // this call created, and only if it is still an empty plain directory.
    // `AlreadyExists` means `create_dir` itself failed because something else holds the
    // name (another process won the race), so there is nothing of ours to remove.
    if let Err(err) = &result
        && err.kind() != std::io::ErrorKind::AlreadyExists
        && !existed
        && junction::get_target(link).is_err()
    {
        let _ = fs::remove_dir(link);
    }
    result
}

#[cfg(windows)]
pub fn make_symlink(target: &Path, link: &Path) -> Result<(PathBuf, PathBuf)> {
    if let Err(err) = create_windows_dir_link(target, link) {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            remove_symlink_or_junction(link)?;
            create_windows_dir_link(target, link)
        } else {
            Err(err)
        }
    } else {
        Ok(())
    }
    .wrap_err_with(|| format!("failed to ln -sf {} {}", target.display(), link.display()))?;
    Ok((target.to_path_buf(), link.to_path_buf()))
}

#[cfg(windows)]
pub fn make_symlink_or_file(target: &Path, link: &Path) -> Result<()> {
    trace!("ln -sf {} {}", target.display(), link.display());
    if link.is_file() || link.is_symlink() {
        // remove existing file if exists
        fs::remove_file(link)?;
    }
    xx::file::write(link, target.to_string_lossy().to_string())?;
    Ok(())
}

pub fn resolve_symlink(link: &Path) -> Result<Option<PathBuf>> {
    // Windows aliases created before runtime links became junctions are plain
    // files holding the target path, so both forms are read here.
    if link.is_symlink() {
        Ok(Some(fs::read_link(link)?))
    } else if let Some(target) = junction_target(link) {
        Ok(Some(target))
    } else if link.is_file() {
        Ok(Some(fs::read_to_string(link)?.into()))
    } else {
        Ok(None)
    }
}

#[cfg(windows)]
fn junction_target(link: &Path) -> Option<PathBuf> {
    junction::get_target(link).ok()
}

#[cfg(not(windows))]
fn junction_target(_link: &Path) -> Option<PathBuf> {
    None
}

/// Links `link` to the directory `target` (a relative target is taken from
/// `link`'s parent) for a runtime alias such as `installs/node/latest`.
///
/// On Windows this is a real junction, which tools that enumerate the path
/// (IDE SDK selectors) can follow; it never falls back to a text file. A failure
/// is returned so the caller can warn and carry on without the link.
#[cfg(unix)]
pub fn make_dir_link(target: &Path, link: &Path) -> Result<()> {
    make_symlink(target, link)?;
    Ok(())
}

#[cfg(windows)]
pub fn make_dir_link(target: &Path, link: &Path) -> Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let target = resolve_relative_link_target(link, target.to_path_buf());
    let target = target.absolutize()?.into_owned();
    if fs::symlink_metadata(link).is_err() {
        match make_symlink(&target, link) {
            Ok(_) => return Ok(()),
            // Something else made the slot in the meantime: replace it below.
            Err(_) if fs::symlink_metadata(link).is_ok() => {}
            Err(err) => return Err(err),
        }
    }
    // Replacing a link: build the new one beside it first, so a failure to
    // create it leaves the working link in place instead of an empty slot.
    let name = link.file_name().and_then(|n| n.to_str()).unwrap_or("link");
    let tmp = link.with_file_name(format!(
        ".{name}.tmp.{}.{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    make_symlink(&target, &tmp)?;
    // What the link pointed at, to put it back if the swap cannot be completed.
    let previous = resolve_symlink(link)
        .ok()
        .flatten()
        .map(|old| resolve_relative_link_target(link, old));
    let swapped = remove_dir_link(link).and_then(|()| {
        // `rename` retries the transient locks antivirus and the OS put on a fresh entry.
        rename(&tmp, link)
    });
    if swapped.is_err() {
        let _ = remove_symlink_or_junction(&tmp);
        if let Some(previous) = previous
            && fs::symlink_metadata(link).is_err()
        {
            let _ = make_symlink(&previous, link);
        }
    }
    swapped
}

/// Removes a link made by [`make_dir_link`], or an older text-file alias.
#[cfg(unix)]
pub fn remove_dir_link(link: &Path) -> Result<()> {
    if link.is_symlink() || link.is_file() {
        remove_file(link)?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn remove_dir_link(link: &Path) -> Result<()> {
    if is_symlink_or_junction(link) {
        remove_symlink_or_junction(link)
    } else if link.is_file() {
        remove_file(link)
    } else {
        Ok(())
    }
}

pub fn is_symlink_to(link: &Path, target: &Path) -> bool {
    is_symlink_or_junction(link) && same_file::is_same_file(link, target).unwrap_or(false)
}

#[cfg(unix)]
pub fn is_symlink_or_junction(path: &Path) -> bool {
    path.is_symlink()
}

#[cfg(windows)]
pub fn is_symlink_or_junction(path: &Path) -> bool {
    path.is_symlink() || junction::get_target(path).is_ok()
}

#[cfg(unix)]
pub fn make_symlink_or_file(target: &Path, link: &Path) -> Result<()> {
    make_symlink(target, link)?;
    Ok(())
}

pub fn is_symlink_target_within(link: &Path, root: &Path) -> Result<bool> {
    let Some(target) = dir_link_target(link)? else {
        return Ok(false);
    };
    let target = target.absolutize()?;
    let root = root.absolutize()?;
    Ok(path_starts_with(&target, &root))
}

#[cfg(unix)]
fn path_starts_with(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(windows)]
fn path_starts_with(path: &Path, root: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    if path.starts_with(root) {
        return true;
    }

    let root_component_count = root.components().count();
    let root = root.components().collect::<PathBuf>();
    let candidate = path
        .components()
        .take(root_component_count)
        .collect::<PathBuf>();
    if candidate.components().count() != root_component_count {
        return false;
    }

    let candidate_wide = candidate.as_os_str().encode_wide().collect::<Vec<_>>();
    let root_wide = root.as_os_str().encode_wide().collect::<Vec<_>>();
    let Ok(candidate_len) = i32::try_from(candidate_wide.len()) else {
        return false;
    };
    let Ok(root_len) = i32::try_from(root_wide.len()) else {
        return false;
    };
    let equal_ignoring_case = unsafe {
        CompareStringOrdinal(
            candidate_wide.as_ptr(),
            candidate_len,
            root_wide.as_ptr(),
            root_len,
            1,
        ) == CSTR_EQUAL
    };

    if !equal_ignoring_case {
        return false;
    }

    match same_file::is_same_file(&candidate, &root) {
        Ok(same) => same,
        Err(_) => dangling_paths_share_case_insensitive_parent(&candidate, &root),
    }
}

#[cfg(windows)]
fn dangling_paths_share_case_insensitive_parent(path: &Path, root: &Path) -> bool {
    let Some((path_parent, path_missing_components)) = nearest_existing_directory(path) else {
        return false;
    };
    let Some((root_parent, root_missing_components)) = nearest_existing_directory(root) else {
        return false;
    };

    path_missing_components == root_missing_components
        && same_file::is_same_file(&path_parent, &root_parent).unwrap_or(false)
        && directory_is_case_sensitive(&path_parent) == Some(false)
}

#[cfg(windows)]
fn nearest_existing_directory(path: &Path) -> Option<(PathBuf, usize)> {
    let mut path = path;
    let mut missing_components = 0;
    loop {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => {
                return Some((path.to_path_buf(), missing_components));
            }
            Ok(_) => return None,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                path = path.parent()?;
                missing_components += 1;
            }
            Err(_) => return None,
        }
    }
}

#[cfg(windows)]
fn directory_is_case_sensitive(path: &Path) -> Option<bool> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FileCaseSensitiveInfo,
        GetFileInformationByHandleEx,
    };
    use windows_sys::Win32::System::SystemServices::FILE_CS_FLAG_CASE_SENSITIVE_DIR;

    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut case_info = FILE_CASE_SENSITIVE_INFO::default();
    let inspected = unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            FileCaseSensitiveInfo,
            std::ptr::from_mut(&mut case_info).cast(),
            std::mem::size_of::<FILE_CASE_SENSITIVE_INFO>() as u32,
        )
    };
    (inspected != 0).then_some(case_info.Flags & FILE_CS_FLAG_CASE_SENSITIVE_DIR != 0)
}

#[cfg(unix)]
fn dir_link_target(link: &Path) -> Result<Option<PathBuf>> {
    let metadata = match fs::symlink_metadata(link) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .wrap_err_with(|| format!("failed to inspect link: {}", display_path(link)));
        }
    };
    if !metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let target = fs::read_link(link)
        .wrap_err_with(|| format!("failed to read link: {}", display_path(link)))?;
    Ok(Some(resolve_relative_link_target(link, target)))
}

#[cfg(windows)]
fn dir_link_target(link: &Path) -> Result<Option<PathBuf>> {
    const ERROR_NOT_A_REPARSE_POINT: i32 = 4390;

    let metadata = match fs::symlink_metadata(link) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .wrap_err_with(|| format!("failed to inspect link: {}", display_path(link)));
        }
    };
    let target = if metadata.file_type().is_symlink() {
        fs::read_link(link)
            .wrap_err_with(|| format!("failed to read link: {}", display_path(link)))?
    } else {
        match junction::get_target(link) {
            Ok(target) => target,
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound
                    || err.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT)
                    || (err.kind() == std::io::ErrorKind::Other
                        && err.raw_os_error().is_none()) =>
            {
                return Ok(None);
            }
            Err(err) => {
                return Err(err).wrap_err_with(|| {
                    format!("failed to inspect junction: {}", display_path(link))
                });
            }
        }
    };
    Ok(Some(resolve_relative_link_target(link, target)))
}

fn resolve_relative_link_target(link: &Path, target: PathBuf) -> PathBuf {
    if target.is_absolute() {
        target
    } else {
        link.parent().unwrap_or(link).join(target)
    }
}

#[cfg(unix)]
pub fn remove_symlink_or_junction(link: &Path) -> Result<()> {
    // POSIX has no standard unlink-by-handle operation, so a concurrent replacement can still
    // occur between this check and remove_file. The latter cannot remove directories; callers
    // must keep the parent directory protected from untrusted writers.
    if dir_link_target(link)?.is_none() {
        bail!("refusing to remove non-link: {}", display_path(link));
    }
    fs::remove_file(link)
        .wrap_err_with(|| format!("failed to remove symlink: {}", display_path(link)))
}

#[cfg(windows)]
pub fn remove_symlink_or_junction(link: &Path) -> Result<()> {
    let link_handle = open_link_for_removal(link)?;
    remove_open_link(link_handle)
        .wrap_err_with(|| format!("failed to remove link or junction: {}", display_path(link)))
}

#[cfg(windows)]
fn open_link_for_removal(link: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FileAttributeTagInfo, GetFileInformationByHandleEx,
    };
    use windows_sys::Win32::System::SystemServices::{
        IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
    };

    let file = fs::OpenOptions::new()
        .access_mode(DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(link)
        .wrap_err_with(|| format!("failed to open link: {}", display_path(link)))?;
    let mut tag_info = FILE_ATTRIBUTE_TAG_INFO::default();
    let inspected = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            std::ptr::from_mut(&mut tag_info).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if inspected == 0 {
        return Err(std::io::Error::last_os_error())
            .wrap_err_with(|| format!("failed to inspect link: {}", display_path(link)));
    }
    if !matches!(
        tag_info.ReparseTag,
        IO_REPARSE_TAG_SYMLINK | IO_REPARSE_TAG_MOUNT_POINT
    ) {
        bail!("refusing to remove non-link: {}", display_path(link));
    }
    Ok(file)
}

#[cfg(windows)]
fn remove_open_link(file: File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };

    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let removed = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            std::ptr::from_ref(&disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if removed == 0 {
        return Err(std::io::Error::last_os_error());
    }
    drop(file);
    Ok(())
}

#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
    if let Ok(metadata) = path.metadata() {
        return metadata.permissions().mode() & 0o111 != 0;
    }
    false
}

#[cfg(windows)]
pub fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if has_known_executable_extension(path) {
        return true;
    }
    has_shebang(path)
}

/// How to make `path` count as executable, phrased for the platform the user is on.
///
/// Lives beside [`is_executable`] because it has to track it: the two platforms answer that
/// question by different rules, so the remedy differs too. `chmod +x` is not merely unavailable on
/// Windows — it is the wrong instruction, since the Windows branch never looks at a permission bit.
#[cfg(unix)]
pub fn make_executable_hint(path: &Path) -> String {
    format!("Run: chmod +x {}", display_path(path))
}

#[cfg(windows)]
pub fn make_executable_hint(path: &Path) -> String {
    // A shebang the file already carries, in an encoding `has_shebang` reads as bytes and so
    // cannot see. Windows PowerShell 5.1's `>` and `Out-File` write UTF-16LE by default, which
    // makes this the shell that ships with the OS producing a file mise then tells the user to
    // add a shebang to. Naming the encoding is the fix; telling them to add what is already
    // there is not. Said whether or not a shebang is in there, because either way the encoding
    // is what has to change first.
    if let Some(encoding) = utf16_bom(path) {
        return format!(
            "{} is {encoding}. mise reads a shebang as bytes, so save it as UTF-8.",
            display_path(path),
        );
    }
    format!(
        "Add a shebang line to {}, or give it one of these extensions: {}",
        display_path(path),
        // Read from the setting rather than hardcoded: a user who has changed
        // `windows_executable_extensions` would otherwise be told to use extensions mise will not
        // accept from them.
        Settings::get().windows_executable_extensions.join(", ")
    )
}

#[cfg(windows)]
pub fn has_known_executable_extension(path: &Path) -> bool {
    path.extension().map_or(
        Settings::get()
            .windows_executable_extensions
            .contains(&String::new()),
        |ext| {
            if let Some(str_val) = ext.to_str() {
                return Settings::get()
                    .windows_executable_extensions
                    .contains(&str_val.to_lowercase().to_string());
            }
            false
        },
    )
}

/// Check if a file starts with a shebang (#!), allowing for a leading UTF-8 byte-order mark.
///
/// Reads only the first 5 bytes to minimize I/O during task discovery: two for the `#!`, plus
/// three for a mark in front of it. Reading just the two saw `EF BB` and answered "no shebang",
/// which on Windows is the whole of [`is_executable`] for an extensionless file -- so a task
/// written by `Out-File -Encoding utf8` disappeared from `mise tasks` with no diagnostic, while
/// the same file on unix was a task because there the execute bit decides.
///
/// A `read_exact` here would fail outright on a file shorter than the buffer, so read what is
/// available instead. A UTF-16 mark is deliberately not accepted: no interpreter mise dispatches
/// to can run a UTF-16 script, so treating one as a task would only trade silence for a
/// confusing failure at exec time.
#[cfg(windows)]
pub fn has_shebang(path: &Path) -> bool {
    std::fs::File::open(path)
        .and_then(|f| {
            use std::io::Read;
            let mut buf = Vec::with_capacity(UTF8_BOM_BYTES.len() + 2);
            f.take((UTF8_BOM_BYTES.len() + 2) as u64)
                .read_to_end(&mut buf)?;
            let bytes = buf.strip_prefix(UTF8_BOM_BYTES.as_slice()).unwrap_or(&buf);
            Ok(bytes.starts_with(b"#!"))
        })
        .unwrap_or(false)
}

/// Extensions `std::process::Command` can start from a path alone on Windows: `exe` and `com`
/// natively, `bat` and `cmd` because std routes those through cmd.exe with escaped arguments.
/// Anything else needs an interpreter named for it — `pwsh -File` for a `.ps1`,
/// `cscript` for a `.vbs`, whatever a shebang asks for.
///
/// A fixed list on purpose. It describes what the OS can start, which
/// `windows_executable_extensions` has no say over: that setting decides what mise *treats* as
/// executable. Deriving it the other way round — subtracting known interpreter-only extensions
/// from the setting — happened to be right for the default list and wrong for any other, since a
/// user who added `sh` or `py` was then answered "yes, `CreateProcess` can start this" and the
/// task died with "not a valid Win32 application" instead of using its shebang.
///
/// `pub(crate)` so `task::task_executor` can assert that `shell_from_extension` names an
/// interpreter for every default extension this list excludes.
#[cfg(windows)]
pub const OS_LAUNCHABLE_EXTENSIONS: [&str; 4] = ["exe", "com", "bat", "cmd"];

/// Whether the OS can start a file with this extension without an interpreter.
///
/// Compared case-insensitively, the way [`has_known_executable_extension`] does: Windows
/// extensions are not case-sensitive, so `PIPX.PS1` is the same file as `pipx.ps1`.
#[cfg(windows)]
pub fn os_can_launch_extension(ext: &str) -> bool {
    OS_LAUNCHABLE_EXTENSIONS
        .iter()
        .any(|known| ext.eq_ignore_ascii_case(known))
}

/// Check if a file can be executed directly by the OS without a shell wrapper.
/// On Unix, this checks the executable permission bit.
/// On Windows, it takes both questions in turn: does mise treat this extension as executable
/// (`windows_executable_extensions`, which a user may narrow), and can the OS actually start it
/// ([`OS_LAUNCHABLE_EXTENSIONS`], which a user may not widen)?
///
/// Distinct from [`is_executable`], which on Windows deliberately also accepts a
/// shebang-only file. Callers that hand the path to `Command::new` need this one:
/// `CreateProcess` can run `.exe`/`.com`/`.cmd`/`.bat`, but not a `.ps1`, a `.vbs`,
/// or a script that only carries a shebang.
///
/// Note that on Windows this never touches the filesystem — it is pure extension
/// inspection, so it answers true for a `foo.exe` that does not exist. `is_executable`
/// checks `is_file()` inline; this one does not. A lookup that walks candidate paths must
/// compose the two, which is what [`is_spawnable`] does.
pub fn can_execute_directly(path: &Path) -> bool {
    #[cfg(windows)]
    {
        has_known_executable_extension(path)
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(os_can_launch_extension)
    }
    #[cfg(not(windows))]
    {
        is_executable(path)
    }
}

/// True when the OS will accept `path` as the program argument of a spawn.
pub fn is_spawnable(path: &Path) -> bool {
    if cfg!(windows) && !path.is_file() {
        return false;
    }
    can_execute_directly(path)
}

#[cfg(unix)]
pub fn make_executable<P: AsRef<Path>>(path: P) -> Result<()> {
    trace!("chmod +x {}", display_path(&path));
    let path = path.as_ref();
    let mut perms = path.metadata()?.permissions();
    perms.set_mode(executable_mode(perms.mode()));
    fs::set_permissions(path, perms)
        .wrap_err_with(|| format!("failed to chmod +x: {}", display_path(path)))?;
    Ok(())
}

/// Add execute bits along with the matching read bits.
///
/// A file that only receives the execute bits (`mode | 0o111`) can end up executable but not
/// readable (e.g. a `0o600` tempfile becomes `0o711`). For interpreted executables like PHP PHARs
/// the interpreter must be able to *read* the file, so any class that gains execute must also gain
/// read. See https://github.com/jdx/mise/discussions/11108.
#[cfg(unix)]
fn executable_mode(mode: u32) -> u32 {
    mode | 0o111 | 0o444
}

#[cfg(windows)]
pub fn make_executable<P: AsRef<Path>>(_path: P) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub async fn make_executable_async<P: AsRef<Path>>(path: P) -> Result<()> {
    trace!("chmod +x {}", display_path(&path));
    let path = path.as_ref();
    let mut perms = path.metadata()?.permissions();
    perms.set_mode(executable_mode(perms.mode()));
    tokio::fs::set_permissions(path, perms)
        .await
        .wrap_err_with(|| format!("failed to chmod +x: {}", display_path(path)))
}

#[cfg(windows)]
pub async fn make_executable_async<P: AsRef<Path>>(_path: P) -> Result<()> {
    Ok(())
}

pub fn all_dirs<P: AsRef<Path>>(
    start_dir: P,
    ceiling_dirs: &HashSet<PathBuf>,
) -> Result<Vec<PathBuf>> {
    trace!(
        "file::all_dirs Collecting all ancestors of {} until ceiling {:?}",
        display_path(&start_dir),
        ceiling_dirs
    );
    Ok(start_dir
        .as_ref()
        .ancestors()
        .map_while(|p| {
            if ceiling_dirs.contains(p) {
                debug!(
                    "file::all_dirs Reached ceiling directory: {}",
                    display_path(p)
                );
                None
            } else {
                trace!(
                    "file::all_dirs Adding ancestor directory: {}",
                    display_path(p)
                );
                Some(p.to_path_buf())
            }
        })
        .collect())
}

fn is_empty_dir(path: &Path) -> Result<bool> {
    path.read_dir()
        .map(|mut i| i.next().is_none())
        .wrap_err_with(|| format!("failed to read_dir: {}", display_path(path)))
}

fn is_empty_dir_ignore(path: &Path, ignore_files: Vec<&str>) -> Result<bool> {
    path.read_dir()
        .map(|mut i| {
            i.all(|entry| match entry {
                Ok(entry) => ignore_files.iter().any(|ignore_file| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(ignore_file)
                }),
                Err(_) => false,
            })
        })
        .wrap_err_with(|| format!("failed to read_dir: {}", display_path(path)))
}

pub struct FindUp {
    current_dir: PathBuf,
    current_dir_filenames: Vec<String>,
    filenames: Vec<String>,
}

impl FindUp {
    pub fn new(from: &Path, filenames: &[String]) -> Self {
        let filenames: Vec<String> = filenames.iter().map(|s| s.to_string()).collect();
        Self {
            current_dir: from.to_path_buf(),
            filenames: filenames.clone(),
            current_dir_filenames: filenames,
        }
    }
}

impl Iterator for FindUp {
    type Item = PathBuf;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(filename) = self.current_dir_filenames.pop() {
            let path = self.current_dir.join(filename);
            if path.is_file() {
                return Some(path);
            }
        }
        self.current_dir_filenames.clone_from(&self.filenames);
        if crate::testing::active() && self.current_dir == *dirs::HOME {
            return None; // in tests, do not recurse further than ./test
        }
        if !self.current_dir.pop() {
            return None;
        }
        self.next()
    }
}

/// returns the first executable in PATH
/// will not include mise bin paths or other paths added by mise
pub fn which<P: AsRef<Path>>(name: P) -> Option<PathBuf> {
    static CACHE: Lazy<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = Lazy::new(Default::default);

    let name = name.as_ref();
    if let Some(path) = CACHE.lock().unwrap().get(name) {
        return path.clone();
    }
    let path = _which(name, &env::PATH);
    CACHE
        .lock()
        .unwrap()
        .insert(name.to_path_buf(), path.clone());
    path
}

/// Returns the first directly spawnable executable in PATH, expanding configured
/// executable extensions on Windows when `name` has no extension.
pub fn which_spawnable(name: &str) -> Option<PathBuf> {
    _which_spawnable(name, &env::PATH)
}

fn _which_spawnable(name: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    let names = executable_names(name);
    paths.iter().find_map(|dir| {
        names
            .iter()
            .map(|name| dir.join(name))
            .find(|candidate| is_spawnable(candidate))
    })
}

/// returns the first executable in PATH
/// will include mise bin paths or other paths added by mise
pub fn which_non_pristine<P: AsRef<Path>>(name: P) -> Option<PathBuf> {
    _which(name, &env::PATH_NON_PRISTINE)
}

/// Canonicalize a path and cache successful resolutions for the current process.
///
/// Use this for repeated comparisons against stable roots or PATH entries. Failed
/// canonicalizations are not cached because many callers handle paths that may be
/// created later in the same process.
pub fn canonicalize_cached(path: &Path) -> Option<PathBuf> {
    static CACHE: Lazy<Mutex<HashMap<PathBuf, PathBuf>>> = Lazy::new(Default::default);

    if !path.is_absolute() {
        return path.canonicalize().ok();
    }
    if let Some(path) = CACHE.lock().unwrap().get(path).cloned() {
        return Some(path);
    }
    let canonicalized = path.canonicalize().ok()?;
    CACHE
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), canonicalized.clone());
    Some(canonicalized)
}

/// Canonicalize a path using the process cache, falling back to the original
/// path when canonicalization fails.
pub fn canonicalize_or_self(path: &Path) -> PathBuf {
    canonicalize_cached(path).unwrap_or_else(|| path.to_path_buf())
}

/// Returns true if `path` is one of mise's shim directories.
///
/// The configured user and system shim directories qualify. An active shim outside these
/// configured directories is rejected per candidate instead of treating its
/// entire parent directory as shims, since that directory may also contain
/// legitimate executables.
///
/// Uses `paths_eq` + `replace_path` for the fast path (expands `~`,
/// case-insensitive on macOS/Windows), then falls back to `canonicalize_or_self`
/// so symlinked roots (e.g. `/usr/local/share` → `/private/usr/local/share` on
/// macOS) still match — the cached helper keeps this off the filesystem hot path.
pub fn is_mise_shims_dir(path: &Path) -> bool {
    let resolved = replace_path(path);
    let user_shims = dirs::shims();
    let sys_shims = dirs::system_shims();
    if paths_eq(&resolved, &user_shims) || paths_eq(&resolved, &sys_shims) {
        return true;
    }
    let canon_input = canonicalize_or_self(&resolved);
    let canon_user = canonicalize_or_self(&user_shims);
    let canon_sys = canonicalize_or_self(&sys_shims);
    paths_eq(&canon_input, &canon_user) || paths_eq(&canon_input, &canon_sys)
}

/// Returns true if `path` resolves to the shim that delegated to this mise
/// process. Candidate-level filtering avoids excluding legitimate sibling
/// executables that happen to share a directory with the active shim.
pub fn is_active_mise_shim(path: &Path) -> bool {
    env::MISE_SHIM_PATH
        .read()
        .unwrap()
        .as_ref()
        .is_some_and(|active| paths_eq(&canonicalize_or_self(path), &canonicalize_or_self(active)))
}

/// Build a PATH value with mise shims filtered out, suitable for passing to
/// subprocesses via `.env("PATH", ...)`. Prevents infinite recursion when a
/// subprocess (e.g. `gh auth token`, `git credential fill`) resolves to a
/// mise shim that re-enters mise.
///
/// Uses the current process's PATH (`PATH_NON_PRISTINE`). For stripping
/// shims from an arbitrary PATH string (e.g. from `PRISTINE_ENV`), use
/// `strip_shims_from_path` instead.
pub fn path_env_without_shims() -> std::ffi::OsString {
    let filtered: Vec<_> = env::PATH_NON_PRISTINE
        .iter()
        .filter(|p| !is_mise_dispatch_dir(p))
        .cloned()
        .collect();
    std::env::join_paths(filtered)
        .unwrap_or_else(|_| std::env::var_os(&*env::PATH_KEY).unwrap_or_default())
}

/// Strip mise shims from an arbitrary PATH string. Use this when the
/// subprocess receives a custom env map (e.g. `PRISTINE_ENV`) rather
/// than inheriting the current process's PATH.
pub fn strip_shims_from_path(path_val: &str) -> String {
    let filtered = env::split_paths(path_val).filter(|p| !is_mise_dispatch_dir(p));
    std::env::join_paths(filtered)
        .unwrap_or_else(|_| std::ffi::OsString::from(path_val))
        .to_string_lossy()
        .into_owned()
}

/// Strip every mise dispatch directory from PATH before a command wrapper
/// delegates. This lets the wrapped command resolve a tool managed by mise or
/// fall through to rustup/the system without invoking the wrapper again.
pub fn strip_dispatch_dirs_from_path(path_val: &str) -> String {
    let filtered = env::split_paths(path_val).filter(|p| !is_mise_dispatch_dir(p));
    std::env::join_paths(filtered)
        .unwrap_or_else(|_| std::ffi::OsString::from(path_val))
        .to_string_lossy()
        .into_owned()
}

pub fn is_command_wrapper_dir(path: &Path) -> bool {
    let resolved = replace_path(path);
    paths_eq(&resolved, &dirs::COMMAND_WRAPPERS)
        || paths_eq(
            &canonicalize_or_self(&resolved),
            &canonicalize_or_self(&dirs::COMMAND_WRAPPERS),
        )
}

pub fn is_mise_dispatch_dir(path: &Path) -> bool {
    is_mise_shims_dir(path) || is_command_wrapper_dir(path)
}

/// returns the first executable in PATH, excluding the mise shim directories
/// use this for internal tool lookups to avoid recursive shim invocations
/// (shims call `mise exec`, which would re-enter the same code path)
pub fn which_no_shims<P: AsRef<Path>>(name: P) -> Option<PathBuf> {
    let paths: Vec<PathBuf> = env::PATH_NON_PRISTINE
        .iter()
        .filter(|p| !is_mise_dispatch_dir(p))
        .cloned()
        .collect();
    _which(name, &paths)
}

fn _which<P: AsRef<Path>>(name: P, paths: &[PathBuf]) -> Option<PathBuf> {
    let name = name.as_ref();
    paths.iter().find_map(|path| {
        let bin = path.join(name);
        if is_executable(&bin) { Some(bin) } else { None }
    })
}

#[cfg(not(windows))]
pub fn executable_names(bin: &str) -> Vec<String> {
    vec![bin.to_string()]
}

#[cfg(windows)]
pub fn executable_names(bin: &str) -> Vec<String> {
    let mut names = vec![bin.to_string()];
    if Path::new(bin).extension().is_none() {
        for ext in &Settings::get().windows_executable_extensions {
            let name = if ext.is_empty() {
                bin.to_string()
            } else {
                format!("{bin}.{ext}")
            };
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

pub fn un_gz(input: &Path, dest: &Path) -> Result<()> {
    debug!("gunzip {} > {}", input.display(), dest.display());
    let f = File::open(input)?;
    let mut dec = GzDecoder::new(f);
    let mut output = File::create(dest)?;
    std::io::copy(&mut dec, &mut output)
        .wrap_err_with(|| format!("failed to un-gzip: {}", display_path(input)))?;
    Ok(())
}

pub fn un_xz(input: &Path, dest: &Path) -> Result<()> {
    debug!("xz -d {} -c > {}", input.display(), dest.display());
    let f = File::open(input)?;
    let mut dec = xz2::read::XzDecoder::new(f);
    let mut output = File::create(dest)?;
    std::io::copy(&mut dec, &mut output)
        .wrap_err_with(|| format!("failed to un-xz: {}", display_path(input)))?;
    Ok(())
}

/// The largest zstd window mise decodes: 2^30 (1 GiB). libzstd refuses frames
/// with a window above 2^27 (128 MiB) unless the caller raises the limit, the
/// same limit the `zstd` CLI lifts with `--long`. Large release archives such
/// as LLVM's are compressed with a 1 GiB window. Going no higher bounds the
/// memory an archive can make the decoder allocate, and 2^30 is also the most
/// libzstd supports on 32-bit platforms.
const ZSTD_WINDOW_LOG_MAX: u32 = 30;

fn zstd_decoder<R: Read>(reader: R) -> Result<zstd::Decoder<'static, BufReader<R>>> {
    let mut dec = zstd::Decoder::new(reader)?;
    dec.window_log_max(ZSTD_WINDOW_LOG_MAX)?;
    Ok(dec)
}

pub fn un_zst(input: &Path, dest: &Path) -> Result<()> {
    debug!("zstd -d {} -c > {}", input.display(), dest.display());
    let f = File::open(input)?;
    let mut dec = zstd_decoder(f)?;
    let mut output = File::create(dest)?;
    std::io::copy(&mut dec, &mut output)
        .wrap_err_with(|| format!("failed to un-zst: {}", display_path(input)))?;
    Ok(())
}

pub fn un_bz2(input: &Path, dest: &Path) -> Result<()> {
    debug!("bzip2 -d {} -c > {}", input.display(), dest.display());
    let f = File::open(input)?;
    let mut dec = BzDecoder::new(f);
    let mut output = File::create(dest)?;
    std::io::copy(&mut dec, &mut output)
        .wrap_err_with(|| format!("failed to un-bz2: {}", display_path(input)))?;
    Ok(())
}

/// Run long blocking work (archive extraction, subprocess waits) without tying up a tokio worker.
///
/// Extraction can take seconds and is called directly from async install paths, where it would
/// otherwise block a runtime worker thread for the duration. On mise's multi-threaded runtime
/// (see `main.rs`), `tokio::task::block_in_place` hands the worker's core off to another thread so
/// concurrent tasks (progress bars, downloads, other installs) keep running. Outside a runtime, or
/// on a current-thread runtime (e.g. `#[tokio::test]`), `block_in_place` would panic, so fall back
/// to running the closure inline.
pub fn run_blocking<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

pub fn decompress_file(input: &Path, dest: &Path, format: ExtractionFormat) -> Result<()> {
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent)?;
    }

    run_blocking(|| match format {
        ExtractionFormat::Gz => un_gz(input, dest),
        ExtractionFormat::Xz => un_xz(input, dest),
        ExtractionFormat::Zst => un_zst(input, dest),
        ExtractionFormat::Bz2 => un_bz2(input, dest),
        ExtractionFormat::Br | ExtractionFormat::Lz4 | ExtractionFormat::Sz => {
            bail!("{format} format not supported")
        }
        _ => bail!("unsupported compressed file format: {}", format),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, strum::EnumString, strum::Display)]
pub enum ExtractionFormat {
    #[strum(to_string = "tar.gz", serialize = "tgz")]
    TarGz,
    #[strum(serialize = "gz")]
    Gz,
    #[strum(to_string = "tar.xz", serialize = "txz")]
    TarXz,
    #[strum(serialize = "xz")]
    Xz,
    #[strum(to_string = "tar.bz2", serialize = "tbz2", serialize = "tbz")]
    TarBz2,
    #[strum(serialize = "bz2")]
    Bz2,
    #[strum(to_string = "tar.zst", serialize = "tzst")]
    TarZst,
    #[strum(serialize = "zst")]
    Zst,
    #[strum(serialize = "tar")]
    Tar,
    #[strum(to_string = "zip", serialize = "vsix")]
    Zip,
    #[strum(serialize = "7z")]
    SevenZip,
    #[strum(to_string = "tar.br", serialize = "tbr")]
    TarBr,
    #[strum(serialize = "br")]
    Br,
    #[strum(to_string = "tar.lz4", serialize = "tlz4")]
    TarLz4,
    #[strum(serialize = "lz4")]
    Lz4,
    #[strum(to_string = "tar.sz", serialize = "tsz")]
    TarSz,
    #[strum(serialize = "sz")]
    Sz,
    #[strum(serialize = "rar")]
    Rar,
    #[strum(serialize = "raw")]
    Raw,
}

impl ExtractionFormat {
    pub fn from_file_name(filename: &str) -> Self {
        let filename = filename.to_lowercase();

        if let Some(idx) = filename.rfind(".tar.") {
            let ext = &filename[idx + 1..];
            if let Some(fmt) = Self::from_ext(ext) {
                return fmt;
            }
        }

        if let Some(ext) = Path::new(&filename).extension().and_then(|s| s.to_str()) {
            Self::from_ext(ext).unwrap_or(ExtractionFormat::Raw)
        } else {
            ExtractionFormat::Raw
        }
    }

    pub fn from_ext(ext: &str) -> Option<Self> {
        ext.to_lowercase().parse().ok()
    }

    /// The format a download's name implies, falling back to its leading
    /// bytes when the name carries no known extension — as brew does, since
    /// URLs such as `codeload.github.com/<owner>/<repo>/tar.gz/refs/tags/<tag>`
    /// end without one.
    pub fn detect(path: &Path, filename: &str) -> Result<Self> {
        match Self::from_file_name(filename) {
            ExtractionFormat::Raw => Ok(Self::from_magic(path)?.unwrap_or(ExtractionFormat::Raw)),
            format => Ok(format),
        }
    }

    /// Identify an archive by its content. A compressed stream only counts as
    /// a tarball when its payload opens with a ustar header, so a bare
    /// compressed file or a binary is never mistaken for an archive.
    pub fn from_magic(path: &Path) -> Result<Option<Self>> {
        let mut magic = [0; 6];
        let len = File::open(path)?.read(&mut magic)?;
        let magic = &magic[..len];
        if magic.starts_with(b"PK\x03\x04") {
            return Ok(Some(ExtractionFormat::Zip));
        }
        let format = if magic.starts_with(&[0x1f, 0x8b]) {
            ExtractionFormat::TarGz
        } else if magic.starts_with(b"\xfd7zXZ\0") {
            ExtractionFormat::TarXz
        } else if magic.starts_with(b"BZh") {
            ExtractionFormat::TarBz2
        } else if magic.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            ExtractionFormat::TarZst
        } else {
            ExtractionFormat::Tar
        };
        let mut header = Vec::with_capacity(512);
        // a stream that fails to decode is not an archive we can unpack
        let decoded = open_tar(format, path)?.take(512).read_to_end(&mut header);
        let is_tar = decoded.is_ok() && header.get(257..262) == Some(&b"ustar"[..]);
        Ok(is_tar.then_some(format))
    }

    pub fn is_archive(&self) -> bool {
        self.is_tar_archive()
            || matches!(
                self,
                ExtractionFormat::Zip | ExtractionFormat::SevenZip | ExtractionFormat::Rar
            )
    }

    pub fn is_tar_archive(&self) -> bool {
        matches!(
            self,
            ExtractionFormat::TarGz
                | ExtractionFormat::TarXz
                | ExtractionFormat::TarBz2
                | ExtractionFormat::TarZst
                | ExtractionFormat::Tar
                | ExtractionFormat::TarBr
                | ExtractionFormat::TarLz4
                | ExtractionFormat::TarSz
        )
    }

    pub fn is_compressed_file(&self) -> bool {
        matches!(
            self,
            ExtractionFormat::Gz
                | ExtractionFormat::Xz
                | ExtractionFormat::Bz2
                | ExtractionFormat::Zst
                | ExtractionFormat::Br
                | ExtractionFormat::Lz4
                | ExtractionFormat::Sz
        )
    }

    pub fn extension(&self) -> Option<String> {
        (*self != ExtractionFormat::Raw).then(|| self.to_string())
    }
}

pub struct ExtractOptions<'a> {
    pub strip_components: usize,
    pub pr: Option<&'a dyn SingleReport>,
    /// When false, files will be extracted with current timestamp instead of archive's mtime
    pub preserve_mtime: bool,
}

impl<'a> Default for ExtractOptions<'a> {
    fn default() -> Self {
        Self {
            strip_components: 0,
            pr: None,
            preserve_mtime: true,
        }
    }
}

pub fn extract_archive(
    archive: &Path,
    dest: &Path,
    format: ExtractionFormat,
    opts: &ExtractOptions,
) -> Result<()> {
    match format {
        ExtractionFormat::TarGz
        | ExtractionFormat::TarXz
        | ExtractionFormat::TarBz2
        | ExtractionFormat::TarZst
        | ExtractionFormat::Tar
        | ExtractionFormat::TarBr
        | ExtractionFormat::TarLz4
        | ExtractionFormat::TarSz
        | ExtractionFormat::Raw => untar(archive, dest, format, opts),
        ExtractionFormat::Zip => unzip(archive, dest, opts),
        ExtractionFormat::SevenZip => un7z(archive, dest, opts),
        ExtractionFormat::Gz
        | ExtractionFormat::Xz
        | ExtractionFormat::Bz2
        | ExtractionFormat::Zst
        | ExtractionFormat::Br
        | ExtractionFormat::Lz4
        | ExtractionFormat::Sz => {
            bail!("extract_archive does not support compressed single-file format: {format}")
        }
        ExtractionFormat::Rar => bail!("rar format not supported"),
    }
}

pub fn untar(
    archive: &Path,
    dest: &Path,
    format: ExtractionFormat,
    opts: &ExtractOptions,
) -> Result<()> {
    if !format.is_tar_archive() && format != ExtractionFormat::Raw {
        bail!("untar only supports tar formats, got {}", format);
    }

    debug!("tar -xf {} -C {}", archive.display(), dest.display());
    if let Some(pr) = &opts.pr {
        pr.set_message(format!(
            "extract {}",
            archive.file_name().unwrap().to_string_lossy()
        ));
    }

    let err = || {
        let archive = display_path(archive);
        let dest = display_path(dest);
        format!("failed to extract tar: {archive} to {dest}")
    };

    run_blocking(|| {
        let tar = open_tar(format, archive)?;
        create_dir_all(dest).wrap_err_with(err)?;
        let mut unpack_opts = UnpackOptions::default();
        unpack_opts.preserve_mtime = opts.preserve_mtime;
        unpack_opts.on_entry = Some(Box::new(|entry| {
            trace!("extracting {}", entry.path.display());
        }));
        let summary = Archive::new(tar)
            .unpack(dest, &mut unpack_opts)
            .wrap_err_with(err)?;
        debug!("tar extraction summary: {summary:?}");
        strip_archive_path_components(dest, opts.strip_components).wrap_err_with(|| {
            format!(
                "failed to strip path components from tar archive: {}",
                display_path(archive)
            )
        })
    })
}

fn open_tar(format: ExtractionFormat, archive: &Path) -> Result<Box<dyn std::io::Read>> {
    let f = File::open(archive)?;
    Ok(match format {
        // TODO: we probably shouldn't assume raw is tar.gz, but this was to retain existing behavior
        ExtractionFormat::TarGz | ExtractionFormat::Raw => Box::new(GzDecoder::new(f)),
        ExtractionFormat::TarXz => Box::new(xz2::read::XzDecoder::new(f)),
        ExtractionFormat::TarBz2 => Box::new(BzDecoder::new(f)),
        ExtractionFormat::TarZst => Box::new(zstd_decoder(f)?),
        ExtractionFormat::Tar => Box::new(f),
        ExtractionFormat::TarBr | ExtractionFormat::TarLz4 | ExtractionFormat::TarSz => {
            bail!("{format} format not supported")
        }
        ExtractionFormat::Gz
        | ExtractionFormat::Xz
        | ExtractionFormat::Bz2
        | ExtractionFormat::Zst
        | ExtractionFormat::Br
        | ExtractionFormat::Lz4
        | ExtractionFormat::Sz => {
            bail!("{} is not a tar archive", format)
        }
        ExtractionFormat::Zip => bail!("zip format not supported"),
        ExtractionFormat::SevenZip => bail!("7z format not supported"),
        ExtractionFormat::Rar => bail!("rar format not supported"),
    })
}

fn reset_dir_mtime_to_now(dir: &Path) -> Result<()> {
    let now = FileTime::now();
    for entry in WalkDir::new(dir) {
        let entry = entry?;
        if entry.file_type().is_file() {
            set_file_times(entry.path(), now, now)?;
        }
    }
    Ok(())
}

fn strip_archive_path_components(dir: &Path, strip_depth: usize) -> Result<()> {
    if strip_depth == 0 {
        return Ok(());
    }
    if strip_depth > 1 {
        bail!("strip-components > 1 is not supported");
    }

    let top_level_paths = ls(dir)?;

    for path in top_level_paths {
        if !path.symlink_metadata()?.is_dir() {
            continue;
        }

        // rename the directory to a temp name to avoid conflicts when moving files
        let temp_path = path.with_file_name(format!(
            "{}_tmp_strip",
            path.file_name().unwrap().to_string_lossy()
        ));
        do_rename(&path, &temp_path)?;

        for entry in ls(&temp_path)? {
            if let Some(file_name) = entry.file_name() {
                let dest_path = dir.join(file_name);
                do_rename(&entry, &dest_path)?;
            } else {
                continue;
            }
        }

        remove_dir(temp_path)?;
    }
    Ok(())
}

pub fn unzip(archive: &Path, dest: &Path, opts: &ExtractOptions<'_>) -> Result<()> {
    // TODO: show progress
    debug!("unzip {} -d {}", archive.display(), dest.display());
    if let Some(pr) = &opts.pr {
        pr.set_message(format!(
            "extract {}",
            archive.file_name().unwrap().to_string_lossy()
        ));
    }
    run_blocking(|| {
        ZipArchive::new(File::open(archive)?)
            .wrap_err_with(|| format!("failed to open zip archive: {}", display_path(archive)))?
            .extract(dest)
            .wrap_err_with(|| {
                format!("failed to extract zip archive: {}", display_path(archive))
            })?;

        if !opts.preserve_mtime {
            reset_dir_mtime_to_now(dest)?;
        }

        strip_archive_path_components(dest, opts.strip_components).wrap_err_with(|| {
            format!(
                "failed to strip path components from zip archive: {}",
                display_path(archive)
            )
        })
    })
}

/// Volume metadata that macOS and disk-image tools leave at a DMG's root. It is
/// never part of an artifact, and some of it cannot be read: `.Trashes` ships
/// with mode 0333 in some images. This is Homebrew's `DMG_METADATA` list.
const DMG_VOLUME_METADATA: &[&str] = &[
    ".background",
    ".com.apple.timemachine.donotpresent",
    ".com.apple.timemachine.supported",
    ".DocumentRevisions-V100",
    ".DS_Store",
    ".fseventsd",
    ".MobileBackups",
    ".Spotlight-V100",
    ".TemporaryItems",
    ".Trashes",
    ".VolumeIcon.icns",
    ".HFS+ Private Directory Data\r",
    ".HFS+ Private Data\r",
];

fn copy_dmg_volume(volume: &Path, dest: &Path) -> Result<()> {
    copy_dir_all_preserve_symlinks_skipping(volume, dest, DMG_VOLUME_METADATA)
}

pub fn un_dmg(archive: &Path, dest: &Path) -> Result<()> {
    debug!(
        "hdiutil attach -quiet -nobrowse -mountpoint {} {}",
        dest.display(),
        archive.display()
    );
    run_blocking(|| {
        let tmp = tempfile::TempDir::new()?;
        cmd!(
            "hdiutil",
            "attach",
            "-quiet",
            "-nobrowse",
            "-mountpoint",
            tmp.path(),
            archive.to_path_buf()
        )
        // Display licenses without an interactive pager before accepting them.
        .env("PAGER", "cat")
        // DMGs can require license acceptance even with -quiet. Supply one
        // answer directly so unattended installs do not wait for terminal input.
        .stdin_bytes("Y\n")
        .run()?;
        let copy_result = copy_dmg_volume(tmp.path(), dest);
        let detach_result = cmd!("hdiutil", "detach", tmp.path()).run();
        match (copy_result, detach_result) {
            (Err(copy_err), Err(detach_err)) => Err(copy_err)
                .wrap_err_with(|| format!("additionally failed to detach DMG: {detach_err}")),
            (Err(copy_err), _) => Err(copy_err),
            (Ok(()), Err(detach_err)) => Err(detach_err.into()),
            (Ok(()), Ok(_)) => Ok(()),
        }
    })
}

pub fn un_pkg(archive: &Path, dest: &Path) -> Result<()> {
    debug!(
        "pkgutil --expand-full {} {}",
        archive.display(),
        dest.display()
    );
    run_blocking(|| cmd!("pkgutil", "--expand-full", archive, dest).run())?;
    Ok(())
}

pub fn un7z(archive: &Path, dest: &Path, opts: &ExtractOptions<'_>) -> Result<()> {
    if let Some(pr) = &opts.pr {
        pr.set_message(format!(
            "extract {}",
            archive.file_name().unwrap().to_string_lossy()
        ));
    }
    run_blocking(|| {
        sevenz_rust2::decompress_file_with_extract_fn(archive, dest, |entry, reader, _| {
            let dest_path = dest.join(
                sanitize_7z_entry_path(entry.name())
                    .map_err(|err| sevenz_rust2::Error::Other(format!("{err:#}").into()))?,
            );
            sevenz_rust2::default_entry_extract_fn(entry, reader, &dest_path)
        })
        .wrap_err_with(|| format!("failed to extract 7z archive: {}", display_path(archive)))?;

        if !opts.preserve_mtime {
            reset_dir_mtime_to_now(dest)?;
        }

        strip_archive_path_components(dest, opts.strip_components).wrap_err_with(|| {
            format!(
                "failed to strip path components from 7z archive: {}",
                display_path(archive)
            )
        })
    })
}

/// Whether `name` is a plain file name: exactly one normal path component, with
/// no separators, parent/root components, or drive prefixes on any platform.
/// Use to validate user-supplied names (e.g. `bin`, `rename_exe`, `filter_bins`
/// tool options) before joining them onto a directory, so a value like
/// `../evil` or `/abs/path` cannot escape it.
pub fn is_plain_file_name(name: &str) -> bool {
    // Reject both separators explicitly: `\` is a legal file-name character on
    // Unix, but these names come from cross-platform config.
    if name.contains('/') || name.contains('\\') {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}

/// Whether `path` is a non-empty relative path containing only normal
/// components. Both slash styles are treated as separators so config values are
/// validated consistently across platforms.
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let normalized = path.replace('\\', "/");
    let bytes = normalized.as_bytes();
    if normalized.starts_with('/')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    {
        return false;
    }
    let mut components = Path::new(&normalized).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn sanitize_7z_entry_path(path: &str) -> Result<PathBuf> {
    let normalized = PathBuf::from(path.replace('\\', "/"));
    let mut safe_path = PathBuf::new();

    for component in normalized.components() {
        match component {
            std::path::Component::Normal(part) => safe_path.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                bail!("7z archive entry path escapes extraction directory: {path}")
            }
        }
    }

    Ok(safe_path)
}

pub fn split_file_name(path: &Path) -> (String, String) {
    let file_name = path.file_name().unwrap().to_string_lossy();
    let (file_name_base, ext) = file_name
        .split_once('.')
        .unwrap_or((file_name.as_ref(), ""));
    (file_name_base.to_string(), ext.to_string())
}

pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b || desymlink_path(a) == desymlink_path(b)
}

/// Returns whether `path` starts with `prefix` either lexically or after
/// resolving symlinks and the existing path prefix.
pub fn path_starts_with_resolved(path: &Path, prefix: &Path) -> bool {
    path.starts_with(prefix) || desymlink_path(path).starts_with(desymlink_path(prefix))
}

fn resolve_path_with_existing_prefix(path: &Path) -> PathBuf {
    let mut resolved = if path.is_relative() {
        env::current_dir().unwrap_or_default()
    } else {
        PathBuf::new()
    };
    let mut components = path.components();
    while let Some(component) = components.next() {
        if matches!(component, std::path::Component::CurDir) {
            continue;
        }
        #[cfg(windows)]
        if matches!(
            component,
            std::path::Component::Prefix(_) | std::path::Component::RootDir
        ) {
            // A drive prefix such as `C:` is drive-relative until its root is
            // present, so do not canonicalize the two components separately.
            resolved.push(component.as_os_str());
            continue;
        }
        let candidate = resolved.join(component.as_os_str());
        match candidate.canonicalize() {
            Ok(candidate) => resolved = candidate,
            Err(_) => {
                #[cfg(windows)]
                {
                    // PathBuf::push lexically resolves `..` after a verbatim
                    // (`\\?\`) prefix, but the unresolved suffix must remain
                    // opaque so two potentially different files stay distinct.
                    let mut raw = resolved.into_os_string();
                    if Path::new(&raw).file_name().is_some() {
                        raw.push("\\");
                    }
                    raw.push(component.as_os_str());
                    for component in components {
                        raw.push("\\");
                        raw.push(component.as_os_str());
                    }
                    resolved = raw.into();
                }
                #[cfg(not(windows))]
                {
                    resolved.push(component.as_os_str());
                    resolved.extend(components.map(|component| component.as_os_str()));
                }
                break;
            }
        }
    }
    resolved
}

pub fn desymlink_path(p: &Path) -> PathBuf {
    if p.is_symlink()
        && let Ok(target) = fs::read_link(p)
    {
        let target = if target.is_absolute() {
            target
        } else {
            p.parent().unwrap_or_else(|| Path::new("")).join(target)
        };
        return target
            .canonicalize()
            .unwrap_or_else(|_| resolve_path_with_existing_prefix(&target));
    }
    p.canonicalize()
        .unwrap_or_else(|_| resolve_path_with_existing_prefix(p))
}

/// Resolutions made by [`desymlink_path_cached`], and how many times
/// [`clear_desymlink_cache`] has run. A lookup caches its result only if no
/// clear happened while it was resolving, so a resolution made before a clear
/// cannot be stored after it.
static DESYMLINKED: Lazy<Mutex<(u64, HashMap<PathBuf, PathBuf>)>> = Lazy::new(Default::default);

/// [`desymlink_path`] with resolutions of existing absolute paths cached until
/// [`clear_desymlink_cache`]. A path that does not exist yet is resolved on
/// every call, since it may be created later in the process.
pub fn desymlink_path_cached(p: &Path) -> PathBuf {
    if !p.is_absolute() {
        return desymlink_path(p);
    }
    let generation = {
        let cache = DESYMLINKED.lock().unwrap();
        if let Some(resolved) = cache.1.get(p) {
            return resolved.clone();
        }
        cache.0
    };
    let resolved = desymlink_path(p);
    if p.exists() {
        let mut cache = DESYMLINKED.lock().unwrap();
        if cache.0 == generation {
            cache.1.insert(p.to_path_buf(), resolved.clone());
        }
    }
    resolved
}

/// Forget every resolution [`desymlink_path_cached`] has made, so the next call
/// follows symlinks as they are now. Config reloads call this: a symlink
/// retargeted since the last load must not keep its old destination.
pub fn clear_desymlink_cache() {
    let mut cache = DESYMLINKED.lock().unwrap();
    cache.0 += 1;
    cache.1.clear();
}

pub fn clone_dir(from: &PathBuf, to: &PathBuf) -> Result<()> {
    if cfg!(target_os = "macos") {
        cmd!("/bin/cp", "-cR", from, to).run()?;
    } else if cfg!(windows) {
        cmd!("robocopy", from, to, "/MIR").run()?;
    } else {
        cmd!("cp", "--reflink=auto", "-r", from, to).run()?;
    }
    Ok(())
}

/// Inspects the top-level contents of a tar archive without extracting it
/// Skips leading CurDir (".") components from a path's components iterator.
/// Archives often have paths like "./foo/bar" where the leading "." should be ignored.
fn skip_curdir_components(path: &Path) -> impl Iterator<Item = std::path::Component<'_>> {
    path.components()
        .skip_while(|c| matches!(c, std::path::Component::CurDir))
}

pub fn inspect_tar_contents(
    archive: &Path,
    format: ExtractionFormat,
) -> Result<Vec<(String, bool)>> {
    let tar = open_tar(format, archive)?;
    let mut archive = Archive::new(tar);
    let mut top_level_components = std::collections::HashMap::new();

    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        let entry_type = entry.entry_type();

        // Get the first non-CurDir component of the path (top-level directory/file)
        let mut components = skip_curdir_components(&path);

        if let Some(first_component) = components.next() {
            let name = first_component.as_os_str().to_string_lossy().to_string();

            // Check if this entry indicates the component is a directory
            // It's a directory if the entry type is dir OR if there are more components after the first
            let is_directory = entry_type == EntryType::Directory || components.next().is_some();

            // Update the component's directory status
            // A component is a directory if ANY entry indicates it's a directory
            let existing = top_level_components.entry(name.clone()).or_insert(false);
            *existing = *existing || is_directory;
        }
    }

    Ok(top_level_components.into_iter().collect())
}

/// Inspects the top-level contents of a zip archive without extracting it
pub fn inspect_zip_contents(archive: &Path) -> Result<Vec<(String, bool)>> {
    let f = File::open(archive)?;
    let mut archive = ZipArchive::new(f)
        .wrap_err_with(|| format!("failed to open zip archive: {}", display_path(archive)))?;
    let mut top_level_components = std::collections::HashMap::new();

    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        if let Some(path) = file.enclosed_name() {
            // Get the first non-CurDir component of the path (top-level directory/file)
            let mut components = skip_curdir_components(&path);

            if let Some(first_component) = components.next() {
                let name = first_component.as_os_str().to_string_lossy().to_string();

                // Check if this entry indicates the component is a directory
                // It's a directory if the entry type is dir OR if there are more components after the first
                let is_directory = file.is_dir() || components.next().is_some();

                let existing = top_level_components.entry(name.clone()).or_insert(false);
                *existing = *existing || is_directory;
            }
        }
    }

    Ok(top_level_components.into_iter().collect())
}

/// Adapted from inspect_tar_contents for 7z archives
pub fn inspect_7z_contents(archive: &Path) -> Result<Vec<(String, bool)>> {
    let sevenz = sevenz_rust2::Archive::open(archive)?;
    let mut top_level_components = std::collections::HashMap::new();

    for file in &sevenz.files {
        let path = sanitize_7z_entry_path(file.name())?;

        // Get the first non-CurDir component of the path (top-level directory/file)
        let mut components = skip_curdir_components(&path);

        if let Some(first_component) = components.next() {
            let name = first_component.as_os_str().to_string_lossy().to_string();
            // It's a directory if the entry type is dir OR if there are more components after the first
            let is_directory = file.is_directory() || components.next().is_some();

            let existing = top_level_components.entry(name.clone()).or_insert(false);
            *existing = *existing || is_directory;
        }
    }

    Ok(top_level_components.into_iter().collect())
}

/// Determines if strip_components=1 should be applied based on archive structure
pub fn should_strip_components(archive: &Path, format: ExtractionFormat) -> Result<bool> {
    let top_level_entries = match format {
        ExtractionFormat::Zip => inspect_zip_contents(archive)?,
        ExtractionFormat::SevenZip => inspect_7z_contents(archive)?,
        _ => inspect_tar_contents(archive, format)?,
    };

    // If there's exactly one top-level entry and it's a directory, we should strip it
    if top_level_entries.len() == 1 {
        let (_, is_directory) = &top_level_entries[0];
        Ok(*is_directory)
    } else {
        Ok(false)
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveContent {
    pub name: String,
    pub sha256: String,
}

/// Return the regular files in an archive after applying strip-components.
///
/// This is intentionally stricter than extraction: content-level provenance is
/// only safe when every installed regular file is covered, so ambiguous archive
/// entries (links, unsafe paths, stripped-away file names, unsupported formats)
/// fail closed instead of being ignored.
pub fn archive_content_files(
    archive_path: &Path,
    format: ExtractionFormat,
    strip_components: usize,
) -> Result<Vec<ArchiveContent>> {
    if strip_components > 1 {
        bail!("content-level SLSA verification only supports strip_components values of 0 or 1");
    }

    match format {
        ExtractionFormat::TarGz
        | ExtractionFormat::TarXz
        | ExtractionFormat::TarBz2
        | ExtractionFormat::TarZst
        | ExtractionFormat::Tar
        | ExtractionFormat::TarBr
        | ExtractionFormat::TarLz4
        | ExtractionFormat::TarSz => {
            archive_content_files_tar(archive_path, format, strip_components)
        }
        ExtractionFormat::Zip => archive_content_files_zip(archive_path, strip_components),
        ExtractionFormat::SevenZip => {
            bail!("content-level SLSA verification does not support 7z archives")
        }
        ExtractionFormat::Gz
        | ExtractionFormat::Xz
        | ExtractionFormat::Bz2
        | ExtractionFormat::Zst
        | ExtractionFormat::Br
        | ExtractionFormat::Lz4
        | ExtractionFormat::Sz
        | ExtractionFormat::Raw => {
            bail!("content-level SLSA verification only supports archive formats")
        }
        ExtractionFormat::Rar => bail!("rar format not supported"),
    }
}

fn archive_content_files_tar(
    archive_path: &Path,
    format: ExtractionFormat,
    strip_components: usize,
) -> Result<Vec<ArchiveContent>> {
    let tar = open_tar(format, archive_path)?;
    let mut archive = Archive::new(tar);
    let mut files = Vec::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let entry_type = entry.entry_type();
        if entry_type == EntryType::Directory {
            continue;
        }
        if entry_type != EntryType::File {
            bail!(
                "content-level SLSA verification does not support non-regular archive entry: {}",
                path.display()
            );
        }
        let name = normalize_archive_content_path(&path, strip_components)?;
        let sha256 = sha256_reader(&mut entry)?;
        files.push(ArchiveContent { name, sha256 });
    }

    validate_archive_content_files(files)
}

fn archive_content_files_zip(
    archive_path: &Path,
    strip_components: usize,
) -> Result<Vec<ArchiveContent>> {
    let f = File::open(archive_path)?;
    let mut archive = ZipArchive::new(f)
        .wrap_err_with(|| format!("failed to open zip archive: {}", display_path(archive_path)))?;
    let mut files = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }
        if file.is_symlink() {
            bail!(
                "content-level SLSA verification does not support symlink archive entry: {}",
                file.name()
            );
        }
        let enclosed_name = file.enclosed_name().ok_or_else(|| {
            eyre::eyre!(
                "content-level SLSA verification rejected unsafe zip path: {}",
                file.name()
            )
        })?;
        let name = normalize_archive_content_path(&enclosed_name, strip_components)?;
        let sha256 = sha256_reader(&mut file)?;
        files.push(ArchiveContent { name, sha256 });
    }

    validate_archive_content_files(files)
}

fn sha256_reader(reader: &mut impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn validate_archive_content_files(files: Vec<ArchiveContent>) -> Result<Vec<ArchiveContent>> {
    if files.is_empty() {
        bail!("content-level SLSA verification found no regular files in archive");
    }
    let mut names = std::collections::HashSet::new();
    for file in &files {
        if !names.insert(file.name.clone()) {
            bail!(
                "content-level SLSA verification found duplicate installed archive path: {}",
                file.name
            );
        }
    }
    Ok(files)
}

fn normalize_archive_content_path(path: &Path, strip_components: usize) -> Result<String> {
    let mut parts = Vec::new();
    for component in skip_curdir_components(path) {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                bail!(
                    "content-level SLSA verification rejected unsafe archive path: {}",
                    path.display()
                )
            }
        }
    }
    if strip_components > parts.len() {
        bail!(
            "content-level SLSA verification stripped all components from archive path: {}",
            path.display()
        );
    }
    let parts = &parts[strip_components..];
    if parts.is_empty() {
        bail!(
            "content-level SLSA verification stripped all components from archive path: {}",
            path.display()
        );
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests;
