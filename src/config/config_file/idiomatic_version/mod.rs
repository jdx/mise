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

        for plugin in plugins {
            match plugin.parse_idiomatic_file(&path).await {
                Ok(versions) => {
                    for v in versions {
                        let tr = ToolRequest::new(plugin.ba().clone(), &v, source.clone())?;
                        tools.add_version(tr, &source);
                    }
                }
                Err(e) => {
                    trace!("skipping {} for {}: {}", path.display(), plugin.id(), e);
                    continue;
                }
            }
        }

        Ok(Self { tools, path })
    }

    pub async fn from_file(path: &Path) -> Result<Self> {
        trace!("parsing idiomatic version: {}", path.display());
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut tools: Vec<Arc<dyn Backend>> = vec![];
        for b in backend::list().into_iter() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Backend, VersionInfo};
    use crate::cli::args::{BackendArg, BackendResolution};
    use crate::config::Config;
    use crate::install_context::InstallContext;
    use crate::toolset::ToolVersion;
    use async_trait::async_trait;
    use std::sync::Arc;

    #[derive(Debug)]
    struct MockBackend {
        ba: Arc<BackendArg>,
        fail: bool,
        version: Option<String>,
    }

    impl MockBackend {
        fn new(short: &str, fail: bool, version: Option<String>) -> Self {
            let ba = BackendArg::new_raw(
                short.to_string(),
                None,
                short.to_string(),
                None,
                BackendResolution::new(false),
            );
            Self {
                ba: Arc::new(ba),
                fail,
                version,
            }
        }
    }

    #[async_trait]
    impl Backend for MockBackend {
        fn ba(&self) -> &Arc<BackendArg> {
            &self.ba
        }

        async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
            Ok(vec![])
        }

        async fn install_version_(
            &self,
            _ctx: &InstallContext,
            _tv: ToolVersion,
        ) -> Result<ToolVersion> {
            unimplemented!()
        }

        async fn parse_idiomatic_file(&self, _path: &Path) -> Result<Vec<String>> {
            if self.fail {
                eyre::bail!("mock error");
            }
            if let Some(v) = &self.version {
                Ok(vec![v.clone()])
            } else {
                Ok(vec![])
            }
        }
    }

    #[tokio::test]
    async fn test_idiomatic_parse_error_propagation() {
        let _config = Config::get().await.unwrap();
        let path = PathBuf::from(".tool-versions");
        let backend1 = Arc::new(MockBackend::new("node", true, None));
        let backend2 = Arc::new(MockBackend::new(
            "python",
            false,
            Some("3.10.0".to_string()),
        ));
        let plugins: BackendList = vec![backend1, backend2];

        let result = IdiomaticVersionFile::parse(path, plugins).await;

        assert!(result.is_ok(), "Should not propagate error from backend1");

        let file = result.unwrap();
        let trs = file.to_tool_request_set().unwrap();
        let tools: Vec<_> = trs.into_iter().collect();
        assert_eq!(tools.len(), 1);
        let (ba, versions, _) = &tools[0];
        assert_eq!(ba.short, "python");
        assert_eq!(versions[0].version(), "3.10.0");
    }
}
