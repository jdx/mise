use std::collections::HashSet;
use std::env::join_paths;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use console::style;
use dialoguer::theme::ColorfulTheme;
use dialoguer::MultiSelect;
use indexmap::IndexMap;
use itertools::Itertools;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

pub use builder::ToolsetBuilder;
pub use tool_source::ToolSource;
pub use tool_version::ToolVersion;
pub use tool_version::ToolVersionType;
pub use tool_version_list::ToolVersionList;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgVersion};
use crate::config::{Config, MissingRuntimeBehavior, Settings};
use crate::env;
use crate::plugins::{Plugin, PluginName};
use crate::runtimes::RuntimeVersion;
use crate::ui::multi_progress_report::MultiProgressReport;

mod builder;
mod tool_source;
mod tool_version;
mod tool_version_list;

/// a toolset is a collection of tools for various plugins
///
/// one example is a .tool-versions file
/// the idea is that we start with an empty toolset, then
/// merge in other toolsets from various sources
#[derive(Debug, Default, Clone)]
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
        let versions = self.list_missing_versions();
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
                let versions = prompt_for_versions(&versions)?;
                if versions.is_empty() {
                    warn();
                } else {
                    self.install_missing_versions(config, versions)?;
                }
            }
            MissingRuntimeBehavior::AutoInstall => {
                self.install_missing_versions(config, versions)?;
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

    fn install_missing_versions(
        &mut self,
        config: &Config,
        selected_versions: Vec<ToolVersion>,
    ) -> Result<()> {
        ThreadPoolBuilder::new()
            .num_threads(config.settings.jobs)
            .build()
            .unwrap()
            .install(|| -> Result<()> {
                let mpr = MultiProgressReport::new(config.settings.verbose);
                let plugins = selected_versions
                    .iter()
                    .map(|v| v.plugin_name.clone())
                    .unique()
                    .collect_vec();
                let selected_versions = selected_versions
                    .into_iter()
                    .map(|v| v.r#type)
                    .collect::<HashSet<_>>();
                self.install_missing_plugins(config, &mpr, plugins)?;
                self.versions
                    .iter_mut()
                    .par_bridge()
                    .filter_map(|(p, v)| {
                        let versions = v
                            .versions
                            .iter_mut()
                            .filter(|v| v.is_missing() && selected_versions.contains(&v.r#type))
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
        missing_plugins: Vec<PluginName>,
    ) -> Result<()> {
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

    fn list_missing_versions(&self) -> Vec<ToolVersion> {
        let versions = self
            .versions
            .values()
            .flat_map(|v| v.versions.iter().filter(|v| v.is_missing()).collect_vec())
            .cloned()
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
                Ok(versions.into_iter().map(|v| {
                    let tv = ToolVersion::new(p.name.clone(), ToolVersionType::Version(v.clone()));
                    RuntimeVersion::new(p.clone(), v, tv)
                }))
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
    pub fn env(&self, config: &Config) -> IndexMap<String, String> {
        let mut entries: IndexMap<String, String> = self
            .list_current_installed_versions()
            .into_par_iter()
            .flat_map(|v| match v.exec_env() {
                Ok(env) => env.clone().into_iter().collect(),
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
        entries.extend(config.env.clone());
        entries
    }
    pub fn path_env(&self, settings: &Settings) -> String {
        let installs = self.list_paths(settings);
        join_paths([installs, env::PATH.clone()].concat())
            .unwrap()
            .to_string_lossy()
            .into()
    }
    pub fn list_paths(&self, settings: &Settings) -> Vec<PathBuf> {
        self.list_current_installed_versions()
            .into_par_iter()
            .flat_map(|rtv| match rtv.list_bin_paths(settings) {
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

    pub fn which(&self, settings: &Settings, bin_name: &str) -> Option<&RuntimeVersion> {
        self.list_current_installed_versions()
            .into_par_iter()
            .find_first(|v| {
                if let Ok(x) = v.which(settings, bin_name) {
                    x.is_some()
                } else {
                    false
                }
            })
    }
}

impl Display for Toolset {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let plugins = &self
            .versions
            .iter()
            .map(|(_, v)| v.versions.iter().map(|v| v.to_string()).join(" "))
            .collect_vec();
        write!(f, "{}", plugins.join(", "))
    }
}

fn display_versions(versions: &[ToolVersion]) -> String {
    let display_versions = versions
        .iter()
        .map(|v| style(&v.to_string()).cyan().for_stderr().to_string())
        .join(", ");
    display_versions
}

fn prompt_for_versions(versions: &[ToolVersion]) -> Result<Vec<ToolVersion>> {
    if !console::user_attended_stderr() {
        return Ok(vec![]);
    }
    Ok(MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select versions to install")
        .items(versions)
        .defaults(&versions.iter().map(|_| true).collect_vec())
        .interact()?
        .into_iter()
        .map(|i| versions[i].clone())
        .collect())
}
