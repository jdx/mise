use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::Result;

use crate::backend::{self, Backend, BackendList};
use crate::cli::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};

use super::ConfigFileType;

#[derive(Debug, Clone)]
pub struct IdiomaticVersionFile {
    path: PathBuf,
    tools: ToolRequestSet,
}

impl IdiomaticVersionFile {
    pub fn init(path: PathBuf) -> Self {
        Self {
            path,
            tools: ToolRequestSet::new(),
        }
    }

    pub async fn parse(path: PathBuf, plugins: BackendList) -> Result<Self> {
        let source = ToolSource::IdiomaticVersionFile(path.clone());
        let mut tools = ToolRequestSet::new();

        for plugin in plugins {
            let version = plugin.parse_idiomatic_file(&path).await?;
            for version in version.split_whitespace() {
                let tr = ToolRequest::new(plugin.ba().clone(), version, source.clone())?;
                tools.add_version(tr, &source);
            }
        }

        Ok(Self { tools, path })
    }

    pub async fn from_file(path: &Path) -> Result<Self> {
        trace!("parsing idiomatic version: {}", path.display());
        let file_name = &path.file_name().unwrap().to_string_lossy().to_string();
        let mut tools: Vec<Arc<dyn Backend>> = vec![];
        for b in backend::list().into_iter() {
            if b.idiomatic_filenames()
                .await
                .is_ok_and(|f| f.contains(file_name))
            {
                tools.push(b);
            }
        }
        Self::parse(path.to_path_buf(), tools).await
    }
}

impl ConfigFile for IdiomaticVersionFile {
    fn config_type(&self) -> ConfigFileType {
        ConfigFileType::IdiomaticVersion
    }

    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn remove_tool(&self, _fa: &BackendArg) -> Result<()> {
        unimplemented!()
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn replace_versions(
        &self,
        _plugin_name: &BackendArg,
        _versions: Vec<ToolRequest>,
    ) -> Result<()> {
        unimplemented!()
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn save(&self) -> Result<()> {
        unimplemented!()
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn dump(&self) -> Result<String> {
        unimplemented!()
    }

    fn source(&self) -> ToolSource {
        ToolSource::IdiomaticVersionFile(self.path.clone())
    }

    fn to_tool_request_set(&self) -> Result<ToolRequestSet> {
        Ok(self.tools.clone())
    }
}
