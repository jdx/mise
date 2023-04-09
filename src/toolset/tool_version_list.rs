use crate::config::Config;
use crate::toolset::tool_version_request::ToolVersionRequest;
use crate::toolset::{ToolSource, ToolVersion, ToolVersionOptions};

/// represents several versions of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersionList {
    pub plugin_name: String,
    pub versions: Vec<ToolVersion>,
    pub requests: Vec<(ToolVersionRequest, ToolVersionOptions)>,
    pub source: ToolSource,
}

impl ToolVersionList {
    pub fn new(plugin_name: String, source: ToolSource) -> Self {
        Self {
            plugin_name,
            versions: Vec::new(),
            requests: vec![],
            source,
        }
    }
    pub fn resolve(&mut self, config: &Config, latest_versions: bool) {
        self.versions.clear();
        let plugin = match config.tools.get(&self.plugin_name) {
            Some(p) => p,
            _ => {
                debug!("Plugin {} is not installed", self.plugin_name);
                return;
            }
        };
        for (tvr, opts) in &mut self.requests {
            match tvr.resolve(config, plugin, opts.clone(), latest_versions) {
                Ok(v) => self.versions.push(v),
                Err(err) => warn!("failed to resolve tool version: {:#}", err),
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::sync::Arc;

    use super::*;
    use crate::plugins::ExternalPlugin;
    use crate::tool::Tool;

    #[test]
    fn test_tool_version_list() {
        let mut config = Config::default();
        let plugin_name = "tiny".to_string();
        let plugin = ExternalPlugin::new(&config.settings, &plugin_name);
        let tool = Tool::new(plugin_name.clone(), Box::new(plugin));
        config.tools.insert(plugin_name.clone(), Arc::new(tool));
        let mut tvl = ToolVersionList::new(plugin_name.clone(), ToolSource::Argument);
        tvl.requests.push((
            ToolVersionRequest::new(plugin_name, "latest"),
            ToolVersionOptions::default(),
        ));
        tvl.resolve(&config, true);
        assert_eq!(tvl.versions.len(), 1);
    }
}
