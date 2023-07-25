use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env::join_paths;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use color_eyre::eyre::Result;
use console::style;
use dialoguer::theme::ColorfulTheme;
use dialoguer::MultiSelect;
use indexmap::IndexMap;
use itertools::Itertools;
use rayon::prelude::*;

pub use builder::ToolsetBuilder;
pub use tool_source::ToolSource;
pub use tool_version::ToolVersion;
pub use tool_version_list::ToolVersionList;
pub use tool_version_request::ToolVersionRequest;

use crate::config::{Config, MissingRuntimeBehavior};
use crate::env;
use crate::plugins::PluginName;
use crate::runtime_symlinks;
use crate::shims;
use crate::tool::Tool;
use crate::ui::multi_progress_report::MultiProgressReport;

mod builder;
mod tool_source;
mod tool_version;
mod tool_version_list;
mod tool_version_request;

pub type ToolVersionOptions = BTreeMap<String, String>;

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
    pub disable_tools: BTreeSet<PluginName>,
}

impl Toolset {
    pub fn new(source: ToolSource) -> Self {
        Self {
            source: Some(source),
            ..Default::default()
        }
    }
    pub fn add_version(&mut self, tvr: ToolVersionRequest, opts: ToolVersionOptions) {
        if self.disable_tools.contains(tvr.plugin_name()) {
            return;
        }
        let tvl = self
            .versions
            .entry(tvr.plugin_name().clone())
            .or_insert_with(|| {
                ToolVersionList::new(tvr.plugin_name().to_string(), self.source.clone().unwrap())
            });
        tvl.requests.push((tvr, opts));
    }
    pub fn merge(&mut self, other: &Toolset) {
        let mut versions = other.versions.clone();
        for (plugin, tvl) in self.versions.clone() {
            if !other.versions.contains_key(&plugin) {
                versions.insert(plugin, tvl);
            }
        }
        versions.retain(|_, tvl| !self.disable_tools.contains(&tvl.plugin_name));
        self.versions = versions;
        self.source = other.source.clone();
    }
    pub fn resolve(&mut self, config: &mut Config) {
        self.list_missing_plugins(config);
        self.versions
            .iter_mut()
            .collect::<Vec<_>>()
            .par_iter_mut()
            .for_each(|(_, v)| v.resolve(config, self.latest_versions));
    }
    pub fn install_missing(&mut self, config: &mut Config, mpr: MultiProgressReport) -> Result<()> {
        let versions = self
            .list_missing_versions(config)
            .into_iter()
            .cloned()
            .collect_vec();
        if versions.is_empty() {
            return Ok(());
        }
        let display_versions = display_versions(&versions);
        let plural_versions = if versions.len() == 1 { "" } else { "s" };
        let warn = || {
            warn!(
                "Tool{} not installed: {} (install with: rtx install)",
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
                    self.install_versions(config, versions, &mpr, false)?;
                }
            }
            MissingRuntimeBehavior::AutoInstall => {
                self.install_versions(config, versions, &mpr, false)?;
            }
        }
        Ok(())
    }

    pub fn list_missing_plugins(&self, config: &mut Config) -> Vec<PluginName> {
        for plugin in self.versions.keys() {
            config.get_or_create_tool(plugin);
        }
        self.versions
            .keys()
            .map(|p| config.tools.get(p).unwrap())
            .filter(|p| !p.is_installed())
            .map(|p| p.name.clone())
            .collect()
    }

    pub fn install_versions(
        &mut self,
        config: &mut Config,
        versions: Vec<ToolVersion>,
        mpr: &MultiProgressReport,
        force: bool,
    ) -> Result<()> {
        self.latest_versions = true;
        let queue: Vec<_> = versions
            .into_iter()
            .group_by(|v| v.plugin_name.clone())
            .into_iter()
            .map(|(pn, v)| (config.get_or_create_tool(&pn), v.collect_vec()))
            .collect();
        let queue = Arc::new(Mutex::new(queue));
        thread::scope(|s| {
            (0..config.settings.jobs)
                .map(|_| {
                    let queue = queue.clone();
                    let config = &*config;
                    s.spawn(move || {
                        let next_job = || queue.lock().unwrap().pop();
                        while let Some((t, versions)) = next_job() {
                            if !t.is_installed() {
                                t.install(config, &mut mpr.add(), force)?;
                            }
                            for tv in versions {
                                let tv = tv.request.resolve(config, &t, tv.opts.clone(), true)?;
                                let mut pr = mpr.add();
                                t.install_version(config, &tv, &mut pr, force)?;
                            }
                        }
                        Ok(())
                    })
                })
                .collect_vec()
                .into_iter()
                .map(|t| t.join().unwrap())
                .collect::<Result<Vec<()>>>()
        })?;
        self.resolve(config);
        shims::reshim(config, self)?;
        runtime_symlinks::rebuild(config)
    }
    pub fn list_missing_versions(&self, config: &Config) -> Vec<&ToolVersion> {
        self.versions
            .iter()
            .map(|(p, tvl)| {
                let p = config.tools.get(p).unwrap();
                (p, tvl)
            })
            .flat_map(|(p, tvl)| {
                tvl.versions
                    .iter()
                    .filter(|tv| !p.is_version_installed(tv))
                    .collect_vec()
            })
            .collect()
    }
    pub fn list_installed_versions(
        &self,
        config: &Config,
    ) -> Result<Vec<(Arc<Tool>, ToolVersion)>> {
        let current_versions: HashMap<(PluginName, String), (Arc<Tool>, ToolVersion)> = self
            .list_current_versions(config)
            .into_iter()
            .map(|(p, tv)| ((p.name.clone(), tv.version.clone()), (p.clone(), tv)))
            .collect();
        let versions = config
            .tools
            .values()
            .collect_vec()
            .into_par_iter()
            .map(|p| {
                let versions = p.list_installed_versions()?;
                Ok(versions.into_iter().map(|v| {
                    match current_versions.get(&(p.name.clone(), v.clone())) {
                        Some((p, tv)) => (p.clone(), tv.clone()),
                        None => {
                            let tv = ToolVersionRequest::new(p.name.clone(), &v)
                                .resolve(config, p, Default::default(), false)
                                .unwrap();
                            (p.clone(), tv)
                        }
                    }
                }))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();

        Ok(versions)
    }
    pub fn list_versions_by_plugin(&self, config: &Config) -> Vec<(Arc<Tool>, &Vec<ToolVersion>)> {
        self.versions
            .iter()
            .map(|(p, v)| {
                let p = config.tools.get(p).unwrap();
                (p.clone(), &v.versions)
            })
            .collect()
    }
    pub fn list_current_versions(&self, config: &Config) -> Vec<(Arc<Tool>, ToolVersion)> {
        self.list_versions_by_plugin(config)
            .iter()
            .flat_map(|(p, v)| v.iter().map(|v| (p.clone(), v.clone())))
            .collect()
    }
    pub fn list_current_installed_versions(
        &self,
        config: &Config,
    ) -> Vec<(Arc<Tool>, ToolVersion)> {
        self.list_current_versions(config)
            .into_iter()
            .filter(|(p, v)| p.is_version_installed(v))
            .collect()
    }
    pub fn list_outdated_versions(&self, config: &Config) -> Vec<(Arc<Tool>, ToolVersion, String)> {
        self.list_current_versions(config)
            .into_iter()
            .filter_map(|(t, tv)| {
                if t.symlink_path(&tv).is_some() {
                    // do not consider symlinked versions to be outdated
                    return None;
                }
                let latest = match tv.latest_version(config, &t) {
                    Ok(latest) => latest,
                    Err(e) => {
                        warn!("Error getting latest version for {}: {:#}", t.name, e);
                        return None;
                    }
                };
                if !t.is_version_installed(&tv) || tv.version != latest {
                    Some((t, tv, latest))
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn env_with_path(&self, config: &Config) -> BTreeMap<String, String> {
        let mut env = self.env(config);
        let path_env = self.path_env(config);
        env.insert("PATH".to_string(), path_env);
        env
    }
    pub fn env(&self, config: &Config) -> BTreeMap<String, String> {
        let mut entries: BTreeMap<String, String> = self
            .list_current_installed_versions(config)
            .into_par_iter()
            .flat_map(|(p, tv)| match p.exec_env(config, &tv) {
                Ok(env) => env.into_iter().collect(),
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
        self.list_current_installed_versions(config)
            .into_par_iter()
            .flat_map(|(p, tv)| match p.list_bin_paths(config, &tv) {
                Ok(paths) => paths,
                Err(e) => {
                    warn!("Error listing bin paths for {}: {:#}", tv, e);
                    Vec::new()
                }
            })
            .collect()
    }
    pub fn which(&self, config: &Config, bin_name: &str) -> Option<(Arc<Tool>, ToolVersion)> {
        self.list_current_installed_versions(config)
            .into_par_iter()
            .find_first(|(p, tv)| {
                if let Ok(x) = p.which(config, tv, bin_name) {
                    x.is_some()
                } else {
                    false
                }
            })
    }

    pub fn list_rtvs_with_bin(&self, config: &Config, bin_name: &str) -> Result<Vec<ToolVersion>> {
        Ok(self
            .list_installed_versions(config)?
            .into_par_iter()
            .filter(|(p, tv)| match p.which(config, tv, bin_name) {
                Ok(x) => x.is_some(),
                Err(e) => {
                    warn!("Error running which: {:#}", e);
                    false
                }
            })
            .map(|(_, tv)| tv)
            .collect())
    }
}

impl Display for Toolset {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let plugins = &self
            .versions
            .iter()
            .map(|(_, v)| v.requests.iter().map(|(tvr, _)| tvr.to_string()).join(" "))
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
