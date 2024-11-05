use std::path::{Path, PathBuf};

use eyre::Result;

use crate::backend::{self, BackendList};
use crate::cli::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource, ToolVersionOptions};

#[derive(Debug, Clone)]
pub struct LegacyVersionFile {
    path: PathBuf,
    tools: ToolRequestSet,
}

impl LegacyVersionFile {
    pub fn init(path: PathBuf) -> Self {
        Self {
            path,
            tools: ToolRequestSet::new(),
        }
    }

    pub fn parse(path: PathBuf, plugins: BackendList) -> Result<Self> {
        let source = ToolSource::LegacyVersionFile(path.clone());
        let mut tools = ToolRequestSet::new();

        for plugin in plugins {
            let version = plugin.parse_legacy_file(&path)?;
            for version in version.split_whitespace() {
                let tr = ToolRequest::new(plugin.fa().clone(), version, source.clone())?;
                tools.add_version(tr, &source);
            }
        }

        Ok(Self { tools, path })
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        trace!("parsing legacy version: {}", path.display());
        let file_name = &path.file_name().unwrap().to_string_lossy().to_string();
        let tools = backend::list()
            .into_iter()
            .filter(|f| match f.legacy_filenames() {
                Ok(f) => f.contains(file_name),
                Err(_) => false,
            })
            .collect::<Vec<_>>();
        Self::parse(path.to_path_buf(), tools)
    }
}

impl ConfigFile for LegacyVersionFile {
    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn remove_plugin(&mut self, _fa: &BackendArg) -> Result<()> {
        unimplemented!()
    }

    fn replace_versions(
        &mut self,
        _plugin_name: &BackendArg,
        _versions: &[(String, ToolVersionOptions)],
    ) -> Result<()> {
        unimplemented!()
    }

    fn save(&self) -> Result<()> {
        unimplemented!()
    }

    fn dump(&self) -> Result<String> {
        unimplemented!()
    }

    fn source(&self) -> ToolSource {
        ToolSource::LegacyVersionFile(self.path.clone())
    }

    fn to_tool_request_set(&self) -> Result<ToolRequestSet> {
        Ok(self.tools.clone())
    }

    fn clone_box(&self) -> Box<dyn ConfigFile> {
        Box::new(self.clone())
    }
}
