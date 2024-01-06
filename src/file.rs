use std::fs;
use std::fs::File;
use std::os::unix::fs::symlink;
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

use filetime::{set_file_times, FileTime};
use flate2::read::GzDecoder;
use miette::{Context, IntoDiagnostic, Result};
use tar::Archive;
use zip::ZipArchive;

use crate::{dirs, env};

pub fn remove_all<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    match path.metadata().map(|m| m.file_type()) {
        Ok(x) if x.is_symlink() || x.is_file() => {
            remove_file(path)?;
        }
        Ok(x) if x.is_dir() => {
            trace!("rm -rf {}", display_path(path));
            fs::remove_dir_all(path)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed rm -rf: {}", display_path(path)))?;
        }
        _ => {}
    };
    Ok(())
}

pub fn remove_file<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("rm {}", display_path(path));
    fs::remove_file(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed rm: {}", display_path(path)))
}

pub fn remove_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    (|| -> Result<()> {
        if path.exists() && is_empty_dir(path)? {
            trace!("rmdir {}", display_path(path));
            fs::remove_dir(path).into_diagnostic()?;
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
    fs::rename(from, to).into_diagnostic().wrap_err_with(|| {
        format!(
            "failed rename: {} -> {}",
            display_path(from),
            display_path(to)
        )
    })
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    trace!("write {}", display_path(path));
    fs::write(path, contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed write: {}", display_path(path)))
}

pub fn read_to_string<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    trace!("cat {}", display_path(path));
    fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed read_to_string: {}", display_path(path)))
}

pub fn create(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    trace!("touch {}", display_path(path));
    File::create(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed create: {}", display_path(path)))
}

pub fn create_dir_all<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        trace!("mkdir -p {}", display_path(path));
        fs::create_dir_all(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed create_dir_all: {}", display_path(path)))?;
    }
    Ok(())
}

pub fn basename(path: &Path) -> Option<String> {
    path.file_name().map(|f| f.to_string_lossy().to_string())
}

/// replaces $HOME with "~"
pub fn display_path(path: &Path) -> String {
    let home = dirs::HOME.to_string_lossy();
    match path.starts_with(home.as_ref()) && home != "/" {
        true => path.to_string_lossy().replacen(home.as_ref(), "~", 1),
        false => path.to_string_lossy().to_string(),
    }
}

/// replaces "~" with $HOME
pub fn replace_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    match path.starts_with("~/") {
        true => dirs::HOME.join(path.strip_prefix("~/").unwrap()),
        false => path.to_path_buf(),
    }
}

pub fn touch_dir(dir: &Path) -> Result<()> {
    trace!("touch {}", dir.display());
    let now = FileTime::now();
    set_file_times(dir, now, now)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to touch dir: {}", display_path(dir)))
}

pub fn modified_duration(path: &Path) -> Result<Duration> {
    let metadata = path.metadata().into_diagnostic()?;
    let modified = metadata.modified().into_diagnostic()?;
    let duration = modified.elapsed().into_diagnostic()?;
    Ok(duration)
}

pub fn find_up(from: &Path, filenames: &[&str]) -> Option<PathBuf> {
    let mut current = from.to_path_buf();
    loop {
        for filename in filenames {
            let path = current.join(filename);
            if path.exists() {
                return Some(path);
            }
        }
        if !current.pop() {
            return None;
        }
    }
}

pub fn dir_subdirs(dir: &Path) -> Result<Vec<String>> {
    let mut output = vec![];

    if !dir.exists() {
        return Ok(output);
    }

    for entry in dir.read_dir().into_diagnostic()? {
        let entry = entry.into_diagnostic()?;
        let ft = entry.file_type().into_diagnostic()?;
        if ft.is_dir() || ft.is_symlink() {
            output.push(entry.file_name().into_string().unwrap());
        }
    }

    Ok(output)
}

pub fn ls(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut output = vec![];

    if !dir.is_dir() {
        return Ok(output);
    }

    for entry in dir.read_dir().into_diagnostic()? {
        let entry = entry.into_diagnostic()?;
        if entry.file_type().into_diagnostic()?.is_file() {
            output.push(entry.path());
        }
    }

    Ok(output)
}

pub fn make_symlink(target: &Path, link: &Path) -> Result<()> {
    trace!("ln -sf {} {}", target.display(), link.display());
    if link.is_file() || link.is_symlink() {
        fs::remove_file(link).into_diagnostic()?;
    }
    symlink(target, link).into_diagnostic()?;
    Ok(())
}

pub fn remove_symlinks_with_target_prefix(symlink_dir: &Path, target_prefix: &Path) -> Result<()> {
    if !symlink_dir.exists() {
        return Ok(());
    }
    for entry in symlink_dir.read_dir().into_diagnostic()? {
        let entry = entry.into_diagnostic()?;
        let path = entry.path();
        if path.is_symlink() {
            let target = path.read_link().into_diagnostic()?;
            if target.starts_with(target_prefix) {
                fs::remove_file(&path).into_diagnostic()?;
            }
        }
    }
    Ok(())
}

pub fn is_executable(path: &Path) -> bool {
    if let Ok(metadata) = path.metadata() {
        return metadata.permissions().mode() & 0o111 != 0;
    }
    false
}

pub fn make_executable(path: &Path) -> Result<()> {
    let mut perms = path.metadata().into_diagnostic()?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to chmod +x: {}", display_path(path)))?;
    Ok(())
}

fn is_empty_dir(path: &Path) -> Result<bool> {
    path.read_dir()
        .map(|mut i| i.next().is_none())
        .into_diagnostic()
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
        self.current_dir_filenames = self.filenames.clone();
        if cfg!(test) && self.current_dir == *dirs::HOME {
            return None; // in tests, do not recurse further than ./test
        }
        if !self.current_dir.pop() {
            return None;
        }
        self.next()
    }
}

