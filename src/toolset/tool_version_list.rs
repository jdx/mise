use crate::backend;
use crate::cli::args::BackendArg;
use crate::errors::Error;
use crate::toolset::tool_version_request::ToolRequest;
use crate::toolset::{ToolSource, ToolVersion};

/// represents several versions of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersionList {
    pub backend: BackendArg,
    pub versions: Vec<ToolVersion>,
    pub requests: Vec<ToolRequest>,
    pub source: ToolSource,
}

impl ToolVersionList {
    pub fn new(backend: BackendArg, source: ToolSource) -> Self {
        Self {
            backend,
            versions: Vec::new(),
            requests: vec![],
            source,
        }
    }
    pub fn resolve(&mut self, latest_versions: bool) -> eyre::Result<()> {
        self.versions.clear();
        let plugin = backend::get(&self.backend);
        for tvr in &mut self.requests {
            match tvr.resolve(plugin.as_ref(), latest_versions) {
                Ok(v) => self.versions.push(v),
                Err(err) => {
                    return Err(Error::FailedToResolveVersion {
                        tr: tvr.clone(),
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

    use crate::test::reset;
    use crate::{dirs, env, file};

    use super::*;

    #[test]
    fn test_tool_version_list() {
        reset();
        let fa: BackendArg = "tiny".into();
        let mut tvl = ToolVersionList::new(fa.clone(), ToolSource::Argument);
        tvl.requests.push(ToolRequest::new(fa, "latest").unwrap());
        tvl.resolve(true).unwrap();
        assert_eq!(tvl.versions.len(), 1);
    }

    #[test]
    fn test_tool_version_list_failure() {
        reset();
        backend::reset();
        env::set_var("MISE_FAILURE", "1");
        file::remove_all(dirs::CACHE.join("dummy")).unwrap();
        let fa: BackendArg = "dummy".into();
        let mut tvl = ToolVersionList::new(fa.clone(), ToolSource::Argument);
        tvl.requests.push(ToolRequest::new(fa, "latest").unwrap());
        let _ = tvl.resolve(true);
        assert_eq!(tvl.versions.len(), 0);
    }
}
