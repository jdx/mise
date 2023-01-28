use std::collections::HashMap;
use std::sync::Arc;

use color_eyre::eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use rayon::prelude::*;
use versions::Mess;

use crate::config::AliasMap;
use crate::plugins::{Plugin, PluginName, PluginSource};
use crate::runtimes::RuntimeVersion;

#[derive(Debug, Default)]
pub struct Toolset {
    pub plugins: HashMap<PluginName, Arc<Plugin>>,
    installed_versions: HashMap<PluginName, HashMap<String, Arc<RuntimeVersion>>>,
    current_versions: IndexMap<PluginName, Vec<String>>,
    current_versions_sources: HashMap<PluginName, PluginSource>,
}

impl Toolset {
    pub fn find_plugin(&self, key: &PluginName) -> Option<Arc<Plugin>> {
        self.plugins.get(key).map(Arc::clone)
    }

    pub fn get_or_add_plugin(&mut self, name: String) -> Result<Arc<Plugin>> {
        let plugin = match self.plugins.get(&name) {
            Some(p) => p,
            None => {
                let plugin = Plugin::load(&name)?;
                self.plugins.entry(name).or_insert_with(|| Arc::new(plugin))
            }
        };

        Ok(plugin.clone())
    }

    pub fn get_or_add_version(
        &mut self,
        plugin: &str,
        version: String,
    ) -> Result<Arc<RuntimeVersion>> {
        let plugin = self.get_or_add_plugin(plugin.into())?;
        let rtv = self
            .installed_versions
            .entry(plugin.name.clone())
            .or_default()
            .entry(version.clone())
            .or_insert_with(|| Arc::new(RuntimeVersion::new(plugin, &version)))
            .clone();

        Ok(rtv)
    }

    pub fn add_runtime_versions(&mut self, plugin: &str, versions: Vec<String>) -> Result<()> {
        for version in versions {
            self.get_or_add_version(plugin, version)?;
        }
        Ok(())
    }

    pub fn set_current_runtime_versions(
        &mut self,
        plugin: &str,
        versions: Vec<String>,
        source: PluginSource,
    ) -> Result<()> {
        self.get_or_add_plugin(plugin.into())?;
        self.current_versions.insert(plugin.into(), versions);
        self.current_versions_sources.insert(plugin.into(), source);
        Ok(())
    }

    pub fn list_plugins(&self) -> Vec<Arc<Plugin>> {
        self.plugins
            .values()
            .sorted_by_cached_key(|p| p.name.clone())
            .map(Arc::clone)
            .collect()
    }

    pub fn list_installed_plugins(&self) -> Vec<Arc<Plugin>> {
        self.list_plugins()
            .into_iter()
            .filter(|p| p.is_installed())
            .collect()
    }

    pub fn list_current_plugins(&self) -> Vec<Arc<Plugin>> {
        self.current_versions
            .keys()
            .sorted()
            .map(|p| self.find_plugin(p).unwrap())
            .collect()
    }

    pub fn list_installed_versions(&self) -> Vec<Arc<RuntimeVersion>> {
        self.installed_versions
            .iter()
            .sorted_by_cached_key(|(plugin_name, _)| plugin_name.to_string())
            .flat_map(|(_, versions)| versions.iter().map(|(_, rtv)| rtv.clone()))
            .collect()
    }

    pub fn list_current_versions(&self) -> Vec<Arc<RuntimeVersion>> {
        self.current_versions
            .iter()
            .flat_map(|(plugin_name, versions)| {
                versions
                    .iter()
                    .map(|v| {
                        self.resolve_version(plugin_name, v).unwrap_or_else(|| {
                            let plugin = self
                                .find_plugin(plugin_name)
                                .unwrap_or_else(|| Arc::new(Plugin::new(plugin_name)));
                            Arc::new(RuntimeVersion::new(plugin, v))
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn list_current_installed_versions(&self) -> Vec<Arc<RuntimeVersion>> {
        self.list_current_versions()
            .into_iter()
            .filter(|rtv| rtv.is_installed())
            .collect()
    }

    pub fn resolve_all_versions(&mut self, aliases: &AliasMap) -> Result<()> {
        let default_aliases = IndexMap::new();
        self.current_versions = self
            .current_versions
            .clone()
            .into_iter()
            .collect_vec()
            .into_par_iter()
            .map(|(plugin_name, versions)| {
                let aliases = aliases.get(&plugin_name).unwrap_or(&default_aliases);
                let plugin = self
                    .find_plugin(&plugin_name)
                    .unwrap_or_else(|| Arc::new(Plugin::new(&plugin_name)));
                let versions = versions
                    .iter()
                    .map(|v| {
                        let v = match aliases.get(v) {
                            Some(version) => {
                                trace!("resolved alias: {}@{} -> {}", plugin.name, v, version);
                                version
                            }
                            _ => v,
                        };
                        match self.resolve_version(&plugin_name, v) {
                            Some(rtv) => Ok(rtv.version.clone()),
                            None => {
                                let latest = if plugin.is_installed() {
                                    plugin.latest_version(v)?
                                } else {
                                    Some(v.clone())
                                };
                                Ok(latest.unwrap_or_else(|| v.clone()))
                            }
                        }
                    })
                    .collect::<Result<Vec<String>>>()?;
                Ok((plugin_name, versions))
            })
            .collect::<Result<Vec<(PluginName, Vec<String>)>>>()?
            .into_iter()
            .collect::<IndexMap<PluginName, Vec<String>>>();
        trace!("resolved versions: {:?}", self.current_versions);
        Ok(())
        // if plugin.is_installed() {
        //     if let Some(latest) = plugin.latest_version(version)? {
        //         return Ok(Arc::new(RuntimeVersion::new(plugin, &latest)));
        //     }
        // }
        // Ok(Arc::new(RuntimeVersion::new(plugin, version)))
    }

    pub fn resolve_version(
        &self,
        plugin: &PluginName,
        version: &str,
    ) -> Option<Arc<RuntimeVersion>> {
        if let Some(installed_versions) = self.installed_versions.get(plugin) {
            if let Some(rtv) = installed_versions.get(version) {
                return Some(rtv.clone());
            }
            let sorted_versions = installed_versions
                .keys()
                .sorted_by_cached_key(|v| v.parse::<Mess>().unwrap())
                .rev()
                .collect::<Vec<_>>();
            for v in sorted_versions {
                if v.starts_with(version) {
                    return Some(installed_versions[v].clone());
                }
            }
        }

        None
    }

    pub fn get_source_for_plugin(&self, plugin: &PluginName) -> Option<PluginSource> {
        self.current_versions_sources.get(plugin).cloned()
    }

    pub fn find_by_prefix(
        &self,
        aliases: &AliasMap,
        plugin: &str,
        prefix: &str,
    ) -> Option<Arc<RuntimeVersion>> {
        let default_aliases = IndexMap::new();
        let aliases = aliases.get(plugin).unwrap_or(&default_aliases);
        let prefix = aliases.get(prefix).cloned().unwrap_or(prefix.to_string());

        let mut versions = self.list_current_versions();
        versions.extend(self.list_installed_versions());
        versions
            .into_iter()
            .find(|rtv| rtv.plugin.name == plugin && rtv.version.starts_with(&prefix))
    }
}
