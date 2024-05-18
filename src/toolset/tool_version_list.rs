use crate::cli::args::ForgeArg;
use crate::forge;
use crate::toolset::tool_version_request::ToolRequest;
use crate::toolset::{ToolSource, ToolVersion};

/// represents several versions of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersionList {
    pub forge: ForgeArg,
    pub versions: Vec<ToolVersion>,
    pub requests: Vec<ToolRequest>,
    pub source: ToolSource,
}

impl ToolVersionList {
    pub fn new(forge: ForgeArg, source: ToolSource) -> Self {
        Self {
            forge,
            versions: Vec::new(),
            requests: vec![],
            source,
        }
    }
    pub fn resolve(&mut self, latest_versions: bool) -> eyre::Result<()> {
        self.versions.clear();
        let plugin = forge::get(&self.forge);
        for tvr in &mut self.requests {
            match tvr.resolve(plugin.as_ref(), latest_versions) {
                Ok(v) => self.versions.push(v),
                Err(err) => {
                    let source = self.source.to_string();
                    bail!("failed to resolve version of {plugin} from {source}: {err:#}");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::test::reset;
    use crate::{dirs, env, file};

    use super::*;

    #[test]
    fn test_tool_version_list() {
        reset();
        let fa: ForgeArg = "tiny".into();
        let mut tvl = ToolVersionList::new(fa.clone(), ToolSource::Argument);
        tvl.requests.push(ToolRequest::new(fa, "latest").unwrap());
        tvl.resolve(true).unwrap();
        assert_eq!(tvl.versions.len(), 1);
    }

    #[test]
    fn test_tool_version_list_failure() {
        reset();
        forge::reset();
        env::set_var("MISE_FAILURE", "1");
        file::remove_all(dirs::CACHE.join("dummy")).unwrap();
        let fa: ForgeArg = "dummy".into();
        let mut tvl = ToolVersionList::new(fa.clone(), ToolSource::Argument);
        tvl.requests.push(ToolRequest::new(fa, "latest").unwrap());
        let _ = tvl.resolve(true);
        assert_eq!(tvl.versions.len(), 0);
    }
}
