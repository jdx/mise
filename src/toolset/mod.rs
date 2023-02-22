use std::env::join_paths;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use console::style;
use indexmap::IndexMap;
use itertools::Itertools;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

pub use builder::ToolsetBuilder;
pub use tool_source::ToolSource;
pub use tool_version::ToolVersion;
pub use tool_version::ToolVersionType;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgVersion};
use crate::config::{Config, MissingRuntimeBehavior};
use crate::env;
use crate::plugins::{InstallType, Plugin, PluginName};
use crate::runtimes::RuntimeVersion;
use crate::toolset::tool_version_list::ToolVersionList;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt;

mod builder;
mod tool_source;
mod tool_version;
mod tool_version_list;

/// a toolset is a collection of tools for various plugins
///
/// one example is a .tool-versions file
/// the idea is that we start with an empty toolset, then
/// merge in other toolsets from various sources
#[derive(Debug, Default)]
pub struct Toolset {
    pub versions: IndexMap<PluginName, ToolVersionList>,
    source: Option<ToolSource>,
    plugins: IndexMap<PluginName, Arc<Plugin>>,
}

impl Toolset {
    pub fn new(source: ToolSource) -> Self {
        Self {
            source: Some(source),
            ..Default::default()
        }
    }
    pub fn with_plugins(mut self, plugins: IndexMap<PluginName, Arc<Plugin>>) -> Self {
        self.plugins = plugins;
        self
    }
    pub fn add_version(&mut self, plugin: PluginName, version: ToolVersion) {
        let versions = self
            .versions
            .entry(plugin)
            .or_insert_with(|| ToolVersionList::new(self.source.clone().unwrap()));
        versions.add_version(version);
    }
    pub fn merge(&mut self, mut other: Toolset) {
        for (plugin, versions) in self.versions.clone() {
            if !other.versions.contains_key(&plugin) {
                other.versions.insert(plugin, versions);
            }
        }
        self.versions = other.versions; // swap to use other's first
        self.source = other.source;
    }
    pub fn resolve(&mut self, config: &Config) {
        self.versions
            .iter_mut()
            .collect::<Vec<_>>()
            .par_iter_mut()
            .for_each(|(p, v)| {
                let plugin = match self.plugins.get(&p.to_string()) {
                    Some(p) => p,
                    None => {
                        debug!("Plugin {} not found", p);
                        return;
                    }
                };
                v.resolve(&config.settings, plugin.clone());
            });
    }
    pub fn install_missing(&mut self, config: &Config) -> Result<()> {
        let versions = self.list_missing_versions_mut();
        if versions.is_empty() {
            return Ok(());
        }
        let display_versions = display_versions(&versions);
        let plural_versions = if versions.len() == 1 { "" } else { "s" };
        let warn = || {
            warn!(
                "Tool{} not installed: {}",
                plural_versions, display_versions
            );
        };
        match config.settings.missing_runtime_behavior {
            MissingRuntimeBehavior::Ignore => {}
            MissingRuntimeBehavior::Warn => {
                warn();
            }
            MissingRuntimeBehavior::Prompt => {
                if prompt::prompt_for_install(&display_versions) {
                    self.install_missing_versions(config)?;
                } else {
                    warn();
                }
            }
            MissingRuntimeBehavior::AutoInstall => {
                self.install_missing_versions(config)?;
            }
        }
        Ok(())
    }

    pub fn list_missing_plugins(&self) -> Vec<PluginName> {
        self.versions
            .keys()
            .filter(|p| !self.plugins.contains_key(*p))
            .cloned()
            .collect()
    }

    fn install_missing_versions(&mut self, config: &Config) -> Result<()> {
        ThreadPoolBuilder::new()
            .num_threads(config.settings.jobs)
            .build()
            .unwrap()
            .install(|| -> Result<()> {
                let mpr = MultiProgressReport::new(config.settings.verbose);
                self.install_missing_plugins(config, &mpr)?;
                self.versions
                    .iter_mut()
                    .par_bridge()
                    .filter_map(|(p, v)| {
                        let versions = v
                            .versions
                            .iter_mut()
                            .filter(|v| v.is_missing())
                            .collect_vec();
                        let plugin = self.plugins.get(&p.to_string()).unwrap();
                        match versions.is_empty() {
                            true => None,
                            false => Some((plugin, versions)),
                        }
                    })
                    .map(|(plugin, versions)| {
                        for version in versions {
                            version.resolve(&config.settings, plugin.clone())?;
                            version.install(config, mpr.add())?;
                        }
                        Ok(())
                    })
                    .collect::<Result<Vec<()>>>()?;
                Ok(())
            })
    }
    fn install_missing_plugins(
        &mut self,
        config: &Config,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        let missing_plugins = self.list_missing_plugins();
        if missing_plugins.is_empty() {
            return Ok(());
        }
        let plugins = missing_plugins
            .into_par_iter()
            .map(|plugin_name| {
                let plugin = Plugin::new(&plugin_name);
                if !plugin.is_installed() {
                    plugin.install(config, None, mpr.add())?;
                }
                Ok(plugin)
            })
            .collect::<Result<Vec<_>>>()?;
        for plugin in plugins {
            self.plugins.insert(plugin.name.clone(), Arc::new(plugin));
        }
        self.plugins.sort_keys();
        Ok(())
    }

