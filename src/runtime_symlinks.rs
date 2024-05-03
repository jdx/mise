use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use versions::Versioning;

use crate::config::Config;
use crate::file::make_symlink;
use crate::forge::{forge_meta, Forge};
use crate::plugins::VERSION_REGEX;
use crate::{file, forge};

pub fn rebuild(config: &Config) -> Result<()> {
    for forge in forge::list() {
        let symlinks = list_symlinks(config, forge.clone())?;
        let installs_dir = &forge.fa().installs_path;
        for (from, to) in symlinks {
            let from = installs_dir.join(from);
            if from.exists() {
                if is_runtime_symlink(&from) && from.read_link()?.as_path() != to {
                    trace!("Removing existing symlink: {}", from.display());
                    file::remove_file(&from)?;
                } else {
                    continue;
                }
            }
            make_symlink(&to, &from)?;
        }
        remove_missing_symlinks(forge.clone())?;
        // remove install dir if empty (ignore metadata)
        file::remove_dir_ignore(
            installs_dir,
            vec![forge_meta::FORGE_META_FILENAME.to_string()],
        )?;
    }
    Ok(())
}

fn list_symlinks(config: &Config, forge: Arc<dyn Forge>) -> Result<IndexMap<String, PathBuf>> {
    // TODO: make this a pure function and add test cases
    let mut symlinks = IndexMap::new();
    let rel_path = |x: &String| PathBuf::from(".").join(x.clone());
    for v in installed_versions(&forge)? {
        let prefix = regex!(r"^[a-zA-Z0-9]+-")
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
        for (from, to) in config
            .get_all_aliases()
            .get(forge.fa())
            .unwrap_or(&BTreeMap::new())
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
    Ok(symlinks)
}

fn installed_versions(forge: &Arc<dyn Forge>) -> Result<Vec<String>> {
    let versions = forge
        .list_installed_versions()?
        .into_iter()
        .filter(|v| !VERSION_REGEX.is_match(v))
        .collect();
    Ok(versions)
}

fn remove_missing_symlinks(forge: Arc<dyn Forge>) -> Result<()> {
    let installs_dir = &forge.fa().installs_path;
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
    Ok(())
}

pub fn is_runtime_symlink(path: &Path) -> bool {
    if let Ok(link) = path.read_link() {
        return link.starts_with("./");
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::plugins::ExternalPlugin;

    use super::*;

    #[test]
    fn test_list_symlinks() {
        let config = Config::load().unwrap();
        let plugin = ExternalPlugin::new(String::from("tiny"));
        let plugin = Arc::new(plugin);
        let symlinks = list_symlinks(&config, plugin).unwrap();
        assert_debug_snapshot!(symlinks);
    }
}
