use crate::file::display_path;
use eyre::{Result, eyre};
use glob::glob;
use itertools::Itertools;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Check if a path is a glob pattern
pub fn is_glob_pattern(path: &str) -> bool {
    // This is the character set used for glob detection by glob
    let glob_chars = ['*', '{', '}'];
    path.chars().any(|c| glob_chars.contains(&c))
}

/// Get the last modified time from a list of paths
pub(crate) fn last_modified_path(root: &Path, paths: &[&String]) -> Result<Option<SystemTime>> {
    let files = paths.iter().map(|p| {
        let base = Path::new(p);
        if base.is_relative() {
            Path::new(&root).join(base)
        } else {
            base.to_path_buf()
        }
    });

    last_modified_file(files)
}

/// Get the last modified time from files matching glob patterns
pub(crate) fn last_modified_glob_match(
    root: impl AsRef<Path>,
    patterns: &[&String],
) -> Result<Option<SystemTime>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let files = patterns
        .iter()
        .flat_map(|pattern| {
            glob(
                root.as_ref()
                    .join(pattern)
                    .to_str()
                    .expect("Conversion to string path failed"),
            )
            .unwrap()
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.metadata()
                .expect("Metadata call failed")
                .file_type()
                .is_file()
        });

    last_modified_file(files)
}

/// Get the last modified time from an iterator of file paths
pub(crate) fn last_modified_file(
    files: impl IntoIterator<Item = PathBuf>,
) -> Result<Option<SystemTime>> {
    Ok(files
        .into_iter()
        .unique()
        .filter(|p| p.exists())
        .map(|p| {
            p.metadata()
                .map_err(|err| eyre!("{}: {}", display_path(p), err))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|m| m.modified().map_err(|err| eyre!(err)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max())
}
