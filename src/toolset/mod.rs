use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use console::truncate_str;
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use rayon::prelude::*;

pub use builder::ToolsetBuilder;
pub use tool_source::ToolSource;
pub use tool_version::ToolVersion;
pub use tool_version_list::ToolVersionList;
pub use tool_version_request::ToolVersionRequest;

use crate::cli::args::ForgeArg;
use crate::config::settings::SettingsStatusMissingTools;
use crate::config::{Config, Settings};
use crate::env::TERM_WIDTH;
use crate::forge::Forge;
use crate::install_context::InstallContext;
use crate::path_env::PathEnv;
use crate::runtime_symlinks;
use crate::shims;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{env, forge};

mod builder;
mod tool_source;
mod tool_version;
mod tool_version_list;
mod tool_version_request;

pub type ToolVersionOptions = BTreeMap<String, String>;

#[derive(Debug, Default)]
pub struct InstallOptions {
    pub force: bool,
    pub jobs: Option<usize>,
    pub raw: bool,
    pub latest_versions: bool,
}

impl InstallOptions {
    pub fn new() -> Self {
        let settings = Settings::get();
        InstallOptions {
            jobs: Some(settings.jobs),
            raw: settings.raw,
            ..Default::default()
        }
    }
}

/// a toolset is a collection of tools for various plugins
///
/// one example is a .tool-versions file
/// the idea is that we start with an empty toolset, then
/// merge in other toolsets from various sources
#[derive(Debug, Default, Clone)]
pub struct Toolset {
    pub versions: IndexMap<ForgeArg, ToolVersionList>,
    pub source: Option<ToolSource>,
    pub disable_tools: HashSet<ForgeArg>,
}

impl Toolset {
    pub fn new(source: ToolSource) -> Self {
        Self {
            source: Some(source),
            ..Default::default()
        }
    }
    pub fn add_version(&mut self, tvr: ToolVersionRequest, opts: ToolVersionOptions) {
        let fa = tvr.forge();
        if self.disable_tools.contains(fa) {
            return;
        }
        let tvl = self
            .versions
            .entry(tvr.forge().clone())
            .or_insert_with(|| ToolVersionList::new(fa.clone(), self.source.clone().unwrap()));
        tvl.requests.push((tvr, opts));
    }
    pub fn merge(&mut self, other: Toolset) {
        let mut versions = other.versions;
        for (plugin, tvl) in self.versions.clone() {
            if !versions.contains_key(&plugin) {
                versions.insert(plugin, tvl);
            }
        }
        versions.retain(|_, tvl| !self.disable_tools.contains(&tvl.forge));
        self.versions = versions;
        self.source = other.source;
    }
    pub fn resolve(&mut self) {
        self.list_missing_plugins();
        self.versions
            .iter_mut()
            .collect::<Vec<_>>()
            .par_iter_mut()
            .for_each(|(_, v)| v.resolve(false));
    }
    pub fn install_arg_versions(&mut self, config: &Config, opts: &InstallOptions) -> Result<()> {
        let mpr = MultiProgressReport::get();
        let versions = self
            .list_current_versions()
            .into_iter()
            .filter(|(p, tv)| opts.force || !p.is_version_installed(tv))
            .map(|(_, tv)| tv)
            .filter(|tv| matches!(self.versions[&tv.forge].source, ToolSource::Argument))
            .collect_vec();
        self.install_versions(config, versions, &mpr, opts)
    }

    pub fn list_missing_plugins(&self) -> Vec<String> {
        self.versions
            .keys()
            .map(forge::get)
            .filter(|p| !p.is_installed())
            .map(|p| p.id().into())
            .collect()
    }

