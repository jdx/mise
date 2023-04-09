use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use regex::Regex;
use versions::Version;

use crate::config::Config;
use crate::dirs;
use crate::file::make_symlink;
use crate::tool::Tool;

pub fn rebuild_symlinks(config: &Config) -> Result<()> {
    for plugin in config.tools.values() {
        remove_existing_symlinks(plugin)?;
        let symlinks = list_symlinks(config, plugin)?;
        let installs_dir = dirs::INSTALLS.join(&plugin.name);
        for (from, to) in symlinks {
            let from = installs_dir.join(from);
            if from.exists() {
                continue;
            }
            make_symlink(&to, &from)?;
        }
    }
    Ok(())
}

fn list_symlinks(config: &Config, plugin: &Tool) -> Result<IndexMap<String, PathBuf>> {
    let mut symlinks = IndexMap::new();
    let rel_path = |x: &String| PathBuf::from(".").join(x.clone());
    for v in installed_versions(plugin)? {
        let version = Version::new(&v).unwrap();
        if version.chunks.0.len() > 1 {
            let chunks = &version.chunks.0[0..=version.chunks.0.len() - 2];
            for (i, _) in chunks.iter().enumerate() {
                let partial = version.chunks.0[0..=i]
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(".");
                symlinks.insert(partial, rel_path(&v));
            }
        }
        symlinks.insert("latest".into(), rel_path(&v));
        for (from, to) in config
            .get_all_aliases()
            .get(&plugin.name)
            .unwrap_or(&BTreeMap::new())
        {
            if from.contains('/') {
                continue;
            }
            if !v.starts_with(to) {
                continue;
            }
            symlinks.insert(from.clone(), rel_path(&v));
        }
    }
    symlinks = symlinks
        .into_iter()
        .sorted_by_cached_key(|(k, _)| Version::new(k).unwrap_or_default())
        .collect();
    Ok(symlinks)
}

fn installed_versions(plugin: &Tool) -> Result<Vec<String>> {
    let re: &Regex = regex!(r"^\d+(\.\d+)?(\.\d+)?$");
    let versions = plugin
        .list_installed_versions()?
        .into_iter()
        .filter(|v| re.is_match(v))
        .collect();
    Ok(versions)
}

fn remove_existing_symlinks(plugin: &Tool) -> Result<()> {
    let installs_dir = dirs::INSTALLS.join(&plugin.name);
    if !installs_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(installs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if is_runtime_symlink(&path) {
            trace!("Removing existing symlink: {}", path.display());
            std::fs::remove_file(path)?;
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
    use insta::assert_debug_snapshot;

    use crate::config::Config;
    use crate::plugins::{ExternalPlugin, PluginName};

    use super::*;

    #[test]
    fn test_list_symlinks() {
        let config = Config::load().unwrap();
        let plugin = ExternalPlugin::new(&config.settings, &PluginName::from("tiny"));
        let tool = Tool::new(String::from("tiny"), Box::new(plugin));
        let symlinks = list_symlinks(&config, &tool).unwrap();
        assert_debug_snapshot!(symlinks);
    }
}
