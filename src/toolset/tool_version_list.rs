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
        let plugin = match config.plugins.get(&self.plugin_name) {
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
    use crate::plugins::{ExternalPlugin, Plugin, Plugins};

    #[test]
    fn test_tool_version_list() {
        let mut config = Config::default();
        let plugin = ExternalPlugin::new(&config.settings, &"tiny".to_string());
        let plugin = Arc::new(Plugins::External(plugin));
        config.plugins.insert(plugin.name().clone(), plugin.clone());
        let mut tvl = ToolVersionList::new(plugin.name().clone(), ToolSource::Argument);
        tvl.requests.push((
            ToolVersionRequest::new(plugin.name().clone(), "latest"),
            ToolVersionOptions::default(),
        ));
        tvl.resolve(&config, true);
        assert_eq!(tvl.versions.len(), 1);
    }
}
