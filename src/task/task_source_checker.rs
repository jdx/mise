use crate::config::Config;
use crate::dirs;
use crate::file::{self, display_path};
use crate::hash;
use crate::task::Task;
use eyre::{Result, eyre};
use glob::glob;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

/// Get the working directory for a task
pub async fn task_cwd(task: &Task, config: &Arc<Config>) -> Result<PathBuf> {
    if let Some(d) = task.dir(config).await? {
        Ok(d)
    } else {
        Ok(config
            .project_root
            .clone()
            .or_else(|| dirs::CWD.clone())
            .unwrap_or_default())
    }
}

/// Check if task sources are up to date (fresher than outputs)
pub async fn sources_are_fresh(task: &Task, config: &Arc<Config>) -> Result<bool> {
    if task.sources.is_empty() {
        return Ok(false);
    }
    // TODO: We should benchmark this and find out if it might be possible to do some caching around this or something
    // perhaps using some manifest in a state directory or something, maybe leveraging atime?
    let run = async || -> Result<bool> {
        let root = task_cwd(task, config).await?;
        let mut sources = task.sources.clone();
        sources.push(task.config_source.to_string_lossy().to_string());
        let source_metadatas = get_file_metadatas(&root, &sources)?;
        let source_metadata_hash = file_metadatas_to_hash(&source_metadatas);
        let source_metadata_hash_path = sources_hash_path(task);
        if let Some(dir) = source_metadata_hash_path.parent() {
            file::create_dir_all(dir)?;
        }
        if source_metadata_existing_hash(task).is_some_and(|h| h != source_metadata_hash) {
            debug!(
                "source metadata hash mismatch in {}",
                source_metadata_hash_path.display()
            );
            file::write(&source_metadata_hash_path, &source_metadata_hash)?;
            return Ok(false);
        }
        let sources = get_last_modified_from_metadatas(&source_metadatas);
        let outputs = get_last_modified(&root, &task.outputs.paths(task))?;
        file::write(&source_metadata_hash_path, &source_metadata_hash)?;
        trace!("sources: {sources:?}, outputs: {outputs:?}");
        match (sources, outputs) {
            (Some(sources), Some(outputs)) => Ok(sources < outputs),
            _ => Ok(false),
        }
    };
    Ok(run().await.unwrap_or_else(|err| {
        warn!("sources_are_fresh: {err:?}");
        false
    }))
}

/// Save a checksum file after a task completes successfully
pub fn save_checksum(task: &Task) -> Result<()> {
    if task.sources.is_empty() {
        return Ok(());
    }
    if task.outputs.is_auto() {
        for p in task.outputs.paths(task) {
            debug!("touching auto output file: {p}");
            file::touch_file(&PathBuf::from(&p))?;
        }
    }
    Ok(())
}

/// Get the path to store source hashes for a task
fn sources_hash_path(task: &Task) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    task.hash(&mut hasher);
    task.config_source.hash(&mut hasher);
    let hash = format!("{:x}", hasher.finish());
    dirs::STATE.join("task-sources").join(&hash)
}

/// Get the existing source hash for a task, if it exists
fn source_metadata_existing_hash(task: &Task) -> Option<String> {
    let path = sources_hash_path(task);
    if path.exists() {
        Some(file::read_to_string(&path).unwrap_or_default())
    } else {
        None
    }
}

/// Get file metadata for a list of patterns or paths
fn get_file_metadatas(
    root: &Path,
    patterns_or_paths: &[String],
) -> Result<Vec<(PathBuf, fs::Metadata)>> {
    if patterns_or_paths.is_empty() {
        return Ok(vec![]);
    }
    let (patterns, paths): (Vec<&String>, Vec<&String>) =
        patterns_or_paths.iter().partition(|p| is_glob_pattern(p));

    let mut metadatas = BTreeMap::new();
    for pattern in patterns {
        let files = glob(root.join(pattern).to_str().unwrap())?;
        for file in files.flatten() {
            if let Ok(metadata) = file.metadata() {
                metadatas.insert(file, metadata);
            }
        }
    }

    for path in paths {
        let file = root.join(path);
        if let Ok(metadata) = file.metadata() {
            metadatas.insert(file, metadata);
        }
    }

    let metadatas = metadatas
        .into_iter()
        .filter(|(_, m)| m.is_file())
        .collect_vec();

    Ok(metadatas)
}

/// Convert file metadata to a hash string for comparison
fn file_metadatas_to_hash(metadatas: &[(PathBuf, fs::Metadata)]) -> String {
    let paths: Vec<_> = metadatas.iter().map(|(p, _)| p).collect();
    hash::hash_to_str(&paths)
}

/// Get the last modified time from file metadata
fn get_last_modified_from_metadatas(metadatas: &[(PathBuf, fs::Metadata)]) -> Option<SystemTime> {
    metadatas.iter().flat_map(|(_, m)| m.modified()).max()
}

/// Get the last modified time from a list of patterns or paths
fn get_last_modified(root: &Path, patterns_or_paths: &[String]) -> Result<Option<SystemTime>> {
    if patterns_or_paths.is_empty() {
        return Ok(None);
    }
    let (patterns, paths): (Vec<&String>, Vec<&String>) =
        patterns_or_paths.iter().partition(|p| is_glob_pattern(p));

    let last_mod = std::cmp::max(
        last_modified_glob_match(root, &patterns)?,
        last_modified_path(root, &paths)?,
    );

    trace!(
        "last_modified of {}: {last_mod:?}",
        patterns_or_paths.iter().join(" ")
    );
    Ok(last_mod)
}