    pub fn install_versions(
        &mut self,
        config: &Config,
        versions: Vec<ToolVersion>,
        mpr: &MultiProgressReport,
        opts: &InstallOptions,
    ) -> Result<()> {
        if versions.is_empty() {
            return Ok(());
        }
        let settings = Settings::try_get()?;
        let queue: Vec<_> = versions
            .into_iter()
            .rev()
            .group_by(|v| v.forge.clone())
            .into_iter()
            .map(|(fa, v)| (forge::get(&fa), v.collect_vec()))
            .collect();
        for (t, _) in &queue {
            if !t.is_installed() {
                t.ensure_installed(mpr, false)?;
            }
        }
        let queue = Arc::new(Mutex::new(queue));
        let raw = opts.raw || settings.raw;
        let jobs = match raw {
            true => 1,
            false => opts.jobs.unwrap_or(settings.jobs),
        };
        let installing: HashSet<String> = HashSet::new();
        let installing = Arc::new(Mutex::new(installing));
        thread::scope(|s| {
            (0..jobs)
                .map(|_| {
                    let queue = queue.clone();
                    let installing = installing.clone();
                    let ts = &*self;
                    s.spawn(move || {
                        let next_job = || queue.lock().unwrap().pop();
                        while let Some((t, versions)) = next_job() {
                            installing.lock().unwrap().insert(t.id().into());
                            for tv in versions {
                                for dep in t.get_dependencies(&tv)? {
                                    while installing.lock().unwrap().contains(dep.as_str()) {
                                        trace!(
                                            "{tv} waiting for dependency {} to finish installing",
                                            dep
                                        );
                                        sleep(Duration::from_millis(100));
                                    }
                                }
                                let tv = tv.request.resolve(
                                    t.as_ref(),
                                    tv.opts.clone(),
                                    opts.latest_versions,
                                )?;
                                let ctx = InstallContext {
                                    ts,
                                    pr: mpr.add(&tv.style()),
                                    tv,
                                    force: opts.force,
                                };
                                t.install_version(ctx)?;
                            }
                            installing.lock().unwrap().remove(t.id());
                        }
                        Ok(())
                    })
                })
                .collect_vec()
                .into_iter()
                .map(|t| t.join().unwrap())
                .collect::<Result<Vec<()>>>()
        })?;
        self.resolve();
        shims::reshim(self)?;
        runtime_symlinks::rebuild(config)
    }
    pub fn list_missing_versions(&self) -> Vec<ToolVersion> {
        self.list_current_versions()
            .into_iter()
            .filter(|(p, tv)| !p.is_version_installed(tv))
            .map(|(_, tv)| tv)
            .collect()
    }
    pub fn list_installed_versions(&self) -> Result<Vec<(Arc<dyn Forge>, ToolVersion)>> {
        let current_versions: HashMap<(String, String), (Arc<dyn Forge>, ToolVersion)> = self
            .list_current_versions()
            .into_iter()
            .map(|(p, tv)| ((p.id().into(), tv.version.clone()), (p.clone(), tv)))
            .collect();
        let versions = forge::list()
            .into_par_iter()
            .map(|p| {
                let versions = p.list_installed_versions()?;
                Ok(versions
                    .into_iter()
                    .map(
                        |v| match current_versions.get(&(p.id().into(), v.clone())) {
                            Some((p, tv)) => (p.clone(), tv.clone()),
                            None => {
                                let tv = ToolVersionRequest::new(p.fa().clone(), &v)
                                    .resolve(p.as_ref(), Default::default(), false)
                                    .unwrap();
                                (p.clone(), tv)
                            }
                        },
                    )
                    .collect_vec())
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();

        Ok(versions)
    }
    pub fn list_plugins(&self) -> Vec<Arc<dyn Forge>> {
        self.list_versions_by_plugin()
            .into_iter()
            .map(|(p, _)| p)
            .collect()
    }
    pub fn list_versions_by_plugin(&self) -> Vec<(Arc<dyn Forge>, &Vec<ToolVersion>)> {
        self.versions
            .iter()
            .map(|(p, v)| (forge::get(p), &v.versions))
            .collect()
    }
    pub fn list_current_versions(&self) -> Vec<(Arc<dyn Forge>, ToolVersion)> {
        self.list_versions_by_plugin()
            .iter()
            .flat_map(|(p, v)| v.iter().map(|v| (p.clone(), v.clone())))
            .collect()
    }
    pub fn list_current_installed_versions(&self) -> Vec<(Arc<dyn Forge>, ToolVersion)> {
        self.list_current_versions()
            .into_iter()
            .filter(|(p, v)| p.is_version_installed(v))
            .collect()
    }
    pub fn list_outdated_versions(&self) -> Vec<(Arc<dyn Forge>, ToolVersion, String)> {
        self.list_current_versions()
            .into_iter()
            .filter_map(|(t, tv)| {
                if t.symlink_path(&tv).is_some() {
                    // do not consider symlinked versions to be outdated
                    return None;
                }
                let latest = match tv.latest_version(t.as_ref()) {
                    Ok(latest) => latest,
                    Err(e) => {
                        warn!("Error getting latest version for {t}: {e:#}");
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
    pub fn env_with_path(&self, config: &Config) -> Result<BTreeMap<String, String>> {
        let mut path_env = PathEnv::from_iter(env::PATH.clone());
        for p in config.path_dirs()?.clone() {
            path_env.add(p);
        }
        let mut env = self.env(config)?;
        if let Some(path) = env.get("PATH") {
            path_env.add(PathBuf::from(path));
        }
        for p in self.list_paths() {
            path_env.add(p);
        }
        env.insert("PATH".to_string(), path_env.to_string());
        Ok(env)
    }
    pub fn env(&self, config: &Config) -> Result<BTreeMap<String, String>> {
        let entries = self
            .list_current_installed_versions()
            .into_par_iter()
            .filter(|(_, tv)| !matches!(tv.request, ToolVersionRequest::System(_)))
            .flat_map(|(p, tv)| match p.exec_env(config, self, &tv) {
                Ok(env) => env.into_iter().collect(),
                Err(e) => {
                    warn!("Error running exec-env: {:#}", e);
                    Vec::new()
                }
            })
            .collect::<Vec<(String, String)>>();
        let add_paths = entries
            .iter()
            .filter(|(k, _)| k == "MISE_ADD_PATH" || k == "RTX_ADD_PATH")
            .map(|(_, v)| v)
            .join(":");
        let mut entries: BTreeMap<String, String> = entries
            .into_iter()
            .filter(|(k, _)| k != "RTX_ADD_PATH")
            .filter(|(k, _)| k != "MISE_ADD_PATH")
            .filter(|(k, _)| !k.starts_with("RTX_TOOL_OPTS__"))
            .filter(|(k, _)| !k.starts_with("MISE_TOOL_OPTS__"))
            .rev()
            .collect();
        if !add_paths.is_empty() {
            entries.insert("PATH".to_string(), add_paths);
        }
        entries.extend(config.env()?.clone());
        Ok(entries)
    }
    pub fn list_paths(&self) -> Vec<PathBuf> {
        self.list_current_installed_versions()
            .into_par_iter()
            .filter(|(_, tv)| !matches!(tv.request, ToolVersionRequest::System(_)))
            .flat_map(|(p, tv)| {
                p.list_bin_paths(&tv).unwrap_or_else(|e| {
                    warn!("Error listing bin paths for {tv}: {e:#}");
                    Vec::new()
                })
            })
            .collect()
    }
    pub fn which(&self, bin_name: &str) -> Option<(Arc<dyn Forge>, ToolVersion)> {
        self.list_current_installed_versions()
            .into_par_iter()
            .find_first(|(p, tv)| {
                if let Ok(x) = p.which(tv, bin_name) {
                    x.is_some()
                } else {
                    false
                }
            })
    }
    pub fn install_missing_bin(&mut self, bin_name: &str) -> Result<Option<Vec<ToolVersion>>> {
        let config = Config::try_get()?;
        let plugins = self
            .list_installed_versions()?
            .into_iter()
            .filter(|(p, tv)| {
                if let Ok(x) = p.which(tv, bin_name) {
                    x.is_some()
                } else {
                    false
                }
            })
            .collect_vec();
        for (plugin, _) in plugins {
            let versions = self
                .list_missing_versions()
                .into_iter()
                .filter(|tv| &tv.forge == plugin.fa())
                .collect_vec();
            if !versions.is_empty() {
                let mpr = MultiProgressReport::get();
                self.install_versions(&config, versions.clone(), &mpr, &InstallOptions::new())?;
                return Ok(Some(versions));
            }
        }
        Ok(None)
    }

    pub fn list_rtvs_with_bin(&self, bin_name: &str) -> Result<Vec<ToolVersion>> {
        Ok(self
            .list_installed_versions()?
            .into_par_iter()
            .filter(|(p, tv)| match p.which(tv, bin_name) {
                Ok(x) => x.is_some(),
                Err(e) => {
                    warn!("Error running which: {:#}", e);
                    false
                }
            })
            .map(|(_, tv)| tv)
            .collect())
    }

    // shows a warning if any versions are missing
    // only displays for tools which have at least one version already installed
    pub fn notify_if_versions_missing(&self) {
        let settings = Settings::get();
        let missing = self
            .list_missing_versions()
            .into_iter()
            .filter(|tv| match settings.status.missing_tools {
                SettingsStatusMissingTools::Never => false,
                SettingsStatusMissingTools::Always => true,
                SettingsStatusMissingTools::IfOtherVersionsInstalled => tv
                    .get_forge()
                    .list_installed_versions()
                    .is_ok_and(|f| !f.is_empty()),
            })
            .collect_vec();
        if missing.is_empty() {
            return;
        }
        let versions = missing
            .iter()
            .map(|tv| tv.style())
            .collect::<Vec<_>>()
            .join(" ");
        warn!(
            "missing: {}",
            truncate_str(&versions, *TERM_WIDTH - 14, "…"),
        );
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
