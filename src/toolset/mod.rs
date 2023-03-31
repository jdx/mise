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
use crate::config::{Config, MissingRuntimeBehavior};
use crate::env;
use crate::plugins::{ExternalPlugin, Plugin, PluginName, Plugins};
use crate::runtime_symlinks::rebuild_symlinks;
use crate::runtimes::RuntimeVersion;
use crate::shims::reshim;
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
    pub source: Option<ToolSource>,
    pub latest_versions: bool,
}

impl Toolset {
    pub fn new(source: ToolSource) -> Self {
        Self {
            source: Some(source),
            ..Default::default()
        }
    }
    pub fn add_version(&mut self, version: ToolVersion) {
        let versions = self
            .versions
            .entry(version.plugin_name.clone())
            .or_insert_with(|| ToolVersionList::new(self.source.clone().unwrap()));
        versions.add_version(version);
    }
    pub fn merge(&mut self, other: &Toolset) {
        let mut versions = other.versions.clone();
        for (plugin, tvl) in self.versions.clone() {
            if !other.versions.contains_key(&plugin) {
                versions.insert(plugin, tvl);
            }
        }
        self.versions = versions;
        self.source = other.source.clone();
    }
    pub fn resolve(&mut self, config: &Config) {
        self.versions
            .iter_mut()
            .collect::<Vec<_>>()
            .par_iter_mut()
            .for_each(|(p, v)| {
                let plugin = match config.plugins.get(&p.to_string()) {
                    Some(p) if p.is_installed() => p,
                    _ => {
                        debug!("Plugin {} is not installed", p);
                        return;
                    }
                };
                v.resolve(config, plugin.clone(), self.latest_versions);
            });
    }
    pub fn install_missing(&mut self, config: &mut Config, mpr: MultiProgressReport) -> Result<()> {
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
                    self.install_missing_versions(config, versions, mpr)?;
                }
            }
            MissingRuntimeBehavior::AutoInstall => {
                self.install_missing_versions(config, versions, mpr)?;
            }
        }
        Ok(())
    }

    pub fn list_missing_plugins(&self, config: &Config) -> Vec<PluginName> {
        self.versions
            .keys()
            .filter(|p| !config.plugins.contains_key(*p))
            .cloned()
            .collect()
    }

    fn install_missing_versions(
        &mut self,
        config: &mut Config,
        selected_versions: Vec<ToolVersion>,
        mpr: MultiProgressReport,
    ) -> Result<()> {
        ThreadPoolBuilder::new()
            .num_threads(config.settings.jobs)
            .build()?
            .install(|| -> Result<()> {
                let plugins = selected_versions
                    .iter()
                    .map(|v| v.plugin_name.clone())
                    .unique()
                    .collect_vec();
                let selected_versions = selected_versions
                    .into_iter()
                    .map(|v| v.r#type)
                    .collect::<HashSet<_>>();
                self.install_missing_plugins(config, plugins, &mpr)?;
                self.versions
                    .iter_mut()
                    .par_bridge()
                    .filter_map(|(p, v)| {
                        let versions = v
                            .versions
                            .iter_mut()
                            .filter(|v| v.is_missing() && selected_versions.contains(&v.r#type))
                            .collect_vec();
                        let plugin = config.plugins.get(&p.to_string());
                        match (plugin, versions.is_empty()) {
                            (Some(plugin), false) => Some((plugin, versions)),
                            _ => None,
                        }
                    })
                    .map(|(plugin, versions)| {
                        for version in versions {
                            let mut pr = mpr.add();
                            version.resolve(config, plugin.clone(), self.latest_versions)?;
                            version.install(config, &mut pr, false)?;
                        }
                        Ok(())
                    })
                    .collect::<Result<Vec<()>>>()?;
                reshim(config, self)?;
                rebuild_symlinks(config)?;
                Ok(())
            })
    }
    fn install_missing_plugins(
        &mut self,
        config: &mut Config,
        missing_plugins: Vec<PluginName>,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        for plugin in &missing_plugins {
            config.plugins.entry(plugin.clone()).or_insert_with(|| {
                Arc::new(Plugins::External(ExternalPlugin::new(
                    &config.settings,
                    plugin,
                )))
            });
        }
        config.plugins.sort_keys();
        missing_plugins
            .into_par_iter()
            .map(|p| config.plugins.get(&p).unwrap())
            .filter(|p| !p.is_installed())
            .map(|p| {
                let mut pr = mpr.add();
                p.install(config, &mut pr, false)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }

    pub fn list_missing_versions(&self) -> Vec<ToolVersion> {
        let versions = self
            .versions
            .values()
            .flat_map(|v| v.versions.iter().filter(|v| v.is_missing()).collect_vec())
            .cloned()
            .collect_vec();
        versions
    }
    pub fn list_installed_versions(&self, config: &Config) -> Result<Vec<RuntimeVersion>> {
        let versions = config
            .plugins
            .values()
            .collect_vec()
            .into_par_iter()
            .map(|p| {
                let versions = p.list_installed_versions()?;
                Ok(versions.into_iter().map(|v| {
                    let tv =
                        ToolVersion::new(p.name().clone(), ToolVersionType::Version(v.clone()));
                    RuntimeVersion::new(config, p.clone(), v, tv)
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
            .map(|(p, v)| {
                let versions = v.resolved_versions();
                (p.clone(), versions)
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
    pub fn env_with_path(&self, config: &Config) -> IndexMap<String, String> {
        let mut env = self.env(config);
        let path_env = self.path_env(config);
        env.insert("PATH".to_string(), path_env);
        env
    }
    pub fn env(&self, config: &Config) -> IndexMap<String, String> {
        let mut entries: IndexMap<String, String> = self
            .list_current_installed_versions()
            .into_par_iter()
            .flat_map(|v| match v.exec_env() {
                Ok(env) => env.clone().into_iter().collect(),
                Err(e) => {
                    warn!("Error running exec-env: {:#}", e);
                    Vec::new()
                }
            })
            .collect::<Vec<(String, String)>>()
            .into_iter()
            .filter(|(k, _)| k != "RTX_ADD_PATH")
            .filter(|(k, _)| !k.starts_with("RTX_TOOL_OPTS__"))
            .rev()
            .collect();
        entries.sort_keys();
        entries.extend(config.env.clone());
        entries
    }
    pub fn path_env(&self, config: &Config) -> String {
        let installs = self.list_paths(config);
        join_paths([config.path_dirs.clone(), installs, env::PATH.clone()].concat())
            .unwrap()
            .to_string_lossy()
            .into()
    }
    pub fn list_paths(&self, config: &Config) -> Vec<PathBuf> {
        self.list_current_installed_versions()
            .into_par_iter()
            .flat_map(|rtv| match rtv.list_bin_paths(&config.settings) {
                Ok(paths) => paths.clone(),
                Err(e) => {
                    warn!("Error listing bin paths for {}: {:#}", rtv, e);
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

    pub fn which(&self, config: &Config, bin_name: &str) -> Option<&RuntimeVersion> {
        self.list_current_installed_versions()
            .into_par_iter()
            .find_first(|v| {
                if let Ok(x) = v.which(&config.settings, bin_name) {
                    x.is_some()
                } else {
                    false
                }
            })
    }

    pub fn list_rtvs_with_bin(
        &self,
        config: &Config,
        bin_name: &str,
    ) -> Result<Vec<RuntimeVersion>> {
        Ok(self
            .list_installed_versions(config)?
            .into_par_iter()
            .filter(|v| match v.which(&config.settings, bin_name) {
                Ok(x) => x.is_some(),
                Err(e) => {
                    warn!("Error running which: {:#}", e);
                    false
                }
            })
            .collect())
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
