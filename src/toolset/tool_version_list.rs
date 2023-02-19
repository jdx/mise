use color_eyre::eyre::Result;

use crate::config::Settings;
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
    pub fn resolve(&mut self, settings: &Settings, plugin: &Plugin) -> Result<()> {
        for tv in &mut self.versions {
            tv.resolve(settings, plugin)?;
        }
        Ok(())
    }
    pub fn resolved_versions(&self) -> Vec<&RuntimeVersion> {
        self.versions
            .iter()
            .filter_map(|v| v.rtv.as_ref())
            .collect()
    }
}
