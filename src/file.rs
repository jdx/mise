use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

use color_eyre::eyre::Result;
use filetime::{set_file_times, FileTime};
use std::os::unix::fs::symlink;

use crate::dirs;

pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    if path.exists() {
        trace!("rm -rf {}", path.display());
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub fn create_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    trace!("mkdir -p {}", path.display());
    std::fs::create_dir_all(path)?;
    Ok(())
}

pub fn basename(path: &Path) -> Option<String> {
    path.file_name().map(|f| f.to_string_lossy().to_string())
}

/// replaces $HOME with "~"
pub fn display_path(path: &Path) -> String {
    let home = dirs::HOME.to_string_lossy();
    match path.starts_with(home.as_ref()) {
        true => path.to_string_lossy().replacen(home.as_ref(), "~", 1),
        false => path.to_string_lossy().to_string(),
    }
}

pub fn touch_dir(dir: &Path) -> io::Result<()> {
    trace!("touch {}", dir.display());
    let now = FileTime::now();
    set_file_times(dir, now, now)
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

pub struct FindUp {
    current_dir: PathBuf,
    current_dir_filenames: Vec<String>,
    filenames: Vec<String>,
}

impl FindUp {
    pub fn new(from: &Path, filenames: &[&str]) -> Self {
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

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use crate::dirs;

    use super::*;

    #[test]
    fn test_find_up() {
        let path = &dirs::CURRENT;
        let filenames = vec![".rtxrc", ".rtxrc.toml", ".test-tool-versions"];
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
}

pub fn make_symlink(target: &Path, link: &Path) -> Result<()> {
    if link.exists() {
        fs::remove_file(link)?;
    }
    symlink(target, link)?;
    Ok(())
}
