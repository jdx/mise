use std::default::Default;
use std::path::{Path, PathBuf};

use eyre::Result;

use crate::cli::args::ForgeArg;
use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::forge::ForgeList;
use crate::toolset::{ToolSource, ToolVersionRequest, Toolset};

#[derive(Debug)]
pub struct LegacyVersionFile {
    path: PathBuf,
    toolset: Toolset,
}

impl LegacyVersionFile {
    pub fn parse(path: PathBuf, plugins: ForgeList) -> Result<Self> {
        let mut toolset = Toolset::new(ToolSource::LegacyVersionFile(path.clone()));

        for plugin in plugins {
            let version = plugin.parse_legacy_file(&path)?;
            for version in version.split_whitespace() {
                toolset.add_version(
                    ToolVersionRequest::new(plugin.fa().clone(), version),
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

    fn remove_plugin(&mut self, _fa: &ForgeArg) -> Result<()> {
        unimplemented!()
    }

    fn replace_versions(&mut self, _plugin_name: &ForgeArg, _versions: &[String]) -> Result<()> {
        unimplemented!()
    }

    fn save(&self) -> Result<()> {
        unimplemented!()
    }

    fn dump(&self) -> Result<String> {
        unimplemented!()
    }

    fn to_toolset(&self) -> &Toolset {
        &self.toolset
    }
}
