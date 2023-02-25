use crate::config::Settings;
use crate::plugins::Plugin;
use crate::runtimes::RuntimeVersion;
use std::sync::Arc;

use crate::toolset::{ToolSource, ToolVersion};

/// represents several versions of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersionList {
    pub versions: Vec<ToolVersion>,
    pub source: ToolSource,
}

impl ToolVersionList {
    pub fn new(source: ToolSource) -> Self {
        Self {
            versions: Vec::new(),
            source,
        }
    }
    pub fn add_version(&mut self, version: ToolVersion) {
        self.versions.push(version);
    }
    pub fn resolve(&mut self, settings: &Settings, plugin: Arc<Plugin>) {
        for tv in &mut self.versions {
            if let Err(err) = tv.resolve(settings, plugin.clone()) {
                warn!("failed to resolve tool version: {}", err);
            }
        }
    }
    pub fn resolved_versions(&self) -> Vec<&RuntimeVersion> {
        self.versions
            .iter()
            .filter_map(|v| v.rtv.as_ref())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Settings;
    use crate::plugins::{Plugin, PluginName};
    use crate::toolset::{ToolSource, ToolVersion, ToolVersionList, ToolVersionType};
    use std::env;
    use std::sync::Arc;

    #[test]
    fn test_tool_version_list_failure() {
        env::set_var("RTX_FAILURE", "1");
        let mut tvl = ToolVersionList::new(ToolSource::Argument);
        let plugin = Arc::new(Plugin::new(&PluginName::from("dummy")));
        plugin.clear_remote_version_cache().unwrap();
        tvl.add_version(ToolVersion::new(
            plugin.name.to_string(),
            ToolVersionType::Version("1.0.0".to_string()),
        ));
        tvl.resolve(&Settings::default(), plugin);
        assert_eq!(tvl.resolved_versions().len(), 0);
        env::remove_var("RTX_FAILURE");
    }
}
