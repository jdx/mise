use std::default::Default;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use miette::Result;

use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolSource, ToolVersionRequest, Toolset};

#[derive(Debug)]
pub struct LegacyVersionFile {
    path: PathBuf,
    toolset: Toolset,
}

impl LegacyVersionFile {
    pub fn parse(path: PathBuf, plugins: &[&Arc<dyn Plugin>]) -> Result<Self> {
        let mut toolset = Toolset::new(ToolSource::LegacyVersionFile(path.clone()));

        for plugin in plugins {
            let version = plugin.parse_legacy_file(&path)?;
            for version in version.split_whitespace() {
                toolset.add_version(
                    ToolVersionRequest::new(plugin.name().to_string(), version),
                    Default::default(),
                );
            }
        }

        Ok(Self { toolset, path })
    }
}

impl ConfigFile for LegacyVersionFile {
    fn get_type(&self) -> ConfigFileType {
        ConfigFileType::LegacyVersion
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn remove_plugin(&mut self, _plugin_name: &PluginName) {
        unimplemented!()
    }

    fn replace_versions(&mut self, _plugin_name: &PluginName, _versions: &[String]) {
        unimplemented!()
    }

    fn save(&self) -> Result<()> {
        unimplemented!()
    }

    fn dump(&self) -> String {
        unimplemented!()
    }

    fn to_toolset(&self) -> &Toolset {
        &self.toolset
    }
}
