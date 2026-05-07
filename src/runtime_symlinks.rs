use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::Backend;
use crate::config::{Alias, Config};
use crate::file::make_symlink_or_file;
use crate::plugins::VERSION_REGEX;
use crate::semver::split_version_prefix;
use crate::toolset::Toolset;
use crate::{backend, env, file};
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use versions::Versioning;

pub async fn rebuild(config: &Config) -> Result<()> {
    for backend in backend::list() {
        for installs_dir in install_dirs_for(&backend) {
            rebuild_symlinks_in_dir(config, &backend, &installs_dir)?;
        }
    }
    Ok(())
}

pub async fn rebuild_for_toolset(config: &Config, ts: &Toolset) -> Result<()> {
    let mut backends = backend::list();
    for (backend, _) in ts.list_current_versions() {
        if !backends
            .iter()
            .any(|b| b.ba().installs_path == backend.ba().installs_path)
        {
            backends.push(backend);
        }
    }

    for backend in backends {
        for installs_dir in install_dirs_for(&backend) {
            rebuild_symlinks_in_dir(config, &backend, &installs_dir)?;
        }
    }
    Ok(())
}

pub async fn migrate_real_dirs(config: &Config) -> Result<()> {
    for backend in backend::list() {
        for installs_dir in install_dirs_for(&backend) {
            migrate_real_dirs_in_dir(config, &backend, &installs_dir)?;
        }
    }
    Ok(())
}

/// All install directories to consider for a backend: the backend's primary
/// installs_path plus any shared/system dirs that contain the tool. Per-dir
/// rebuilds are no-ops when desired state already matches actual state, so
/// dirs we have no write access to (read-only system installs) only error
/// out when we actually need to change something there.
fn install_dirs_for(backend: &Arc<dyn Backend>) -> Vec<PathBuf> {
    let ba = backend.ba();
    let mut dirs = vec![ba.installs_path.clone()];
    let tool_dir_name = ba.tool_dir_name();
    for shared_dir in env::shared_install_dirs() {
        let dir = shared_dir.join(&tool_dir_name);
        if dir.is_dir() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

fn rebuild_symlinks_in_dir(
    config: &Config,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> Result<()> {
    let concrete_installs = installed_versions_in_dir(installs_dir)
        .into_iter()
        .filter(|v| is_concrete_install(v))
        .collect::<std::collections::HashSet<_>>();
    let symlinks = list_symlinks_for_dir(config, backend, installs_dir);
    for (from, to) in symlinks {
        let from_name = from.clone();
        let from = installs_dir.join(from);
        if from.exists() {
            if is_runtime_symlink(&from) {
                // Existing runtime symlink: only rewrite if the target changed.
                if file::resolve_symlink(&from)?.unwrap_or_default() == to {
                    continue;
                }
                trace!("Removing existing symlink: {}", from.display());
                file::remove_file(&from)?;
            } else if from
                .file_name()
                .zip(to.file_name())
                .is_some_and(|(f, t)| f != t)
                && !concrete_installs.contains(&from_name)
            {
                // Real (non-symlink) directory at a runtime-symlink slot —
                // legacy stale state from the 2026.4 regression. Replace it.
                trace!("Replacing stale runtime dir: {}", from.display());
                file::remove_all(&from)?;
            } else {
                continue;
            }
        }
        make_symlink_or_file(&to, &from)?;
    }
    remove_missing_symlinks_in_dir(installs_dir)?;
    Ok(())
}

fn migrate_real_dirs_in_dir(
    config: &Config,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> Result<()> {
    let concrete_installs = installed_versions_in_dir(installs_dir)
        .into_iter()
        .filter(|v| is_concrete_install(v))
        .collect::<std::collections::HashSet<_>>();
    let symlinks = list_symlinks_for_dir(config, backend, installs_dir);
    for (from, to) in symlinks {
        let from_name = from.clone();
        let from = installs_dir.join(from);
        if !from.exists() || is_runtime_symlink(&from) || concrete_installs.contains(&from_name) {
            continue;
        }
        trace!("Replacing stale runtime dir: {}", from.display());
        file::remove_all(&from)?;
        make_symlink_or_file(&to, &from)?;
    }
    Ok(())
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
        if is_temporary_runtime_label(&v) {
            continue;
        }
        let (prefix, version) = split_version_prefix(&v);
        let Some(versions) = Versioning::new(version) else {
            continue;
        };
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

fn is_concrete_install(v: &str) -> bool {
    let (_, version) = split_version_prefix(v);
    version.chars().any(|c| c.is_ascii_digit()) && Versioning::new(version).is_some()
}

fn is_temporary_runtime_label(v: &str) -> bool {
    debug_assert!(
        {
            let remove_version = Versioning::new("2026.10.0").unwrap();
            *crate::cli::version::V < remove_version
        },
        "Temporary runtime symlink migration guard should be removed in version 2026.10.0."
    );
    // The 2026.4 runtime symlink regression created real "latest" dirs. Treat
    // only that literal label as generated state: numeric prefixes like "25"
    // may be concrete installs requested by users and must not be migrated.
    v == "latest"
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
