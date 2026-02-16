use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::Result;

use crate::backend::{self, Backend, BackendList};
use crate::cli::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};

use super::ConfigFileType;

pub mod package_json;

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

        let add_version =
            |tools: &mut ToolRequestSet, plugin: &Arc<dyn Backend>, version: &str| -> Result<()> {
                let tr = ToolRequest::new(plugin.ba().clone(), version, source.clone())?;
                tools.add_version(tr, &source);
                Ok(())
            };

        for plugin in plugins {
            if path.file_name().is_some_and(|f| f == "package.json") {
                let versions = package_json::parse(&path, plugin.id())?;
                for v in versions {
                    add_version(&mut tools, &plugin, &v)?;
                }
                continue;
            }

            let versions = plugin.parse_idiomatic_file(&path).await?;
            if !versions.is_empty() {
                for v in versions {
                    add_version(&mut tools, &plugin, &v)?;
                }
                continue;
            }
            let body = crate::file::read_to_string(&path).unwrap_or_default();
            let body = body.trim();
            if !body.is_empty() {
                for v in body.split_whitespace() {
                    add_version(&mut tools, &plugin, v)?;
                }
            }
        }

        Ok(Self { tools, path })
    }

    pub async fn from_file(path: &Path) -> Result<Self> {
        trace!("parsing idiomatic version: {}", path.display());
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut tools: Vec<Arc<dyn Backend>> = vec![];
        let enable_tools = crate::config::Settings::get()
            .idiomatic_version_file_enable_tools
            .clone();
        for b in backend::list().into_iter() {
            if !enable_tools.contains(b.id()) {
                continue;
            }

            if b.idiomatic_filenames()
                .await
                .is_ok_and(|f| f.contains(&file_name))
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
