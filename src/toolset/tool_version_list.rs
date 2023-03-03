use std::sync::Arc;

use crate::config::Config;
use crate::env;
use crate::plugins::Plugin;
use crate::runtimes::RuntimeVersion;
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
    pub fn resolve(&mut self, config: &Config, plugin: Arc<Plugin>) {
        for tv in &mut self.versions {
            match tv.resolve(config, plugin.clone()) {
                Ok(_) => {
                    if *env::PRELOAD_ENV {
                        if let Some(rtv) = tv.rtv.as_ref() {
                            // optimize loading by preloading the rtv
                            let _ = rayon::join(
                                || rtv.exec_env(),
                                || rtv.list_bin_paths(&config.settings),
                            );
                        }
                    }
                }
                Err(err) => {
                    warn!("failed to resolve tool version: {}", err);
                    return;
                }
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
    use std::env;
    use std::sync::Arc;

    use crate::config::Config;
    use crate::plugins::{Plugin, PluginName};
    use crate::toolset::{ToolSource, ToolVersion, ToolVersionList, ToolVersionType};

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
        tvl.resolve(&Config::default(), plugin);
        assert_eq!(tvl.resolved_versions().len(), 0);
        env::remove_var("RTX_FAILURE");
    }
}
