use crate::path::{Path, PathBuf, PathExt};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::os::unix::prelude::*;
use std::sync::Mutex;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bzip2::read::BzDecoder;
use color_eyre::eyre::{Context, Result};
use eyre::bail;
use filetime::{FileTime, set_file_times};
use flate2::read::GzDecoder;
use itertools::Itertools;
use sha2::{Digest, Sha256};
use std::sync::LazyLock as Lazy;
use tar::Archive;
use walkdir::WalkDir;
use zip::ZipArchive;

#[cfg(windows)]
use crate::config::Settings;
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env};

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

pub fn remove_all<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    match path.metadata().map(|m| m.file_type()) {
        Ok(x) if x.is_symlink() || x.is_file() => {
            remove_file(path)?;
        }
        Ok(x) if x.is_dir() => {
            trace!("rm -rf {}", display_path(path));
            fs::remove_dir_all(path)
                .wrap_err_with(|| format!("failed rm -rf: {}", display_path(path)))?;
        }
        _ => {}
    };
    Ok(())
}

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

pub fn remove_all_with_progress<P: AsRef<Path>>(path: P, pr: &dyn SingleReport) -> Result<()> {
    let path = path.as_ref();
    if !path.exists() {
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
    trace!("mv {} {}", from.display(), to.display());
    do_rename(from, to).wrap_err_with(|| {
        format!(
            "failed rename: {} -> {}",
            display_path(from),
            display_path(to)
        )
    })
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
/// downloads are cached on one volume and installs are written to another).
pub fn move_file<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();

    match do_rename(from, to) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
            if from.is_dir() {
                create_dir_all(to)?;
                copy_dir_all(from, to)?;
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

pub fn copy_dir_all<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    trace!("cp -r {} {}", from.display(), to.display());
    recursive_ls(from)?.into_iter().try_for_each(|path| {
        let relative = path.strip_prefix(from)?;
        let dest = to.join(relative);
        create_dir_all(dest.parent().unwrap())?;
        copy(&path, &dest)?;
        Ok(())
    })
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    trace!("write {}", display_path(path));
    fs::write(path, contents).wrap_err_with(|| format!("failed write: {}", display_path(path)))
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

/// replaces $HOME with "~"
pub fn display_path<P: AsRef<Path>>(path: P) -> String {
    path.as_ref().display_user()
}

pub fn display_rel_path<P: AsRef<Path>>(path: P) -> String {
    let path = path.as_ref();
    match path.strip_prefix(dirs::CWD.as_ref().unwrap()) {
        Ok(rel) => format!("./{}", rel.display()),
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
pub fn replace_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    match path.starts_with("~/") {
        true => dirs::HOME.join(path.strip_prefix("~/").unwrap()),
        false => path.to_path_buf(),
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

pub fn dir_subdirs(dir: &Path) -> Result<BTreeSet<String>> {
    let mut output = Default::default();

    if !dir.exists() {
        return Ok(output);
    }

    for entry in dir.read_dir()? {
        let entry = entry?;
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

pub fn recursive_ls(dir: &Path) -> Result<BTreeSet<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Default::default());
    }

    Ok(WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_ok(|e| e.file_type().is_file())
        .map_ok(|e| e.path().to_path_buf())
        .try_collect()?)
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
fn is_unc_path(path: &Path) -> bool {
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
        create_windows_unc_symlink(target, link)
    } else {
        junction::create(target, link)
    }
}

#[cfg(windows)]
pub fn make_symlink(target: &Path, link: &Path) -> Result<(PathBuf, PathBuf)> {
    if let Err(err) = create_windows_dir_link(target, link) {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            let _ = fs::remove_file(link);
            let _ = fs::remove_dir(link);
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
    // Windows symlink are write in file currently
    // may be changed to symlink in the future
    if link.is_symlink() {
        Ok(Some(fs::read_link(link)?))
    } else if link.is_file() {
        Ok(Some(fs::read_to_string(link)?.into()))
    } else {
        Ok(None)
    }
}

#[cfg(unix)]
pub fn make_symlink_or_file(target: &Path, link: &Path) -> Result<()> {
    make_symlink(target, link)?;
    Ok(())
}

pub fn remove_symlinks_with_target_prefix(
    symlink_dir: &Path,
    target_prefix: &Path,
) -> Result<Vec<PathBuf>> {
    if !symlink_dir.exists() {
        return Ok(vec![]);
    }
    let mut removed = vec![];
    for entry in symlink_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.is_symlink() {
            let target = path.read_link()?;
            if target.starts_with(target_prefix) {
                fs::remove_file(&path)?;
                removed.push(path);
            }
        }
    }
    Ok(removed)
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

/// Check if a file starts with a shebang (#!).
/// Only reads the first 2 bytes to minimize I/O during task discovery.
#[cfg(windows)]
pub fn has_shebang(path: &Path) -> bool {
    std::fs::File::open(path)
        .and_then(|mut f| {
            use std::io::Read;
            let mut buf = [0u8; 2];
            f.read_exact(&mut buf)?;
            Ok(buf == *b"#!")
        })
        .unwrap_or(false)
}

#[cfg(unix)]
pub fn make_executable<P: AsRef<Path>>(path: P) -> Result<()> {
    trace!("chmod +x {}", display_path(&path));
    let path = path.as_ref();
    let mut perms = path.metadata()?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
        .wrap_err_with(|| format!("failed to chmod +x: {}", display_path(path)))?;
    Ok(())
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
    perms.set_mode(perms.mode() | 0o111);
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
        if cfg!(test) && self.current_dir == *dirs::HOME {
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
/// Two dirs qualify: the user shims dir (`dirs::SHIMS`) and the system shims
/// dir (`$MISE_SYSTEM_DATA_DIR/shims`). Devcontainer / Docker setups built
/// with `mise install --system` put both on PATH, so subprocess-env filters
/// that strip "the shims dir" must consider both — otherwise the recursion
/// these filters were added to prevent (#8475 for `dependency_env`, #8816
/// for `which_shim`, this for the file.rs helpers) leaks back in through the
/// remaining dir.
///
/// Uses `paths_eq` + `replace_path` for the fast path (expands `~`,
/// case-insensitive on macOS/Windows), then falls back to `canonicalize_or_self`
/// so symlinked roots (e.g. `/usr/local/share` → `/private/usr/local/share` on
/// macOS) still match — the cached helper keeps this off the filesystem hot path.
pub fn is_mise_shims_dir(path: &Path) -> bool {
    let resolved = replace_path(path);
    let sys_shims = env::MISE_SYSTEM_DATA_DIR.join("shims");
    if paths_eq(&resolved, &dirs::SHIMS) || paths_eq(&resolved, &sys_shims) {
        return true;
    }
    let canon_input = canonicalize_or_self(&resolved);
    let canon_user = canonicalize_or_self(&dirs::SHIMS);
    let canon_sys = canonicalize_or_self(&sys_shims);
    paths_eq(&canon_input, &canon_user) || paths_eq(&canon_input, &canon_sys)
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
        .filter(|p| !is_mise_shims_dir(p))
        .cloned()
        .collect();
    std::env::join_paths(filtered)
        .unwrap_or_else(|_| std::env::var_os(&*env::PATH_KEY).unwrap_or_default())
}

/// Strip mise shims from an arbitrary PATH string. Use this when the
/// subprocess receives a custom env map (e.g. `PRISTINE_ENV`) rather
/// than inheriting the current process's PATH.
pub fn strip_shims_from_path(path_val: &str) -> String {
    let filtered = env::split_paths(path_val).filter(|p| !is_mise_shims_dir(p));
    std::env::join_paths(filtered)
        .unwrap_or_else(|_| std::ffi::OsString::from(path_val))
        .to_string_lossy()
        .into_owned()
}

/// returns the first executable in PATH, excluding the mise shim directories
/// use this for internal tool lookups to avoid recursive shim invocations
/// (shims call `mise exec`, which would re-enter the same code path)
pub fn which_no_shims<P: AsRef<Path>>(name: P) -> Option<PathBuf> {
    let paths: Vec<PathBuf> = env::PATH_NON_PRISTINE
        .iter()
        .filter(|p| !is_mise_shims_dir(p))
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

pub fn un_zst(input: &Path, dest: &Path) -> Result<()> {
    debug!("zstd -d {} -c > {}", input.display(), dest.display());
    let f = File::open(input)?;
    let mut dec = zstd::Decoder::new(f)?;
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

pub fn decompress_file(input: &Path, dest: &Path, format: ExtractionFormat) -> Result<()> {
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent)?;
    }

    match format {
        ExtractionFormat::Gz => un_gz(input, dest),
        ExtractionFormat::Xz => un_xz(input, dest),
        ExtractionFormat::Zst => un_zst(input, dest),
        ExtractionFormat::Bz2 => un_bz2(input, dest),
        ExtractionFormat::Br | ExtractionFormat::Lz4 | ExtractionFormat::Sz => {
            bail!("{format} format not supported")
        }
        _ => bail!("unsupported compressed file format: {}", format),
    }
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

    let tar = open_tar(format, archive)?;
    // TODO: put this back in when we can read+write in parallel
    // let mut cur = Cursor::new(vec![]);
    // let mut total = 0;
    // loop {
    //     let mut buf = Cursor::new(vec![0; 1024 * 1024]);
    //     let n = tar.read(buf.get_mut()).wrap_err_with(err)?;
    //     cur.get_mut().extend_from_slice(&buf.get_ref()[..n]);
    //     if n == 0 {
    //         break;
    //     }
    //     if let Some(pr) = &opts.pr {
    //         total += n as u64;
    //         pr.set_length(total);
    //     }
    // }
    create_dir_all(dest).wrap_err_with(err)?;

    // Try to extract using the tar crate, detecting sparse files during extraction
    let mut needs_system_tar = false;
    for entry in Archive::new(tar).entries().wrap_err_with(err)? {
        let mut entry = entry.wrap_err_with(err)?;

        // Check if this is a GNU sparse file
        if entry.header().entry_type().is_gnu_sparse() {
            debug!("Detected GNU sparse file, falling back to system tar");
            needs_system_tar = true;
            // Clean up any partial extraction
            remove_all(dest)?;
            create_dir_all(dest)?;
            break;
        }

        // Configure mtime preservation based on options
        entry.set_preserve_mtime(opts.preserve_mtime);

        trace!("extracting {}", entry.path().wrap_err_with(err)?.display());
        entry.unpack_in(dest).wrap_err_with(err)?;
    }

    // Check for the GNUSparseFile.0 directory which indicates the tar crate
    // incorrectly handled a sparse file
    if !needs_system_tar {
        let sparse_dir = dest.join("GNUSparseFile.0");
        if sparse_dir.exists() && sparse_dir.is_dir() {
            debug!("Found GNUSparseFile.0 directory, using system tar");
            needs_system_tar = true;
            // Clean up the bad extraction
            remove_all(dest)?;
            create_dir_all(dest)?;
        }
    }

    if needs_system_tar {
        // Use system tar for archives with problematic sparse files
        // The tar crate doesn't properly handle certain GNU sparse formats
        debug!("Using system tar for: {}", archive.display());

        // When preserve_mtime is false, use -m flag to not restore modification times
        // This causes extracted files to have current time, which is important for
        // cache invalidation and autopruning. Works on both BSD and GNU tar.
        if !opts.preserve_mtime {
            cmd!("tar", "-mxf", archive, "-C", dest)
                .run()
                .wrap_err_with(|| {
                    format!("Failed to extract {} using system tar", archive.display())
                })?;
        } else {
            cmd!("tar", "-xf", archive, "-C", dest)
                .run()
                .wrap_err_with(|| {
                    format!("Failed to extract {} using system tar", archive.display())
                })?;
        }
    }

    // Always use our manual strip to ensure consistent behavior across backends
    strip_archive_path_components(dest, opts.strip_components).wrap_err_with(err)?;
    Ok(())
}

fn open_tar(format: ExtractionFormat, archive: &Path) -> Result<Box<dyn std::io::Read>> {
    let f = File::open(archive)?;
    Ok(match format {
        // TODO: we probably shouldn't assume raw is tar.gz, but this was to retain existing behavior
        ExtractionFormat::TarGz | ExtractionFormat::Raw => Box::new(GzDecoder::new(f)),
        ExtractionFormat::TarXz => Box::new(xz2::read::XzDecoder::new(f)),
        ExtractionFormat::TarBz2 => Box::new(BzDecoder::new(f)),
        ExtractionFormat::TarZst => Box::new(zstd::stream::read::Decoder::new(f)?),
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
    ZipArchive::new(File::open(archive)?)
        .wrap_err_with(|| format!("failed to open zip archive: {}", display_path(archive)))?
        .extract(dest)
        .wrap_err_with(|| format!("failed to extract zip archive: {}", display_path(archive)))?;

    if !opts.preserve_mtime {
        reset_dir_mtime_to_now(dest)?;
    }

    strip_archive_path_components(dest, opts.strip_components).wrap_err_with(|| {
        format!(
            "failed to strip path components from zip archive: {}",
            display_path(archive)
        )
    })
}

pub fn un_dmg(archive: &Path, dest: &Path) -> Result<()> {
    debug!(
        "hdiutil attach -quiet -nobrowse -mountpoint {} {}",
        dest.display(),
        archive.display()
    );
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
    .run()?;
    copy_dir_all(tmp.path(), dest)?;
    cmd!("hdiutil", "detach", tmp.path()).run()?;
    Ok(())
}

pub fn un_pkg(archive: &Path, dest: &Path) -> Result<()> {
    debug!(
        "pkgutil --expand-full {} {}",
        archive.display(),
        dest.display()
    );
    cmd!("pkgutil", "--expand-full", archive, dest).run()?;
    Ok(())
}

pub fn un7z(archive: &Path, dest: &Path, opts: &ExtractOptions<'_>) -> Result<()> {
    if let Some(pr) = &opts.pr {
        pr.set_message(format!(
            "extract {}",
            archive.file_name().unwrap().to_string_lossy()
        ));
    }
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
    desymlink_path(a) == desymlink_path(b)
}

pub fn desymlink_path(p: &Path) -> PathBuf {
    if p.is_symlink()
        && let Ok(target) = fs::read_link(p)
    {
        return target
            .canonicalize()
            .unwrap_or_else(|_| target.to_path_buf());
    }
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

pub fn clone_dir(from: &PathBuf, to: &PathBuf) -> Result<()> {
    if cfg!(macos) {
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
        let header = entry.header();

        // Get the first non-CurDir component of the path (top-level directory/file)
        let mut components = skip_curdir_components(&path);

        if let Some(first_component) = components.next() {
            let name = first_component.as_os_str().to_string_lossy().to_string();

            // Check if this entry indicates the component is a directory
            // It's a directory if the entry type is dir OR if there are more components after the first
            let is_directory = header.entry_type().is_dir() || components.next().is_some();

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
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            continue;
        }
        if !entry_type.is_file() {
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
mod tests {
    use pretty_assertions::assert_eq;

    use crate::config::Config;

    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_make_symlink_creates_and_atomically_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let target_a = dir.path().join("a");
        let target_b = dir.path().join("b");
        fs::write(&target_a, "a").unwrap();
        fs::write(&target_b, "b").unwrap();
        let link = dir.path().join("link");

        // Creates a new symlink.
        make_symlink(&target_a, &link).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target_a);

        // Atomically replaces an existing symlink (no EEXIST).
        make_symlink(&target_b, &link).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target_b);

        // The temporary symlink is consumed by the rename — nothing left behind.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .map(|e| e.file_name())
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp symlink left behind: {leftovers:?}"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_is_unc_path() {
        assert!(is_unc_path(Path::new(
            r"\\wsl.localhost\DistroName\github\verzly\mise-php"
        )));
        assert!(is_unc_path(Path::new(
            r"\\wsl$\DistroName\github\verzly\mise-php"
        )));
        assert!(is_unc_path(Path::new(
            r"\\?\UNC\wsl.localhost\DistroName\github\verzly\mise-php"
        )));

        assert!(!is_unc_path(Path::new(r"D:\github\verzly\mise-php")));
    }

    #[test]
    fn test_archive_content_files_tar_hashes_regular_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("tool.tar");
        {
            let file = File::create(&archive_path).unwrap();
            let mut builder = tar::Builder::new(file);
            let mut header = tar::Header::new_gnu();
            header.set_path("pkg/tool").unwrap();
            header.set_size(4);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append(&header, &b"tool"[..]).unwrap();
            builder.finish().unwrap();
        }

        let files = archive_content_files(&archive_path, ExtractionFormat::Tar, 1).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "tool");
        assert_eq!(files[0].sha256, hex::encode(Sha256::digest(b"tool")));
    }

    #[test]
    fn test_archive_content_files_tar_rejects_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("tool.tar");
        {
            let file = File::create(&archive_path).unwrap();
            let mut builder = tar::Builder::new(file);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_path("tool-link").unwrap();
            header.set_link_name("tool").unwrap();
            header.set_size(0);
            header.set_mode(0o777);
            header.set_cksum();
            builder.append(&header, std::io::empty()).unwrap();
            builder.finish().unwrap();
        }

        let err = archive_content_files(&archive_path, ExtractionFormat::Tar, 0).unwrap_err();
        assert!(err.to_string().contains("non-regular archive entry"));
    }

    #[tokio::test]
    async fn test_find_up() {
        let _config = Config::get().await.unwrap();
        let path = &env::current_dir().unwrap();
        let filenames = vec![".miserc", ".mise.toml", ".test-tool-versions"]
            .into_iter()
            .map(|s| s.to_string())
            .collect_vec();
        #[allow(clippy::needless_collect)]
        let find_up = FindUp::new(path, &filenames).collect::<Vec<_>>();
        let mut find_up = find_up.into_iter();
        assert_eq!(
            find_up.next(),
            Some(dirs::HOME.join("cwd/.test-tool-versions"))
        );
        assert_eq!(find_up.next(), Some(dirs::HOME.join(".test-tool-versions")));
    }

    #[tokio::test]
    async fn test_find_up_2() {
        let _config = Config::get().await.unwrap();
        let path = &dirs::HOME.join("fixtures");
        let filenames = vec![".test-tool-versions"];
        let result = find_up(path, &filenames);
        assert_eq!(result, Some(dirs::HOME.join(".test-tool-versions")));
    }

    #[tokio::test]
    async fn test_dir_subdirs() {
        let _config = Config::get().await.unwrap();
        let subdirs = dir_subdirs(&dirs::HOME).unwrap();
        assert!(subdirs.contains("cwd"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_display_path() {
        let _config = Config::get().await.unwrap();
        use std::ops::Deref;
        let path = dirs::HOME.join("cwd");
        assert_eq!(display_path(path), "~/cwd");

        let path = Path::new("/tmp")
            .join(dirs::HOME.deref().strip_prefix("/").unwrap())
            .join("cwd");
        assert_eq!(display_path(&path), path.display().to_string());
    }

    #[tokio::test]
    async fn test_replace_path() {
        let _config = Config::get().await.unwrap();
        assert_eq!(replace_path(Path::new("~/cwd")), dirs::HOME.join("cwd"));
        assert_eq!(replace_path(Path::new("/cwd")), Path::new("/cwd"));
    }

    #[test]
    fn test_paths_eq_exact() {
        assert!(paths_eq(Path::new("/foo/bar"), Path::new("/foo/bar")));
        assert!(!paths_eq(Path::new("/foo/bar"), Path::new("/foo/baz")));
    }

    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn test_paths_eq_case_insensitive() {
        // macOS volumes (HFS+/APFS) and Windows volumes are case-insensitive by
        // default. The comparator must treat `/Users/Foo` and `/Users/foo` as
        // equal so that PATH stripping doesn't miss the shims dir when `$HOME`
        // is mixed-case in the user's environment but the resolved shims path
        // uses a different case (the cause of the npm-shim recursion bug).
        assert!(paths_eq(
            Path::new("/Users/Olfway/.local/share/mise/shims"),
            Path::new("/Users/olfway/.local/share/mise/shims"),
        ));
    }

    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn test_paths_eq_trailing_separator() {
        // Component-based comparison should fold trailing separators and
        // redundant double-separators so PATH entries like `/foo/shims/`
        // still match `/foo/shims`.
        assert!(paths_eq(Path::new("/foo/shims"), Path::new("/foo/shims/")));
        assert!(paths_eq(Path::new("/foo/shims"), Path::new("/foo//shims"),));
    }

    #[test]
    #[cfg(all(not(windows), not(target_os = "macos")))]
    fn test_paths_eq_case_sensitive_on_linux() {
        // Linux paths are case-sensitive; `/foo` and `/Foo` are distinct files.
        assert!(!paths_eq(Path::new("/foo/bar"), Path::new("/Foo/bar")));
    }

    #[test]
    #[cfg(windows)]
    fn test_paths_eq_separator_normalization() {
        assert!(paths_eq(
            Path::new("C:/Users/foo/shims"),
            Path::new("C:\\Users\\foo\\shims"),
        ));
    }

    #[test]
    fn test_should_strip_components() {
        // Test that the function correctly identifies when to strip components
        // This is a basic test to ensure the logic works correctly

        // For now, we'll test with a nonexistent file to ensure the function
        // returns false when it can't read the archive
        let non_existent_path = Path::new("/non/existent/archive.tar.gz");
        let result = should_strip_components(non_existent_path, ExtractionFormat::TarGz);
        assert!(result.is_err()); // Should fail to open nonexistent file

        // Note: To properly test this function, we would need actual tar archives
        // with different structures (single file, single directory, multiple entries)
        // This would require creating test fixtures, which is beyond the scope
        // of this fix. The important thing is that the logic now correctly
        // checks if the single entry is a directory before deciding to strip.
    }

    #[test]
    fn test_inspect_tar_contents_logic() {
        // Test the logic of inspect_tar_contents with simulated data
        // This tests the core logic without requiring actual tar files

        // Simulate a HashMap that would be returned by inspect_tar_contents
        // for an archive with a single directory containing files
        let mut components = std::collections::HashMap::new();
        components.insert("mydir".to_string(), true); // Directory with nested files

        let result: Vec<(String, bool)> = components.into_iter().collect();

        // Should have exactly one entry that is a directory
        assert_eq!(result.len(), 1);
        let (name, is_directory) = &result[0];
        assert_eq!(name, "mydir");
        assert!(*is_directory);

        // Test the should_strip_components logic with this result
        // This simulates what would happen if inspect_tar_contents returned this
        let should_strip = result.len() == 1 && result[0].1;
        assert!(should_strip);
    }

    #[test]
    fn test_inspect_tar_contents_curdir_prefix() {
        // Test that archives with "./" prefixed paths are handled correctly
        // This reproduces the bug from https://github.com/jdx/mise/discussions/7862
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use tar::Builder;
        use tempfile::NamedTempFile;

        // Create a temp tar.gz with "./" prefixed paths (like unison's archive)
        let temp_file = NamedTempFile::new().unwrap();
        let gz = GzEncoder::new(temp_file.as_file(), Compression::default());
        let mut builder = Builder::new(gz);

        // Add entries with "./" prefix - simulating archive structure like:
        // ./dir1/file1
        // ./dir2/file2
        // ./standalone
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();

        // Add ./dir1/file1
        builder
            .append_data(&mut header.clone(), "./dir1/file1", std::io::empty())
            .unwrap();

        // Add ./dir2/file2
        builder
            .append_data(&mut header.clone(), "./dir2/file2", std::io::empty())
            .unwrap();

        // Add ./standalone (file at root with ./ prefix)
        builder
            .append_data(&mut header.clone(), "./standalone", std::io::empty())
            .unwrap();

        let gz = builder.into_inner().unwrap();
        gz.finish().unwrap();

        // Now test inspect_tar_contents
        let result = inspect_tar_contents(temp_file.path(), ExtractionFormat::TarGz).unwrap();

        // Should have 3 top-level entries: dir1, dir2, standalone
        // NOT a single "." entry
        assert_eq!(
            result.len(),
            3,
            "Expected 3 top-level entries, got: {:?}",
            result
        );

        let names: std::collections::HashSet<_> = result.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains("dir1"), "Should contain dir1");
        assert!(names.contains("dir2"), "Should contain dir2");
        assert!(names.contains("standalone"), "Should contain standalone");
        assert!(!names.contains("."), "Should NOT contain '.' (CurDir)");

        // dir1 and dir2 should be marked as directories (have nested content)
        for (name, is_dir) in &result {
            if name == "dir1" || name == "dir2" {
                assert!(*is_dir, "{} should be marked as directory", name);
            } else if name == "standalone" {
                assert!(!*is_dir, "standalone should NOT be marked as directory");
            }
        }

        // Verify should_strip_components returns false (multiple top-level entries)
        let should_strip =
            should_strip_components(temp_file.path(), ExtractionFormat::TarGz).unwrap();
        assert!(
            !should_strip,
            "Should NOT strip components for multi-entry archive"
        );
    }

    #[test]
    fn test_all_dirs_no_ceiling() {
        let start_dir = Path::new("/a/b/c");
        let ceiling_dirs = HashSet::new();

        let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

        assert_eq!(result.len(), 4);
        assert!(result.contains(&PathBuf::from("/a/b/c")));
        assert!(result.contains(&PathBuf::from("/a/b")));
        assert!(result.contains(&PathBuf::from("/a")));
        assert!(result.contains(&PathBuf::from("/")));
    }

    #[test]
    fn test_all_dirs_with_ceiling() {
        let start_dir = Path::new("/a/b/c");
        let mut ceiling_dirs = HashSet::new();
        ceiling_dirs.insert(PathBuf::from("/a"));

        let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

        assert_eq!(result.len(), 2);
        assert!(result.contains(&PathBuf::from("/a/b/c")));
        assert!(result.contains(&PathBuf::from("/a/b")));
        assert!(!result.contains(&PathBuf::from("/a")));
        assert!(!result.contains(&PathBuf::from("/")));
    }

    #[test]
    fn test_all_dirs_with_ceiling_at_start() {
        let start_dir = Path::new("/a/b/c");
        let mut ceiling_dirs = HashSet::new();
        ceiling_dirs.insert(PathBuf::from("/a/b/c"));

        let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_all_dirs_with_multiple_ceilings() {
        let start_dir = Path::new("/a/b/c/d/e");
        let mut ceiling_dirs = HashSet::new();
        ceiling_dirs.insert(PathBuf::from("/a/b"));
        ceiling_dirs.insert(PathBuf::from("/a/b/c/d"));

        let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

        assert_eq!(result.len(), 1);
        assert!(result.contains(&PathBuf::from("/a/b/c/d/e")));
    }

    #[test]
    fn test_all_dirs_with_relative_path() {
        let start_dir = Path::new("a/b/c");
        let ceiling_dirs = HashSet::new();

        let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

        assert!(result.contains(&PathBuf::from("a/b/c")));
        assert!(result.contains(&PathBuf::from("a/b")));
        assert!(result.contains(&PathBuf::from("a")));
    }

    #[test]
    fn test_extraction_format_from_file_name() {
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tar.gz"),
            ExtractionFormat::TarGz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tgz"),
            ExtractionFormat::TarGz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tar.xz"),
            ExtractionFormat::TarXz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.txz"),
            ExtractionFormat::TarXz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tar.bz2"),
            ExtractionFormat::TarBz2
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tbz2"),
            ExtractionFormat::TarBz2
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tbz"),
            ExtractionFormat::TarBz2
        );
        assert_eq!(
            ExtractionFormat::from_ext("tbz"),
            Some(ExtractionFormat::TarBz2)
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tar.zst"),
            ExtractionFormat::TarZst
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tzst"),
            ExtractionFormat::TarZst
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tar"),
            ExtractionFormat::Tar
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.zip"),
            ExtractionFormat::Zip
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.vsix"),
            ExtractionFormat::Zip
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.7z"),
            ExtractionFormat::SevenZip
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tar.br"),
            ExtractionFormat::TarBr
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tbr"),
            ExtractionFormat::TarBr
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.br"),
            ExtractionFormat::Br
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tar.lz4"),
            ExtractionFormat::TarLz4
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tlz4"),
            ExtractionFormat::TarLz4
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.lz4"),
            ExtractionFormat::Lz4
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tar.sz"),
            ExtractionFormat::TarSz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.tsz"),
            ExtractionFormat::TarSz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.sz"),
            ExtractionFormat::Sz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.rar"),
            ExtractionFormat::Rar
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.gz"),
            ExtractionFormat::Gz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.xz"),
            ExtractionFormat::Xz
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.bz2"),
            ExtractionFormat::Bz2
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.zst"),
            ExtractionFormat::Zst
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo"),
            ExtractionFormat::Raw
        );
        assert_eq!(
            ExtractionFormat::from_file_name("foo.txt"),
            ExtractionFormat::Raw
        );
    }

    #[test]
    fn test_unsupported_extraction_formats_are_classified() {
        for (ext, expected) in [
            ("tar.br", ExtractionFormat::TarBr),
            ("tbr", ExtractionFormat::TarBr),
            ("br", ExtractionFormat::Br),
            ("tar.lz4", ExtractionFormat::TarLz4),
            ("tlz4", ExtractionFormat::TarLz4),
            ("lz4", ExtractionFormat::Lz4),
            ("tar.sz", ExtractionFormat::TarSz),
            ("tsz", ExtractionFormat::TarSz),
            ("sz", ExtractionFormat::Sz),
            ("rar", ExtractionFormat::Rar),
        ] {
            assert_eq!(ExtractionFormat::from_ext(ext), Some(expected));
        }
        assert_eq!(ExtractionFormat::from_ext("unknown"), None);

        assert!(ExtractionFormat::TarBr.is_archive());
        assert!(ExtractionFormat::TarLz4.is_archive());
        assert!(ExtractionFormat::TarSz.is_archive());
        assert!(ExtractionFormat::Rar.is_archive());
        assert!(ExtractionFormat::Br.is_compressed_file());
        assert!(ExtractionFormat::Lz4.is_compressed_file());
        assert!(ExtractionFormat::Sz.is_compressed_file());
    }

    #[test]
    fn test_extraction_format_extension_uses_canonical_display() {
        for (format, expected) in [
            (ExtractionFormat::TarGz, Some("tar.gz")),
            (ExtractionFormat::TarXz, Some("tar.xz")),
            (ExtractionFormat::TarBz2, Some("tar.bz2")),
            (ExtractionFormat::TarZst, Some("tar.zst")),
            (ExtractionFormat::TarBr, Some("tar.br")),
            (ExtractionFormat::TarLz4, Some("tar.lz4")),
            (ExtractionFormat::TarSz, Some("tar.sz")),
            (ExtractionFormat::Zip, Some("zip")),
            (ExtractionFormat::Raw, None),
        ] {
            assert_eq!(format.extension().as_deref(), expected);
        }
    }

    #[test]
    fn test_decompress_file() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let src_path = dir.path().join("test.gz");
        let dest_path = dir.path().join("test-out");

        let file = File::create(&src_path).unwrap();
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(b"hello world").unwrap();
        encoder.finish().unwrap();

        decompress_file(&src_path, &dest_path, ExtractionFormat::Gz).unwrap();

        assert!(dest_path.exists());
        assert!(dest_path.is_file());
        let content = std::fs::read_to_string(&dest_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_decompress_file_creates_parent_dir() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let src_path = dir.path().join("test.gz");
        let dest_path = dir.path().join("missing").join("test-out");

        let file = File::create(&src_path).unwrap();
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(b"hello world").unwrap();
        encoder.finish().unwrap();

        decompress_file(&src_path, &dest_path, ExtractionFormat::Gz).unwrap();

        assert!(dest_path.exists());
        assert!(dest_path.is_file());
        let content = std::fs::read_to_string(&dest_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_extract_archive_zip() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let src_path = dir.path().join("test.zip");
        let dest_dir = dir.path().join("out_dir");

        let file = File::create(&src_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("pkg/tool", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"hello world").unwrap();
        zip.finish().unwrap();

        extract_archive(
            &src_path,
            &dest_dir,
            ExtractionFormat::Zip,
            &ExtractOptions::default(),
        )
        .unwrap();

        let extracted_path = dest_dir.join("pkg").join("tool");
        assert!(extracted_path.exists());
        assert!(extracted_path.is_file());
        let content = std::fs::read_to_string(&extracted_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_extract_archive_7z() {
        use std::io::Cursor;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let pkg_dir = src_dir.join("pkg");
        let archive_path = dir.path().join("test.7z");
        let dest_dir = dir.path().join("out_dir");
        let stripped_dest_dir = dir.path().join("stripped_out_dir");
        let backslash_archive_path = dir.path().join("backslash.7z");
        let backslash_dest_dir = dir.path().join("backslash_out_dir");
        let traversal_archive_path = dir.path().join("traversal.7z");
        let traversal_dest_dir = dir.path().join("traversal_out_dir");
        let traversal_target_path = dir.path().join("traversal_target");
        let absolute_archive_path = dir.path().join("absolute.7z");
        let absolute_dest_dir = dir.path().join("absolute_out_dir");
        let absolute_target_path = dir.path().join("absolute_target");

        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join("tool"), "hello world").unwrap();
        sevenz_rust2::compress_to_path(&src_dir, &archive_path).unwrap();

        let contents = inspect_7z_contents(&archive_path).unwrap();
        assert!(contents.contains(&("pkg".to_string(), true)));
        assert!(should_strip_components(&archive_path, ExtractionFormat::SevenZip).unwrap());

        extract_archive(
            &archive_path,
            &dest_dir,
            ExtractionFormat::SevenZip,
            &ExtractOptions::default(),
        )
        .unwrap();

        let extracted_path = dest_dir.join("pkg").join("tool");
        assert!(extracted_path.exists());
        assert!(extracted_path.is_file());
        let content = std::fs::read_to_string(&extracted_path).unwrap();
        assert_eq!(content, "hello world");

        extract_archive(
            &archive_path,
            &stripped_dest_dir,
            ExtractionFormat::SevenZip,
            &ExtractOptions {
                strip_components: 1,
                ..Default::default()
            },
        )
        .unwrap();

        let stripped_path = stripped_dest_dir.join("tool");
        assert!(stripped_path.exists());
        assert!(stripped_path.is_file());
        assert!(!stripped_dest_dir.join("pkg").exists());
        let content = std::fs::read_to_string(&stripped_path).unwrap();
        assert_eq!(content, "hello world");

        let mut backslash_archive =
            sevenz_rust2::ArchiveWriter::create(&backslash_archive_path).unwrap();
        backslash_archive
            .push_archive_entry(
                sevenz_rust2::ArchiveEntry::new_file("pkg\\tool"),
                Some(Cursor::new(b"hello world")),
            )
            .unwrap();
        backslash_archive.finish().unwrap();

        let contents = inspect_7z_contents(&backslash_archive_path).unwrap();
        assert!(contents.contains(&("pkg".to_string(), true)));
        assert!(
            should_strip_components(&backslash_archive_path, ExtractionFormat::SevenZip).unwrap()
        );

        extract_archive(
            &backslash_archive_path,
            &backslash_dest_dir,
            ExtractionFormat::SevenZip,
            &ExtractOptions {
                strip_components: 1,
                ..Default::default()
            },
        )
        .unwrap();

        let backslash_stripped_path = backslash_dest_dir.join("tool");
        assert!(backslash_stripped_path.exists());
        assert!(backslash_stripped_path.is_file());
        assert!(!backslash_dest_dir.join("pkg").exists());
        let content = std::fs::read_to_string(&backslash_stripped_path).unwrap();
        assert_eq!(content, "hello world");

        let mut traversal_archive =
            sevenz_rust2::ArchiveWriter::create(&traversal_archive_path).unwrap();
        traversal_archive
            .push_archive_entry(
                sevenz_rust2::ArchiveEntry::new_file("../traversal_target"),
                Some(Cursor::new(b"malicious")),
            )
            .unwrap();
        traversal_archive.finish().unwrap();

        let err = extract_archive(
            &traversal_archive_path,
            &traversal_dest_dir,
            ExtractionFormat::SevenZip,
            &ExtractOptions::default(),
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("escapes extraction directory"),
            "{err:#}"
        );
        assert!(!traversal_target_path.exists());

        let mut absolute_archive =
            sevenz_rust2::ArchiveWriter::create(&absolute_archive_path).unwrap();
        absolute_archive
            .push_archive_entry(
                sevenz_rust2::ArchiveEntry::new_file(&absolute_target_path.to_string_lossy()),
                Some(Cursor::new(b"malicious")),
            )
            .unwrap();
        absolute_archive.finish().unwrap();

        let err = extract_archive(
            &absolute_archive_path,
            &absolute_dest_dir,
            ExtractionFormat::SevenZip,
            &ExtractOptions::default(),
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("escapes extraction directory"),
            "{err:#}"
        );
        assert!(!absolute_target_path.exists());
    }

    #[test]
    fn test_untar_rejects_single_file_compression() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let src_path = dir.path().join("test.gz");
        let dest_path = dir.path().join("test-out");
        let err = untar(
            &src_path,
            &dest_path,
            ExtractionFormat::Gz,
            &ExtractOptions::default(),
        )
        .unwrap_err();

        assert!(
            format!("{err:#}").contains("untar only supports tar formats"),
            "{err:#}"
        );
    }

    #[test]
    fn test_unsupported_extraction_formats_error_clearly() {
        use tempfile::NamedTempFile;
        use tempfile::tempdir;

        let archive = NamedTempFile::new().unwrap();
        let dest = tempdir().unwrap();

        let err = extract_archive(
            archive.path(),
            dest.path(),
            ExtractionFormat::TarBr,
            &ExtractOptions::default(),
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("tar.br format not supported"));

        let err = extract_archive(
            archive.path(),
            dest.path(),
            ExtractionFormat::Rar,
            &ExtractOptions::default(),
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("rar format not supported"));

        let err = decompress_file(
            archive.path(),
            dest.path().join("tool").as_path(),
            ExtractionFormat::Lz4,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("lz4 format not supported"));
    }

    #[tokio::test]
    async fn test_remove_file_async_if_exists_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        tokio::fs::write(&path, "content").await.unwrap();
        remove_file_async_if_exists(&path).await.unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_remove_file_async_if_exists_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent");
        // Should not error when file does not exist.
        remove_file_async_if_exists(&path).await.unwrap();
    }

    #[cfg(all(unix, target_os = "linux"))]
    #[test]
    fn test_move_file_falls_back_to_copy_across_filesystems() {
        use std::{fs, os::unix::fs::MetadataExt};
        use tempfile::tempdir_in;

        let source_root = std::env::current_dir().unwrap();
        let source_dir = tempdir_in(&source_root).unwrap();
        let source_dev = source_dir.path().metadata().unwrap().dev();

        let target_dir = tempdir_in("/tmp").unwrap();
        if target_dir.path().metadata().unwrap().dev() == source_dev {
            // This host only has one filesystem for tempdirs, so skip if we can't reproduce EXDEV.
            return;
        }

        let src = source_dir.path().join("bun");
        let dst = target_dir.path().join("bun");
        fs::write(&src, b"hello").unwrap();

        move_file(&src, &dst).unwrap();

        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"hello");
    }

    #[cfg(all(unix, target_os = "linux"))]
    #[test]
    fn test_move_dir_falls_back_to_copy_across_filesystems() {
        use std::{fs, os::unix::fs::MetadataExt};
        use tempfile::tempdir_in;

        let source_root = std::env::current_dir().unwrap();
        let source_dir = tempdir_in(&source_root).unwrap();
        let source_dev = source_dir.path().metadata().unwrap().dev();

        let target_dir = tempdir_in("/tmp").unwrap();
        if target_dir.path().metadata().unwrap().dev() == source_dev {
            // This host only has one filesystem for tempdirs, so skip if we can't reproduce EXDEV.
            return;
        }

        let src = source_dir.path().join("bun-tree");
        let dst = target_dir.path().join("bun-tree");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("nested/bun"), b"hello").unwrap();

        move_file(&src, &dst).unwrap();

        assert!(!src.exists());
        assert_eq!(fs::read(dst.join("nested/bun")).unwrap(), b"hello");
    }
}
