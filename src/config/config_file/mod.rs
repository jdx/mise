use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::Result;
use indexmap::IndexMap;

use rtxrc::RTXFile;
use tool_versions::ToolVersions;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgVersion};
use crate::config::Config;
use crate::config::PluginSource;
use crate::env;
use crate::errors::Error::VersionNotInstalled;
use crate::plugins::{Plugin, PluginName};
use crate::runtimes::RuntimeVersion;

pub mod legacy_version;
pub mod rtxrc;
pub mod tool_versions;

#[derive(Debug, PartialEq)]
pub enum ConfigFileType {
    RtxRc,
    ToolVersions,
    LegacyVersion,
}

pub trait ConfigFile: Debug + Display + Send {
    fn get_type(&self) -> ConfigFileType;
    fn get_path(&self) -> &Path;
    fn source(&self) -> PluginSource;
    fn plugins(&self) -> IndexMap<PluginName, Vec<String>>;
    fn env(&self) -> HashMap<String, String>;
    fn remove_plugin(&mut self, plugin_name: &PluginName);
    fn add_version(&mut self, plugin_name: &PluginName, version: &str);
    fn replace_versions(&mut self, plugin_name: &PluginName, versions: &[String]);
    fn save(&self) -> Result<()>;
    fn dump(&self) -> String;
}

impl dyn ConfigFile {
    pub fn add_runtimes(
        &mut self,
        config: &mut Config,
        runtimes: &[RuntimeArg],
        fuzzy: bool,
    ) -> Result<()> {
        let mut runtime_map: HashMap<PluginName, Vec<String>> = HashMap::new();
        for runtime in runtimes {
            let plugin = Plugin::load_ensure_installed(&runtime.plugin, &config.settings)?;
            let latest = config.resolve_runtime_arg(runtime)?;
            if let Some(latest) = latest {
                let rtv = RuntimeVersion::new(Arc::new(plugin), &latest);
                if !rtv.ensure_installed(config)? {
                    return Err(VersionNotInstalled(rtv.plugin.name.clone(), rtv.version))?;
                };
                runtime_map
                    .entry(rtv.plugin.name.clone())
                    .or_default()
                    .push(if fuzzy {
                        match runtime.version {
                            RuntimeArgVersion::Version(ref v) => v.to_string(),
                            _ => "latest".to_string(),
                        }
                    } else {
                        rtv.version.to_string()
                    });
            }
        }
        for (plugin, versions) in runtime_map {
            self.replace_versions(&plugin, &versions);
        }

        Ok(())
    }
}

pub fn init(path: &Path) -> Box<dyn ConfigFile> {
    if path.ends_with(".rtxrc") || path.ends_with(".rtxrc.toml") {
        return Box::new(RTXFile::init(path));
    } else if path.ends_with(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str()) {
        return Box::new(ToolVersions::init(path));
    }

    panic!("Unknown config file type: {}", path.display());
}

pub fn parse(path: &Path) -> Result<Box<dyn ConfigFile>> {
    match detect_config_file_type(path) {
        Some(ConfigFileType::RtxRc) => Ok(Box::new(RTXFile::from_file(path)?)),
        Some(ConfigFileType::ToolVersions) => Ok(Box::new(ToolVersions::from_file(path)?)),
        #[allow(clippy::box_default)]
        _ => Ok(Box::new(RTXFile::default())),
    }
}

fn detect_config_file_type(path: &Path) -> Option<ConfigFileType> {
    match path.file_name().unwrap().to_str().unwrap() {
        ".rtxrc" | ".rtxrc.toml" | "config.toml" => Some(ConfigFileType::RtxRc),
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
            detect_config_file_type(Path::new("/foo/bar/.rtxrc")),
            Some(ConfigFileType::RtxRc)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.rtxrc.toml")),
            Some(ConfigFileType::RtxRc)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.tool-versions")),
            Some(ConfigFileType::ToolVersions)
        );
        assert_eq!(
            detect_config_file_type(Path::new("/foo/bar/.tool-versions.toml")),
            None
        );
    }
}
