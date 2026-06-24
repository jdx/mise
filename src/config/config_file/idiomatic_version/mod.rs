use std::path::{Path, PathBuf};

use eyre::{Result, eyre};

use crate::backend::BackendList;
use crate::cli::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::file;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};

pub mod package_json;

#[derive(Debug, Clone)]
pub struct IdiomaticVersionFile {
    path: PathBuf,
    tools: ToolRequestSet,
}

impl IdiomaticVersionFile {
    #[allow(dead_code)]
    #[cfg(test)]
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
            match plugin.parse_idiomatic_file_with_options(&path).await {
                Ok(versions) => {
                    for (version, options) in versions {
                        let mut tr =
                            ToolRequest::new(plugin.ba().clone(), &version, source.clone())?;
                        tr.set_options(options);
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

    fn read_only_error(&self, action: &str) -> eyre::Report {
        eyre!(
            "cannot {action} idiomatic version file {}; use mise.toml, .tool-versions, or --path to choose a writable config file",
            file::display_path(&self.path)
        )
    }
}

impl ConfigFile for IdiomaticVersionFile {
    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn remove_tool(&self, _fa: &BackendArg) -> Result<()> {
        Err(self.read_only_error("remove tools from"))
    }

    fn replace_versions(
        &self,
        _plugin_name: &BackendArg,
        _versions: Vec<ToolRequest>,
    ) -> Result<()> {
        Err(self.read_only_error("update"))
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn save(&self) -> Result<()> {
        Ok(())
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn dump(&self) -> Result<String> {
        file::read_to_string(&self.path)
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

    #[test]
    fn test_idiomatic_mutations_return_errors() {
        let file = IdiomaticVersionFile::init(PathBuf::from("package.json"));
        let ba = MockBackend::new("node", false, None).ba;

        let err = file.replace_versions(&ba, vec![]).unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot update idiomatic version file")
        );

        let err = file.remove_tool(&ba).unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot remove tools from idiomatic version file")
        );
    }
}