    fn list_missing_versions_mut(&mut self) -> Vec<&mut ToolVersion> {
        let versions = self
            .versions
            .values_mut()
            .flat_map(|v| {
                v.versions
                    .iter_mut()
                    .filter(|v| v.is_missing())
                    .collect_vec()
            })
            .collect_vec();
        versions
    }
    pub fn list_installed_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let versions = self
            .plugins
            .values()
            .collect_vec()
            .into_par_iter()
            .map(|p| {
                let versions = p.list_installed_versions()?;
                Ok(versions
                    .into_iter()
                    .map(|v| RuntimeVersion::new(p.clone(), InstallType::Version(v))))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();

        Ok(versions)
    }
    pub fn list_versions_by_plugin(&self) -> IndexMap<PluginName, Vec<&RuntimeVersion>> {
        self.versions
            .iter()
            .filter_map(|(p, v)| match self.plugins.get(&p.to_string()) {
                Some(plugin) => {
                    let plugin = Arc::new(plugin.clone());
                    let versions = v.resolved_versions();
                    Some((plugin.name.clone(), versions))
                }
                None => {
                    debug!("Plugin {} not found", p);
                    None
                }
            })
            .collect()
    }
    pub fn list_current_versions(&self) -> Vec<&RuntimeVersion> {
        self.list_versions_by_plugin()
            .into_iter()
            .flat_map(|(_, v)| v)
            .collect()
    }
    pub fn list_current_installed_versions(&self) -> Vec<&RuntimeVersion> {
        self.list_current_versions()
            .into_iter()
            .filter(|v| v.is_installed())
            .collect()
    }
    pub fn env(&self) -> IndexMap<String, String> {
        let mut entries: IndexMap<String, String> = self
            .list_current_installed_versions()
            .into_par_iter()
            .flat_map(|v| match v.exec_env() {
                Ok(env) => env.into_iter().collect(),
                Err(e) => {
                    warn!("Error running exec-env: {}", e);
                    Vec::new()
                }
            })
            .collect::<Vec<(String, String)>>()
            .into_iter()
            .rev()
            .collect();
        entries.sort_keys();
        entries
    }
    pub fn path_env(&self) -> String {
        let installs = self.list_paths();
        join_paths([installs, env::PATH.clone()].concat())
            .unwrap()
            .to_string_lossy()
            .into()
    }
    pub fn list_paths(&self) -> Vec<PathBuf> {
        self.list_current_installed_versions()
            .into_par_iter()
            .flat_map(|rtv| match rtv.list_bin_paths() {
                Ok(paths) => paths,
                Err(e) => {
                    warn!("Error listing bin paths for {}: {}", rtv, e);
                    Vec::new()
                }
            })
            .collect()
    }
    pub fn resolve_runtime_arg(&self, arg: &RuntimeArg) -> Option<&RuntimeVersion> {
        match &arg.version {
            RuntimeArgVersion::System => None,
            RuntimeArgVersion::Version(version) => {
                if let Some(tvl) = self.versions.get(&arg.plugin) {
                    for tv in tvl.versions.iter() {
                        match &tv.r#type {
                            ToolVersionType::Version(v) if v == version => {
                                return tv.rtv.as_ref();
                            }
                            _ => (),
                        }
                    }
                }
                None
            }
            RuntimeArgVersion::Prefix(version) => {
                if let Some(tvl) = self.versions.get(&arg.plugin) {
                    for tv in tvl.versions.iter() {
                        match &tv.r#type {
                            ToolVersionType::Prefix(v) if v.starts_with(version) => {
                                return tv.rtv.as_ref();
                            }
                            _ => (),
                        }
                    }
                }
                None
            }
            RuntimeArgVersion::Ref(ref_) => {
                if let Some(tvl) = self.versions.get(&arg.plugin) {
                    for tv in tvl.versions.iter() {
                        match &tv.r#type {
                            ToolVersionType::Ref(v) if v == ref_ => {
                                return tv.rtv.as_ref();
                            }
                            _ => (),
                        }
                    }
                }
                None
            }
            RuntimeArgVersion::Path(path) => {
                if let Some(tvl) = self.versions.get(&arg.plugin) {
                    for tv in tvl.versions.iter() {
                        match &tv.r#type {
                            ToolVersionType::Path(v) if v == path => {
                                return tv.rtv.as_ref();
                            }
                            _ => (),
                        }
                    }
                }
                None
            }
            RuntimeArgVersion::None => {
                let plugin = self.versions.get(&arg.plugin);
                match plugin {
                    Some(tvl) => tvl.versions.first().unwrap().rtv.as_ref(),
                    None => None,
                }
            }
        }
    }
}

impl Display for Toolset {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let plugins = &self
            .versions
            .iter()
            .map(|(_, v)| v.versions.iter().map(|v| v.to_string()).join(" "))
            .collect_vec();
        write!(f, "Toolset: {}", plugins.join(", "))
    }
}

fn display_versions(versions: &[&mut ToolVersion]) -> String {
    let display_versions = versions
        .iter()
        .map(|v| style(&v.to_string()).cyan().for_stderr().to_string())
        .join(", ");
    display_versions
}
