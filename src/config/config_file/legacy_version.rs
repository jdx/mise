use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use indexmap::IndexMap;

use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::config::Settings;
use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolSource, ToolVersion, ToolVersionType, Toolset};

#[derive(Debug)]
pub struct LegacyVersionFile {
    path: PathBuf,
    version: String,
    plugin: String,
}

impl LegacyVersionFile {
    pub fn parse(settings: &Settings, path: PathBuf, plugin: &Plugin) -> Result<Self> {
        let version = plugin.parse_legacy_file(path.as_path(), settings)?;

        Ok(Self {
            path,
            version,
            plugin: plugin.name.clone(),
        })
    }
}

impl ConfigFile for LegacyVersionFile {
    fn get_type(&self) -> ConfigFileType {
        ConfigFileType::LegacyVersion
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn plugins(&self) -> IndexMap<PluginName, Vec<String>> {
        if self.version.is_empty() {
            IndexMap::new()
        } else {
            IndexMap::from([(self.plugin.clone(), vec![self.version.clone()])])
        }
    }

    fn env(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn remove_plugin(&mut self, _plugin_name: &PluginName) {
        unimplemented!()
    }

    fn add_version(&mut self, _plugin_name: &PluginName, _version: &str) {
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

    fn to_toolset(&self) -> Toolset {
        self.into()
    }
}

impl Display for LegacyVersionFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LegacyVersionFile({})", self.path.display())
    }
}

impl From<&LegacyVersionFile> for Toolset {
    fn from(value: &LegacyVersionFile) -> Self {
        let mut toolset = Toolset::new(ToolSource::LegacyVersionFile(value.path.clone()));
        if !value.version.is_empty() {
            toolset.add_version(
                value.plugin.clone(),
                ToolVersion::new(
                    value.plugin.clone(),
                    ToolVersionType::Version(value.version.clone()),
                ),
            );
        }
        toolset
    }
}
