use crate::path::{Path, PathBuf, PathExt};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::os::unix::prelude::*;
use std::sync::Mutex;
use std::time::Duration;

use color_eyre::eyre::{Context, Result};
use eyre::bail;
use filetime::{FileTime, set_file_times};
use flate2::read::GzDecoder;
use itertools::Itertools;
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

pub async fn remove_file_async<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("rm {}", display_path(path));
    tokio::fs::remove_file(path)
        .await
        .wrap_err_with(|| format!("failed rm: {}", display_path(path)))
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

pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    trace!("mv {} {}", from.display(), to.display());
    fs::rename(from, to).wrap_err_with(|| {
        format!(
            "failed rename: {} -> {}",
            display_path(from),
            display_path(to)
        )
    })
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
    if link.is_file() || link.is_symlink() {
        fs::remove_file(link)?;
    }
    symlink(target, link)
        .wrap_err_with(|| format!("failed to ln -sf {} {}", target.display(), link.display()))?;
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
pub fn make_symlink(target: &Path, link: &Path) -> Result<(PathBuf, PathBuf)> {
    if let Err(err) = junction::create(target, link) {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            let _ = fs::remove_file(link);
            junction::create(target, link)
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
    if !link.is_symlink() {
        return Ok(None);
    }
    if cfg!(windows) {
        Ok(Some(fs::read_to_string(link)?.into()))
    } else {
        Ok(Some(fs::read_link(link)?))
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

pub fn all_dirs() -> Result<Vec<PathBuf>> {
    let mut output = vec![];
    let dir = env::current_dir().ok();
    let mut cwd = dir.as_deref();
    while let Some(dir) = cwd {
        output.push(dir.to_path_buf());
        cwd = dir.parent();
    }
    Ok(output)
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

pub fn un_bz2(input: &Path, dest: &Path) -> Result<()> {
    debug!("bzip2 -d {} -c > {}", input.display(), dest.display());
    let f = File::open(input)?;
    let mut dec = bzip2::read::BzDecoder::new(f);
    let mut output = File::create(dest)?;
    std::io::copy(&mut dec, &mut output)
        .wrap_err_with(|| format!("failed to un-bz2: {}", display_path(input)))?;
    Ok(())
}

#[derive(Default, Clone, Copy, PartialEq, strum::EnumString, strum::Display)]
pub enum TarFormat {
    #[default]
    Auto,
    #[strum(serialize = "tar.gz")]
    TarGz,
    #[strum(serialize = "tar.xz")]
    TarXz,
    #[strum(serialize = "tar.bz2")]
    TarBz2,
    #[strum(serialize = "tar.zst")]
    TarZst,
    #[strum(serialize = "zip")]
    Zip,
}

impl TarFormat {
    pub fn from_ext(ext: &str) -> Self {
        match ext {
            "xz" => TarFormat::TarXz,
            "bz2" => TarFormat::TarBz2,
            "zst" => TarFormat::TarZst,
            "zip" => TarFormat::Zip,
            _ => TarFormat::TarGz,
        }
    }
}

#[derive(Default)]
pub struct TarOptions<'a> {
    pub format: TarFormat,
    pub strip_components: usize,
    pub pr: Option<&'a Box<dyn SingleReport>>,
}

pub fn untar(archive: &Path, dest: &Path, opts: &TarOptions) -> Result<()> {
    let format = match opts.format {
        TarFormat::Auto => {
            TarFormat::from_ext(archive.extension().unwrap().to_string_lossy().as_ref())
        }
        _ => opts.format,
    };
    if format == TarFormat::Zip {
        unzip(archive, dest)?;
        match opts.strip_components {
            0 => {}
            1 => {
                let entries = ls(dest)?
                    .into_iter()
                    .map(|p| ls(&p))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                for entry in entries {
                    let mut new_dest = dest.to_path_buf();
                    new_dest.push(entry.file_name().unwrap());
                    fs::rename(entry, new_dest)?;
                }
            }
            _ => bail!("strip-components not supported for zip archives"),
        }
        return Ok(());
    }
    debug!("tar -xf {} -C {}", archive.display(), dest.display());
    if let Some(pr) = &opts.pr {
        pr.set_message(format!(
            "extract {}",
            archive.file_name().unwrap().to_string_lossy()
        ));
    }
    let tar = open_tar(format, archive)?;
    let err = || {
        let archive = display_path(archive);
        let dest = display_path(dest);
        format!("failed to extract tar: {archive} to {dest}")
    };
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
    for entry in Archive::new(tar).entries().wrap_err_with(err)? {
        let mut entry = entry.wrap_err_with(err)?;
        let path: PathBuf = entry
            .path()
            .wrap_err_with(err)?
            .components()
            .skip(opts.strip_components)
            .filter(|c| c.as_os_str() != ".")
            .collect();
        let path = dest.join(path);
        create_dir_all(path.parent().unwrap()).wrap_err_with(err)?;
        trace!("extracting {}", display_path(&path));
        entry.unpack(&path).wrap_err_with(err)?;
        if let Some(pr) = &opts.pr {
            pr.set_length(entry.raw_file_position());
        }
    }
    // if let Some(pr) = &opts.pr {
    //     pr.set_position(total);
    // }
    Ok(())
}

fn open_tar(format: TarFormat, archive: &Path) -> Result<Box<dyn std::io::Read>> {
    let f = File::open(archive)?;
    Ok(match format {
        TarFormat::TarGz => Box::new(GzDecoder::new(f)),
        TarFormat::TarXz => Box::new(xz2::read::XzDecoder::new(f)),
        TarFormat::TarBz2 => Box::new(bzip2::read::BzDecoder::new(f)),
        TarFormat::TarZst => Box::new(zstd::stream::read::Decoder::new(f)?),
        TarFormat::Zip => bail!("zip format not supported"),
        TarFormat::Auto => match archive.extension().and_then(|s| s.to_str()) {
            Some("xz") => open_tar(TarFormat::TarXz, archive)?,
            Some("bz2") => open_tar(TarFormat::TarBz2, archive)?,
            Some("zst") => open_tar(TarFormat::TarZst, archive)?,
            Some("zip") => bail!("zip format not supported"),
            _ => open_tar(TarFormat::TarGz, archive)?,
        },
    })
}

pub fn unzip(archive: &Path, dest: &Path) -> Result<()> {
    // TODO: show progress
    debug!("unzip {} -d {}", archive.display(), dest.display());
    ZipArchive::new(File::open(archive)?)
        .wrap_err_with(|| format!("failed to open zip archive: {}", display_path(archive)))?
        .extract(dest)
        .wrap_err_with(|| format!("failed to extract zip archive: {}", display_path(archive)))
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

#[cfg(windows)]
pub fn un7z(archive: &Path, dest: &Path) -> Result<()> {
    sevenz_rust::decompress_file(archive, dest)
        .wrap_err_with(|| format!("failed to extract 7z archive: {}", display_path(archive)))
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
    if p.is_symlink() {
        if let Ok(target) = fs::read_link(p) {
            return target
                .canonicalize()
                .unwrap_or_else(|_| target.to_path_buf());
        }
    }
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

pub fn clone_dir(from: &PathBuf, to: &PathBuf) -> Result<()> {
    if cfg!(macos) {
        cmd!("cp", "-cR", from, to).run()?;
    } else if cfg!(windows) {
        cmd!("robocopy", from, to, "/MIR").run()?;
    } else {
        cmd!("cp", "--reflink=auto", "-r", from, to).run()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::config::Config;

    use super::*;

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
}
