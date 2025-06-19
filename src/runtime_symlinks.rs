use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::Backend;
use crate::config::{Alias, Config};
use crate::file::make_symlink_or_file;
use crate::plugins::VERSION_REGEX;
use crate::{backend, file};
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use versions::Versioning;
use xx::regex;

pub async fn rebuild(config: &Config) -> Result<()> {
    for backend in backend::list() {
        let symlinks = list_symlinks(config, backend.clone());
        let installs_dir = &backend.ba().installs_path;
        for (from, to) in symlinks {
            let from = installs_dir.join(from);
            if from.exists() {
                if is_runtime_symlink(&from)
                    && file::resolve_symlink(&from)?.unwrap_or_default() != to
                {
                    trace!("Removing existing symlink: {}", from.display());
                    file::remove_file(&from)?;
                } else {
                    continue;
                }
            }
            make_symlink_or_file(&to, &from)?;
        }
        remove_missing_symlinks(backend.clone())?;
    }
    Ok(())
}

fn list_symlinks(config: &Config, backend: Arc<dyn Backend>) -> IndexMap<String, PathBuf> {
    // TODO: make this a pure function and add test cases
    let mut symlinks = IndexMap::new();
    let rel_path = |x: &String| PathBuf::from(".").join(x.clone());
    let re = regex!(r"^[a-zA-Z0-9]+-");
    for v in installed_versions(&backend) {
        let prefix = re
            .find(&v)
            .map(|s| s.as_str().to_string())
            .unwrap_or_default();
        let sans_prefix = v.trim_start_matches(&prefix);
        let versions = Versioning::new(sans_prefix).unwrap_or_default();
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

fn installed_versions(backend: &Arc<dyn Backend>) -> Vec<String> {
    backend
        .list_installed_versions()
        .into_iter()
        .filter(|v| !VERSION_REGEX.is_match(v))
        .collect()
}

pub fn remove_missing_symlinks(backend: Arc<dyn Backend>) -> Result<()> {
    let installs_dir = &backend.ba().installs_path;
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
