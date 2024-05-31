use std::path::{Path, PathBuf};

use eyre::Result;

use crate::backend::BackendList;
use crate::cli::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};

#[derive(Debug)]
pub struct LegacyVersionFile {
    path: PathBuf,
    tools: ToolRequestSet,
}

impl LegacyVersionFile {
    pub fn parse(path: PathBuf, plugins: BackendList) -> Result<Self> {
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

    fn remove_plugin(&mut self, _fa: &BackendArg) -> Result<()> {
        unimplemented!()
    }

    fn replace_versions(&mut self, _plugin_name: &BackendArg, _versions: &[String]) -> Result<()> {
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
