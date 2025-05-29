use std::sync::Arc;

use crate::errors::Error;
use crate::toolset::tool_request::ToolRequest;
use crate::toolset::tool_version::ResolveOptions;
use crate::toolset::{ToolSource, ToolVersion};
use crate::{cli::args::BackendArg, config::Config};

/// represents several versions of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersionList {
    pub backend: Arc<BackendArg>,
    pub versions: Vec<ToolVersion>,
    pub requests: Vec<ToolRequest>,
    pub source: ToolSource,
}

impl ToolVersionList {
    pub fn new(backend: Arc<BackendArg>, source: ToolSource) -> Self {
        Self {
            backend,
            versions: Vec::new(),
            requests: vec![],
            source,
        }
    }
    pub async fn resolve(
        &mut self,
        config: &Arc<Config>,
        opts: &ResolveOptions,
    ) -> eyre::Result<()> {
        self.versions.clear();
        for tvr in &mut self.requests {
            match tvr.resolve(config, opts).await {
                Ok(v) => self.versions.push(v),
                Err(err) => {
                    return Err(Error::FailedToResolveVersion {
                        tr: Box::new(tvr.clone()),
                        ts: self.source.clone(),
                        source: err,
                    }
                    .into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::{dirs, env, file};

    use super::*;

    #[tokio::test]
    #[cfg(unix)]
    async fn test_tool_version_list() {
        let config = Config::get().await.unwrap();
        let ba: Arc<BackendArg> = Arc::new("tiny".into());
        let mut tvl = ToolVersionList::new(ba.clone(), ToolSource::Argument);
        tvl.requests
            .push(ToolRequest::new(ba, "latest", ToolSource::Argument).unwrap());
        tvl.resolve(
            &config,
            &ResolveOptions {
                latest_versions: true,
                use_locked_version: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(tvl.versions.len(), 1);
    }

    #[tokio::test]
    async fn test_tool_version_list_failure() {
        env::set_var("MISE_FAILURE", "1");
        file::remove_all(dirs::CACHE.join("dummy")).unwrap();
        let config = Config::reset().await.unwrap();
        let ba: Arc<BackendArg> = Arc::new("dummy".into());
        let mut tvl = ToolVersionList::new(ba.clone(), ToolSource::Argument);
        tvl.requests
            .push(ToolRequest::new(ba, "latest", ToolSource::Argument).unwrap());
        let _ = tvl
            .resolve(
                &config,
                &ResolveOptions {
                    latest_versions: true,
                    use_locked_version: false,
                },
            )
            .await;
        assert_eq!(tvl.versions.len(), 0);
        env::remove_var("MISE_FAILURE");
        Config::reset().await.unwrap();
    }
}
