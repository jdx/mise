use std::collections::HashMap;
use std::default::Default;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;

use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::config::settings::SettingsBuilder;
use crate::config::{AliasMap, Settings};
use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolSource, ToolVersion, ToolVersionType, Toolset};

#[derive(Debug)]
pub struct LegacyVersionFile {
    path: PathBuf,
    toolset: Toolset,
}

impl LegacyVersionFile {
    pub fn parse(settings: &Settings, path: PathBuf, plugin: &Plugin) -> Result<Self> {
        let version = plugin.parse_legacy_file(path.as_path(), settings)?;

        Ok(Self {
            toolset: build_toolset(&path, plugin.name.as_str(), version.as_str()),
            path,
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

    fn plugins(&self) -> HashMap<PluginName, String> {
        Default::default()
    }

    fn env(&self) -> HashMap<String, String> {
        HashMap::new()
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

    fn settings(&self) -> SettingsBuilder {
        SettingsBuilder::default()
    }

    fn aliases(&self) -> AliasMap {
        AliasMap::default()
    }
}

impl Display for LegacyVersionFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LegacyVersionFile({})", self.path.display())
    }
}

fn build_toolset(path: &Path, plugin: &str, version: &str) -> Toolset {
    let mut toolset = Toolset::new(ToolSource::LegacyVersionFile(path.to_path_buf()));
    if !version.is_empty() {
        toolset.add_version(ToolVersion::new(
            plugin.to_string(),
            ToolVersionType::Version(version.to_string()),
        ));
    }
    toolset
}
