use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::Backend;
use crate::config::{Alias, Config};
use crate::file::make_symlink_or_file;
use crate::plugins::VERSION_REGEX;
use crate::semver::split_version_prefix;
use crate::{backend, env, file};
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use versions::Versioning;

pub async fn rebuild(config: &Config) -> Result<()> {
    for backend in backend::list() {
        let ba = backend.ba();
        // Collect all install directories for this backend: user dir + shared/system dirs
        let mut installs_dirs = vec![ba.installs_path.clone()];
        let tool_dir_name = ba.tool_dir_name();
        for shared_dir in env::shared_install_dirs() {
            let dir = shared_dir.join(&tool_dir_name);
            if dir.is_dir() && !installs_dirs.contains(&dir) {
                installs_dirs.push(dir);
            }
        }

        // Process user dir (first entry) with normal error propagation
        if let Some(installs_dir) = installs_dirs.first() {
            rebuild_symlinks_in_dir(config, &backend, installs_dir)?;
        }
        // Process shared/system dirs with permission error tolerance
        for installs_dir in installs_dirs.iter().skip(1) {
            if let Err(e) = rebuild_symlinks_in_dir(config, &backend, installs_dir) {
                if is_permission_error(&e) {
                    warn!(
                        "skipping symlink update for {}: {}",
                        installs_dir.display(),
                        e
                    );
                } else {
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}

fn rebuild_symlinks_in_dir(
    config: &Config,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> Result<()> {
    let symlinks = list_symlinks_for_dir(config, backend, installs_dir);
    for (from, to) in symlinks {
        let from = installs_dir.join(from);
        if from.exists() {
            if is_runtime_symlink(&from) && file::resolve_symlink(&from)?.unwrap_or_default() != to
            {
                trace!("Removing existing symlink: {}", from.display());
                file::remove_file(&from)?;
            } else {
                continue;
            }
        }
        make_symlink_or_file(&to, &from)?;
    }
    remove_missing_symlinks_in_dir(installs_dir)?;
    Ok(())
}

fn is_permission_error(e: &eyre::Report) -> bool {
    e.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_err| {
                matches!(
                    io_err.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
                )
            })
    })
}

/// Build symlinks for versions found in a specific install directory.
fn list_symlinks_for_dir(
    config: &Config,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> IndexMap<String, PathBuf> {
    let mut symlinks = IndexMap::new();
    let rel_path = |x: &String| PathBuf::from(".").join(x.clone());
    for v in installed_versions_in_dir(installs_dir) {
        let (prefix, version) = split_version_prefix(&v);
        let versions = Versioning::new(version).unwrap_or_default();
        let mut partial = vec![];
        while versions.nth(partial.len()).is_some() && versions.nth(partial.len() + 1).is_some() {
            let version = versions.nth(partial.len()).unwrap();
            partial.push(version.to_string());
            let from = format!("{}{}", prefix, partial.join("."));
            symlinks.insert(from, rel_path(&v));
        }
        symlinks.insert(format!("{prefix}latest"), rel_path(&v));
        for (from, to) in &config
            .all_aliases
            .get(&backend.ba().short)
            .unwrap_or(&Alias::default())
            .versions
        {
            if from.contains('/') {
                continue;
            }
            if !v.starts_with(to) {
                continue;
            }
            symlinks.insert(format!("{prefix}{from}"), rel_path(&v));
        }
    }
    symlinks = symlinks
        .into_iter()
        .sorted_by_cached_key(|(k, _)| (Versioning::new(k), k.to_string()))
        .collect();
    symlinks
}

/// List real (non-symlink) installed versions in a specific directory.
fn installed_versions_in_dir(installs_dir: &Path) -> Vec<String> {
    if !installs_dir.is_dir() {
        return vec![];
    }
    file::dir_subdirs(installs_dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|v| !v.starts_with('.'))
        .filter(|v| !is_runtime_symlink(&installs_dir.join(v)))
        .filter(|v| !installs_dir.join(v).join("incomplete").exists())
        .filter(|v| !VERSION_REGEX.is_match(v))
        .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
        .collect()
}

pub fn remove_missing_symlinks(backend: Arc<dyn Backend>) -> Result<()> {
    remove_missing_symlinks_in_dir(&backend.ba().installs_path)
}

fn remove_missing_symlinks_in_dir(installs_dir: &Path) -> Result<()> {
    if !installs_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(installs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if is_runtime_symlink(&path) && !path.exists() {
            trace!("Removing missing symlink: {}", path.display());
            file::remove_file(path)?;
        }
    }
    // remove install dir if empty (ignore metadata)
    file::remove_dir_ignore(installs_dir, vec![".mise.backend.json", ".mise.backend"])?;
    Ok(())
}

pub fn is_runtime_symlink(path: &Path) -> bool {
    if let Ok(Some(link)) = file::resolve_symlink(path) {
        return link.starts_with("./");
    }
    false
}
