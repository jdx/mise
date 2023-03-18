use color_eyre::eyre::Result;
use std::sync::Arc;
use std::time::Duration;
use crate::config::Config;
use crate::file::modified_duration;
use crate::plugins::Plugin;

pub struct PluginAutoupdater {
    pub plugin: Arc<Plugin>,
}

impl PluginAutoupdater {
    pub fn new(plugin: Arc<Plugin>) -> Self {
        Self { plugin }
    }

    pub fn autoupdate(&self, config: &Config) -> Result<()> {
        debug!("autoupdating plugin {}", self.plugin.name);
        if !self.needs_update(config) {
            return Ok(());
        }
        debug!("autoupdating plugin {}", self.plugin.name);
        Ok(())
    }

    fn needs_update(&self, config: &Config) -> bool {
        if config.settings.plugin_autoupdate_last_check_duration == Duration::ZERO {
            return false;
        }
        if let Ok(duration) = modified_duration(&self.plugin.plugin_path) {
            dbg!(&duration);
            dbg!(config.settings.plugin_autoupdate_last_check_duration);
            return duration > config.settings.plugin_autoupdate_last_check_duration;
        }
        false
    }
}
