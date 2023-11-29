use std::collections::HashMap;
use std::fmt::{Debug, Display};

use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};

use tool_versions::ToolVersions;

use crate::cli::args::tool::ToolArg;
use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::settings::SettingsBuilder;
use crate::config::{AliasMap, Config, Settings};
use crate::file::{display_path, replace_path};
use crate::hash::hash_to_str;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::toolset::{ToolVersion, ToolVersionList, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{dirs, env, file};

pub mod legacy_version;
pub mod rtx_toml;
pub mod tool_versions;

#[derive(Debug, PartialEq)]
pub enum ConfigFileType {
    RtxToml,
    ToolVersions,
    LegacyVersion,
}

pub trait ConfigFile: Debug + Display + Send + Sync {
    fn get_type(&self) -> ConfigFileType;
    fn get_path(&self) -> &Path;
    fn plugins(&self) -> HashMap<PluginName, String>;
    fn env(&self) -> HashMap<String, String>;
    fn env_remove(&self) -> Vec<String> {
        vec![]
    }
    fn path_dirs(&self) -> Vec<PathBuf>;
    fn remove_plugin(&mut self, plugin_name: &PluginName);
    fn replace_versions(&mut self, plugin_name: &PluginName, versions: &[String]);
    fn save(&self) -> Result<()>;
    fn dump(&self) -> String;
    fn to_toolset(&self) -> &Toolset;
    fn settings(&self) -> SettingsBuilder;
    fn aliases(&self) -> AliasMap;
    fn watch_files(&self) -> Vec<PathBuf>;

    fn is_global(&self) -> bool {
        false
    }
}

impl dyn ConfigFile {
    pub fn add_runtimes(
        &mut self,
        config: &mut Config,
        runtimes: &[ToolArg],
        pin: bool,
    ) -> Result<()> {
        // TODO: this has become a complete mess and could probably be greatly simplified
        let mpr = MultiProgressReport::new(config.show_progress_bars());
        let mut ts = self.to_toolset().to_owned();
        ts.latest_versions = true;
        ts.resolve(config);
        let mut plugins_to_update = HashMap::new();
        for runtime in runtimes {
            if let Some(tv) = &runtime.tvr {
                plugins_to_update
                    .entry(runtime.plugin.clone())
                    .or_insert_with(Vec::new)
                    .push(tv);
            }
        }
        for (plugin, versions) in &plugins_to_update {
            let mut tvl =
                ToolVersionList::new(plugin.to_string(), ts.source.as_ref().unwrap().clone());
            for tv in versions {
                tvl.requests.push(((*tv).clone(), Default::default()));
            }
            ts.versions.insert(plugin.clone(), tvl);
        }
        ts.resolve(config);
        let versions: Vec<ToolVersion> = plugins_to_update
            .iter()
            .flat_map(|(pn, _)| ts.versions.get(pn).unwrap().versions.clone())
            .collect();
        ts.install_versions(config, versions, &mpr, false)?;
        for (plugin, versions) in plugins_to_update {
            let versions = versions
                .into_iter()
                .map(|tvr| {
                    if pin {
                        let plugin = config.tools.get(&plugin).unwrap();
                        let tv =
                            tvr.resolve(config, plugin, Default::default(), ts.latest_versions)?;
                        Ok(tv.version)
                    } else {
                        Ok(tvr.version())
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            self.replace_versions(&plugin, &versions);
        }

        Ok(())
    }

    /// this is for `rtx local|global TOOL` which will display the version instead of setting it
    /// it's only valid to use a single tool in this case
    /// returns "true" if the tool was displayed which means the CLI should exit
    pub fn display_runtime(&self, out: &mut Output, runtimes: &[ToolArg]) -> Result<bool> {
        // in this situation we just print the current version in the config file
        if runtimes.len() == 1 && runtimes[0].tvr.is_none() {
            let plugin = &runtimes[0].plugin;
            let tvl = self
                .to_toolset()
                .versions
                .get(plugin)
                .ok_or_else(|| {
                    eyre!(
                        "no version set for {} in {}",
                        plugin.to_string(),
                        display_path(self.get_path())
                    )
                })?
                .requests
                .iter()
                .map(|(tvr, _)| tvr.version())
                .collect::<Vec<_>>();
            rtxprintln!(out, "{}", tvl.join(" "));
            return Ok(true);
        }
        // check for something like `rtx local node python@latest` which is invalid
        if runtimes.iter().any(|r| r.tvr.is_none()) {
            return Err(eyre!("invalid input, specify a version for each tool. Or just specify one tool to print the current version"));
        }
        Ok(false)
    }
}

pub fn init(path: &Path, is_trusted: bool) -> Box<dyn ConfigFile> {
    match detect_config_file_type(path) {
        Some(ConfigFileType::RtxToml) => Box::new(RtxToml::init(path, is_trusted)),
        Some(ConfigFileType::ToolVersions) => Box::new(ToolVersions::init(path, is_trusted)),
        _ => panic!("Unknown config file type: {}", path.display()),
    }
}

pub fn parse(path: &Path, is_trusted: bool) -> Result<Box<dyn ConfigFile>> {
    match detect_config_file_type(path) {
        Some(ConfigFileType::RtxToml) => Ok(Box::new(RtxToml::from_file(path, is_trusted)?)),
        Some(ConfigFileType::ToolVersions) => {
            Ok(Box::new(ToolVersions::from_file(path, is_trusted)?))
        }
        #[allow(clippy::box_default)]
        _ => Ok(Box::new(RtxToml::default())),
    }
}

pub fn is_trusted(settings: &Settings, path: &Path) -> bool {
    if settings
        .trusted_config_paths
        .iter()
        .any(|p| path.starts_with(replace_path(p)))
    {
        return true;
    }
    match path.canonicalize() {
        Ok(path) => trust_path(&path).exists(),
        Err(_) => false,
    }
}

pub fn trust(path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    let hashed_path = trust_path(&path);
    if !hashed_path.exists() {
        file::create_dir_all(hashed_path.parent().unwrap())?;
        file::make_symlink(&path, &hashed_path)?;
    }
    Ok(())
}

pub fn untrust(path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    let hashed_path = trust_path(&path);
    if hashed_path.exists() {
        file::remove_file(hashed_path)?;
    }
    Ok(())
}

fn trust_path(path: &Path) -> PathBuf {
    dirs::ROOT.join("trusted-configs").join(hash_to_str(&path))
}

fn detect_config_file_type(path: &Path) -> Option<ConfigFileType> {
    match path.file_name().unwrap().to_str().unwrap() {
        f if f.ends_with(".toml") => Some(ConfigFileType::RtxToml),
        f if env::RTX_DEFAULT_CONFIG_FILENAME.as_str() == f => Some(ConfigFileType::RtxToml),
        f if env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str() == f => {
            Some(ConfigFileType::ToolVersions)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_config_file_type() {
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.test-tool-versions")),
            Some(ConfigFileType::ToolVersions)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.tool-versions.toml")),
            Some(ConfigFileType::RtxToml)
        );
    }
}
