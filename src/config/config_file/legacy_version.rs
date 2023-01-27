use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use indexmap::IndexMap;

use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::plugins::{Plugin, PluginName, PluginSource};

#[derive(Debug)]
pub struct LegacyVersionFile {
    path: PathBuf,
    version: String,
    plugin: String,
}

impl LegacyVersionFile {
    pub fn parse(path: PathBuf, plugin: &Plugin) -> Result<Self> {
        let version = plugin.parse_legacy_file(path.as_path())?;

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

    fn source(&self) -> PluginSource {
        PluginSource::LegacyVersionFile(self.path.clone())
    }

    fn plugins(&self) -> IndexMap<PluginName, Vec<String>> {
        IndexMap::from([(self.plugin.clone(), vec![self.version.clone()])])
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
}

impl Display for LegacyVersionFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LegacyVersionFile({})", self.path.display())
    }
}