pub fn which(name: &str) -> Option<PathBuf> {
    for path in &*env::PATH {
        let bin = path.join(name);
        if is_executable(&bin) {
            return Some(bin);
        }
    }
    None
}

pub fn untar(archive: &Path, dest: &Path) -> Result<()> {
    debug!("tar -xzf {} -C {}", archive.display(), dest.display());
    let f = File::open(archive).into_diagnostic()?;
    let tar = GzDecoder::new(f);
    Archive::new(tar)
        .unpack(dest)
        .into_diagnostic()
        .wrap_err_with(|| {
            let archive = display_path(archive);
            let dest = display_path(dest);
            format!("failed to extract tar: {archive} to {dest}")
        })
}

pub fn unzip(archive: &Path, dest: &Path) -> Result<()> {
    ZipArchive::new(File::open(archive).into_diagnostic()?)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to open zip archive: {}", display_path(archive)))?
        .extract(dest)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to extract zip archive: {}", display_path(archive)))
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use itertools::Itertools;

    use crate::dirs;

    use super::*;

    #[test]
    fn test_find_up() {
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

    #[test]
    fn test_find_up_2() {
        let path = &dirs::HOME.join("fixtures");
        let filenames = vec![".test-tool-versions"];
        let result = find_up(path, &filenames);
        assert_eq!(result, Some(dirs::HOME.join(".test-tool-versions")));
    }

    #[test]
    fn test_dir_subdirs() {
        let subdirs = dir_subdirs(&dirs::HOME).unwrap();
        assert!(subdirs.contains(&"cwd".to_string()));
    }

    #[test]
    fn test_display_path() {
        let path = dirs::HOME.join("cwd");
        assert_eq!(display_path(&path), "~/cwd");

        let path = Path::new("/tmp")
            .join(dirs::HOME.deref().strip_prefix("/").unwrap())
            .join("cwd");
        assert_eq!(display_path(&path), path.display().to_string());
    }

    #[test]
    fn test_replace_path() {
        assert_eq!(replace_path(Path::new("~/cwd")), dirs::HOME.join("cwd"));
        assert_eq!(replace_path(Path::new("/cwd")), Path::new("/cwd"));
    }
}
