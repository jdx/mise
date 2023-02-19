use std::fmt::{Display, Formatter};
use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::config::{Config, Settings};
use crate::errors::Error::VersionNotFound;
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

    pub fn resolve(&mut self, settings: &Settings, plugin: &Plugin) -> Result<()> {
        if self.rtv.is_some() {
            return Ok(());
        }
        match self.r#type.clone() {
            ToolVersionType::Version(v) => self.resolve_version(settings, plugin, &v),
            ToolVersionType::Prefix(v) => self.resolve_prefix(plugin, &v),
            ToolVersionType::Ref(_) => unimplemented!(),
            ToolVersionType::Path(_) => unimplemented!(),
            ToolVersionType::System => Ok(()),
        }
    }

    fn resolve_version(&mut self, settings: &Settings, plugin: &Plugin, v: &str) -> Result<()> {
        let v = resolve_alias(settings, plugin, v);

        let matches = plugin.list_versions_matching(&v);
        if matches.contains(&v) {
            self.rtv = Some(RuntimeVersion::new(Arc::new(plugin.clone()), &v));
            Ok(())
        } else {
            self.resolve_prefix(plugin, &v)
        }
    }

    fn resolve_prefix(&mut self, plugin: &Plugin, prefix: &str) -> Result<()> {
        match plugin.list_versions_matching(prefix).last() {
            Some(v) => {
                self.rtv = Some(RuntimeVersion::new(Arc::new(plugin.clone()), v));
                Ok(())
            }
            None => Err(VersionNotFound(plugin.name.clone(), prefix.to_string()))?,
        }
    }

    pub fn is_missing(&self) -> bool {
        match self.rtv {
            Some(ref rtv) => !rtv.is_installed(),
            None => true,
        }
    }

    pub fn install(&mut self, config: &Config, pr: ProgressReport) -> Result<()> {
        match self.r#type {
            ToolVersionType::Version(_) | ToolVersionType::Prefix(_) => {
                self.rtv
                    .as_ref()
                    .unwrap()
                    .install(InstallType::Version, config, pr)?;
            }
            ToolVersionType::Ref(_) => {
                self.rtv
                    .as_ref()
                    .unwrap()
                    .install(InstallType::Ref, config, pr)?;
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

pub fn resolve_alias(settings: &Settings, plugin: &Plugin, v: &str) -> String {
    if let Some(plugin_aliases) = settings.aliases.get(&plugin.name) {
        if let Some(alias) = plugin_aliases.get(v) {
            return alias.clone();
        }
    }
    if let Some(alias) = plugin.get_aliases().get(v) {
        return alias.clone();
    }
    v.to_string()
}
