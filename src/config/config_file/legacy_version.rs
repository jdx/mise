use std::path::{Path, PathBuf};

use eyre::Result;

use crate::cli::args::ForgeArg;
use crate::config::config_file::ConfigFile;
use crate::forge::ForgeList;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};

#[derive(Debug)]
pub struct LegacyVersionFile {
    path: PathBuf,
    tools: ToolRequestSet,
}

impl LegacyVersionFile {
    pub fn parse(path: PathBuf, plugins: ForgeList) -> Result<Self> {
        let source = ToolSource::LegacyVersionFile(path.clone());
        let mut tools = ToolRequestSet::new();

        for plugin in plugins {
            let version = plugin.parse_legacy_file(&path)?;
            for version in version.split_whitespace() {
                let tr = ToolRequest::new(plugin.fa().clone(), version)?;
                tools.add_version(tr, &source);
            }
        }

        Ok(Self { tools, path })
    }
}

impl ConfigFile for LegacyVersionFile {
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

    fn to_tool_request_set(&self) -> Result<ToolRequestSet> {
        Ok(self.tools.clone())
    }
}
