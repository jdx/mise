use std::fmt::{Display, Formatter};
use std::fs;
use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::config::{Config, Settings};
use crate::dirs;
use crate::plugins::{InstallType, Plugin};
use crate::runtimes::RuntimeVersion;
use crate::ui::progress_report::ProgressReport;

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub plugin_name: String,
    pub r#type: ToolVersionType,
    pub rtv: Option<RuntimeVersion>,
}

#[derive(Debug, Clone)]
pub enum ToolVersionType {
    Version(String),
    Prefix(String),
    Ref(String),
    Path(String),
    System,
}

impl ToolVersion {
    pub fn new(plugin_name: String, r#type: ToolVersionType) -> Self {
        Self {
            plugin_name,
            r#type,
            rtv: None,
        }
    }

    pub fn resolve(&mut self, settings: &Settings, plugin: Arc<Plugin>) -> Result<()> {
        if self.rtv.is_some() {
            return Ok(());
        }
        match self.r#type.clone() {
            ToolVersionType::Version(v) => self.resolve_version(settings, plugin, &v),
            ToolVersionType::Prefix(v) => self.resolve_prefix(settings, plugin, &v),
            ToolVersionType::Ref(r) => {
                self.rtv = Some(RuntimeVersion::new(plugin, InstallType::Ref(r)));
                Ok(())
            }
            ToolVersionType::Path(path) => {
                let path = fs::canonicalize(path)?;
                self.rtv = Some(RuntimeVersion::new(plugin, InstallType::Path(path)));
                Ok(())
            }
            ToolVersionType::System => Ok(()),
        }
    }

    fn resolve_version(&mut self, settings: &Settings, plugin: Arc<Plugin>, v: &str) -> Result<()> {
        let v = resolve_alias(settings, plugin.clone(), v)?;

        if dirs::INSTALLS.join(&plugin.name).join(&v).exists() {
            // if the version is already installed, no need to fetch all of the remote versions
            self.rtv = Some(RuntimeVersion::new(plugin, InstallType::Version(v)));
            return Ok(());
        }

        let matches = plugin.list_versions_matching(settings, &v)?;
        if matches.contains(&v) {
            self.rtv = Some(RuntimeVersion::new(plugin, InstallType::Version(v)));
        } else {
            self.resolve_prefix(settings, plugin, &v)?;
        }

        Ok(())
    }

    fn resolve_prefix(
        &mut self,
        settings: &Settings,
        plugin: Arc<Plugin>,
        prefix: &str,
    ) -> Result<()> {
        let matches = plugin.list_versions_matching(settings, prefix)?;
        let v = match matches.last() {
            Some(v) => v,
            None => prefix,
            // None => Err(VersionNotFound(plugin.name.clone(), prefix.to_string()))?,
        };
        self.rtv = Some(RuntimeVersion::new(
            plugin,
            InstallType::Version(v.to_string()),
        ));
        Ok(())
    }

    pub fn is_missing(&self) -> bool {
        match self.rtv {
            Some(ref rtv) => !rtv.is_installed(),
            None => true,
        }
    }

    pub fn install(&mut self, config: &Config, pr: ProgressReport) -> Result<()> {
        match self.r#type {
            ToolVersionType::Version(_) | ToolVersionType::Prefix(_) | ToolVersionType::Ref(_) => {
                self.rtv.as_ref().unwrap().install(config, pr)?;
            }
            _ => (),
        }
        if matches!(self.r#type, ToolVersionType::System) {
            return Ok(());
        }
        Ok(())
    }
}

impl Display for ToolVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let plugin = &self.plugin_name;
        match &self.r#type {
            ToolVersionType::Version(v) => write!(f, "{plugin}@{v}"),
            ToolVersionType::Prefix(p) => write!(f, "{plugin}@prefix:{p}"),
            ToolVersionType::Ref(r) => write!(f, "{plugin}@ref:{r}"),
            ToolVersionType::Path(p) => write!(f, "{plugin}@path:{p}"),
            ToolVersionType::System => write!(f, "{plugin}@system"),
        }
    }
}

pub fn resolve_alias(settings: &Settings, plugin: Arc<Plugin>, v: &str) -> Result<String> {
    if let Some(plugin_aliases) = settings.aliases.get(&plugin.name) {
        if let Some(alias) = plugin_aliases.get(v) {
            return Ok(alias.clone());
        }
    }
    if let Some(alias) = plugin.get_aliases(settings)?.get(v) {
        return Ok(alias.clone());
    }
    Ok(v.to_string())
}
