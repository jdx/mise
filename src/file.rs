use std::fs;
use std::fs::File;
use std::os::unix::fs::symlink;
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{Context, Result};
use filetime::{set_file_times, FileTime};
use flate2::read::GzDecoder;
use tar::Archive;

use crate::{cmd, dirs, env};

pub fn remove_all<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    match path.metadata().map(|m| m.file_type()) {
        Ok(x) if x.is_symlink() || x.is_file() => {
            remove_file(path)?;
        }
        Ok(x) if x.is_dir() => {
            trace!("rm -rf {}", path.display());
            fs::remove_dir_all(path)
                .with_context(|| format!("failed rm -rf: {}", path.display()))?;
        }
        _ => {}
    };
    Ok(())
}

pub fn remove_file<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("rm {}", path.display());
    fs::remove_file(path).with_context(|| format!("failed rm: {}", path.display()))
}

pub fn remove_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("rmdir {}", path.display());
    fs::remove_dir(path).with_context(|| format!("failed rmdir: {}", path.display()))
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
    fs::rename(from, to)
        .with_context(|| format!("failed rename: {} -> {}", from.display(), to.display()))
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    let path = path.as_ref();
    trace!("write {}", path.display());
    fs::write(path, contents).with_context(|| format!("failed write: {}", path.display()))
}

pub fn read_to_string<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    trace!("cat {}", path.display());
    fs::read_to_string(path).with_context(|| format!("failed read_to_string: {}", path.display()))
}

pub fn create_dir_all<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("mkdir -p {}", path.display());
    fs::create_dir_all(path).with_context(|| format!("failed create_dir_all: {}", path.display()))
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
    set_file_times(dir, now, now).with_context(|| format!("failed to touch dir: {}", dir.display()))
}

pub fn modified_duration(path: &Path) -> Result<Duration> {
    let metadata = path.metadata()?;
    let modified = metadata.modified()?;
    let duration = modified.elapsed()?;
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

    for entry in dir.read_dir()? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_dir() || ft.is_symlink() {
            output.push(entry.file_name().into_string().unwrap());
        }
    }

    Ok(output)
}

pub fn dir_files(dir: &Path) -> Result<Vec<String>> {
    let mut output = vec![];

    if !dir.is_dir() {
        return Ok(output);
    }

    for entry in dir.read_dir()? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            output.push(entry.file_name().into_string().unwrap());
        }
    }

    Ok(output)
}

pub fn make_symlink(target: &Path, link: &Path) -> Result<()> {
    trace!("ln -sf {} {}", target.display(), link.display());
    if link.is_file() || link.is_symlink() {
        fs::remove_file(link)?;
    }
    symlink(target, link)?;
    Ok(())
}

pub fn remove_symlinks_with_target_prefix(symlink_dir: &Path, target_prefix: &Path) -> Result<()> {
    if !symlink_dir.exists() {
        return Ok(());
    }
    for entry in symlink_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.is_symlink() {
            let target = path.read_link()?;
            if target.starts_with(target_prefix) {
                fs::remove_file(&path)?;
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
    let mut perms = path.metadata()?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)?;
    Ok(())
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
        if cfg!(test) && self.current_dir == dirs::HOME.as_path() {
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
    let f = File::open(archive)?;
    let tar = GzDecoder::new(f);
    let mut archive = Archive::new(tar);
    archive.unpack(dest)?;
    Ok(())
}

pub fn unzip(archive: &Path, dest: &Path) -> Result<()> {
    cmd!("unzip", archive, "-d", dest).run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use itertools::Itertools;

    use crate::dirs;

    use super::*;

    #[test]
    fn test_find_up() {
        let path = &dirs::CURRENT;
        let filenames = vec![".rtxrc", ".rtxrc.toml", ".test-tool-versions"]
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
        let subdirs = dir_subdirs(dirs::HOME.as_path()).unwrap();
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
