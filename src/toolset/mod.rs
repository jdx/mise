use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{panic, thread};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::settings::{SettingsStatusMissingTools, SETTINGS};
use crate::config::Config;
use crate::env::{PATH_KEY, TERM_WIDTH};
use crate::errors::Error;
use crate::hooks::Hooks;
use crate::install_context::InstallContext;
use crate::path_env::PathEnv;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::uv::UV_VENV;
use crate::{backend, config, env, hooks};
pub use builder::ToolsetBuilder;
use console::truncate_str;
use eyre::{eyre, Result, WrapErr};
use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
pub use outdated_info::is_outdated_version;
use outdated_info::OutdatedInfo;
use rayon::prelude::*;
pub use tool_request::ToolRequest;
pub use tool_request_set::{ToolRequestSet, ToolRequestSetBuilder};
pub use tool_source::ToolSource;
pub use tool_version::{ResolveOptions, ToolVersion};
pub use tool_version_list::ToolVersionList;

mod builder;
pub(crate) mod install_state;
pub(crate) mod outdated_info;
pub(crate) mod tool_request;
mod tool_request_set;
mod tool_source;
mod tool_version;
mod tool_version_list;

pub type ToolVersionOptions = BTreeMap<String, String>;

pub fn parse_tool_options(s: &str) -> ToolVersionOptions {
    let mut opts = ToolVersionOptions::new();
    for opt in s.split(',') {
        let (k, v) = opt.split_once('=').unwrap_or((opt, ""));
        if k.is_empty() {
            continue;
        }
        opts.insert(k.to_string(), v.to_string());
    }
    opts
}

#[derive(Debug)]
pub struct InstallOptions {
    pub force: bool,
    pub jobs: Option<usize>,
    pub raw: bool,
    /// only install missing tools if passed as arguments
    pub missing_args_only: bool,
    pub missing_tools_auto_install: Option<Vec<String>>,
    pub resolve_options: ResolveOptions,
}

impl Default for InstallOptions {
    fn default() -> Self {
        InstallOptions {
            jobs: Some(SETTINGS.jobs),
            raw: SETTINGS.raw,
            force: false,
            missing_args_only: true,
            missing_tools_auto_install: None,
            resolve_options: Default::default(),
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
    pub versions: IndexMap<BackendArg, ToolVersionList>,
    pub source: Option<ToolSource>,
}

impl Toolset {
    pub fn new(source: ToolSource) -> Self {
        Self {
            source: Some(source),
            ..Default::default()
        }
    }
    pub fn add_version(&mut self, tvr: ToolRequest) {
        let ba = tvr.ba();
        if self.is_disabled(ba) {
            return;
        }
        let tvl = self
            .versions
            .entry(tvr.ba().clone())
            .or_insert_with(|| ToolVersionList::new(ba.clone(), self.source.clone().unwrap()));
        tvl.requests.push(tvr);
    }
    pub fn merge(&mut self, other: Toolset) {
        let mut versions = other.versions;
        for (plugin, tvl) in self.versions.clone() {
            if !versions.contains_key(&plugin) {
                versions.insert(plugin, tvl);
            }
        }
        versions.retain(|_, tvl| !self.is_disabled(&tvl.backend));
        self.versions = versions;
        self.source = other.source;
    }
    pub fn resolve(&mut self) -> eyre::Result<()> {
        self.list_missing_plugins();
        let errors = self
            .versions
            .iter_mut()
            .collect::<Vec<_>>()
            .par_iter_mut()
            .map(|(_, v)| v.resolve(&Default::default()))
            .filter(|r| r.is_err())
            .map(|r| r.unwrap_err())
            .collect::<Vec<_>>();
        match errors.is_empty() {
            true => Ok(()),
            false => {
                let err = eyre!("error resolving versions");
                Err(errors.into_iter().fold(err, |e, x| e.wrap_err(x)))
            }
        }
    }
    pub fn install_missing_versions(&mut self, opts: &InstallOptions) -> Result<Vec<ToolVersion>> {
        let mpr = MultiProgressReport::get();
        let versions = self
            .list_missing_versions()
            .into_iter()
            .filter(|tv| {
                !opts.missing_args_only
                    || matches!(self.versions[tv.ba()].source, ToolSource::Argument)
            })
            .filter(|tv| {
                if let Some(missing_tools_auto_install) = &opts.missing_tools_auto_install {
                    missing_tools_auto_install.contains(&tv.ba().short)
                } else {
                    true
                }
            })
            .map(|tv| tv.request)
            .collect_vec();
        let versions = self.install_all_versions(versions, &mpr, opts)?;
        if !versions.is_empty() {
            config::rebuild_shims_and_runtime_symlinks(&versions)?;
        }
        Ok(versions)
    }

    pub fn list_missing_plugins(&self) -> Vec<String> {
        self.versions
            .iter()
            .filter(|(_, tvl)| {
                tvl.versions
                    .first()
                    .map(|tv| tv.request.is_os_supported())
                    .unwrap_or_default()
            })
            .map(|(ba, _)| ba)
            .flat_map(|ba| ba.backend())
            .filter(|b| b.plugin().is_some_and(|p| !p.is_installed()))
            .map(|p| p.id().into())
            .collect()
    }

    /// sets the options on incoming requests to install to whatever is already in the toolset
    /// this handles the use-case where you run `mise use ubi:cilium/cilium-cli` (without CLi options)
    /// but this tool has options inside mise.toml
    fn init_request_options(&self, requests: &mut Vec<ToolRequest>) {
        for tr in requests {
            // TODO: tr.options() probably should be Option<ToolVersionOptions>
            // to differentiate between no options and empty options
            // without that it might not be possible to unset the options if they are set
            if !tr.options().is_empty() {
                continue;
            }
            if let Some(tvl) = self.versions.get(tr.ba()) {
                if tvl.requests.len() != 1 {
                    // TODO: handle this case with multiple versions
                    continue;
                }
                let options = tvl.requests[0].options();
                tr.set_options(options);
            }
        }
    }

    pub fn install_all_versions(
        &mut self,
        mut versions: Vec<ToolRequest>,
        mpr: &MultiProgressReport,
        opts: &InstallOptions,
    ) -> Result<Vec<ToolVersion>> {
        if versions.is_empty() {
            return Ok(vec![]);
        }
        hooks::run_one_hook(self, Hooks::Preinstall);
        self.init_request_options(&mut versions);
        show_python_install_hint(&versions);
        let mut installed = vec![];
        let mut leaf_deps = get_leaf_dependencies(&versions)?;
        while !leaf_deps.is_empty() {
            if leaf_deps.len() < versions.len() {
                debug!("installing {} leaf tools first", leaf_deps.len());
            }
            versions.retain(|tr| !leaf_deps.contains(tr));
            installed.extend(self.install_some_versions(leaf_deps, mpr, opts)?);
            leaf_deps = get_leaf_dependencies(&versions)?;
        }

        trace!("install: resolving");
        install_state::reset();
        if let Err(err) = self.resolve() {
            debug!("error resolving versions after install: {err:#}");
        }
        if log::log_enabled!(log::Level::Debug) {
            for tv in installed.iter() {
                let backend = tv.backend()?;
                let bin_paths = backend
                    .list_bin_paths(tv)
                    .map_err(|e| {
                        warn!("Error listing bin paths for {tv}: {e:#}");
                    })
                    .unwrap_or_default();
                debug!("[{tv}] list_bin_paths: {bin_paths:?}");
                let env = backend
                    .exec_env(&Config::get(), self, tv)
                    .map_err(|e| {
                        warn!("Error running exec-env: {e:#}");
                    })
                    .unwrap_or_default();
                if !env.is_empty() {
                    debug!("[{tv}] exec_env: {env:?}");
                }
            }
        }
        hooks::run_one_hook(self, Hooks::Postinstall);
        Ok(installed)
    }

    fn install_some_versions(
        &mut self,
        versions: Vec<ToolRequest>,
        mpr: &MultiProgressReport,
        opts: &InstallOptions,
    ) -> Result<Vec<ToolVersion>> {
        debug!("install_some_versions: {}", versions.iter().join(" "));
        let queue: Vec<_> = versions
            .into_iter()
            .rev()
            .chunk_by(|v| v.ba().clone())
            .into_iter()
            .map(|(ba, v)| Ok((ba.backend()?, v.collect_vec())))
            .collect::<Result<_>>()?;
        for (backend, _) in &queue {
            if let Some(plugin) = backend.plugin() {
                if !plugin.is_installed() {
                    plugin.ensure_installed(mpr, false).or_else(|err| {
                        if let Some(&Error::PluginNotInstalled(_)) = err.downcast_ref::<Error>() {
                            Ok(())
                        } else {
                            Err(err)
                        }
                    })?;
                }
            }
        }
        let queue = Arc::new(Mutex::new(queue));
        let raw = opts.raw || SETTINGS.raw;
        let jobs = match raw {
            true => 1,
            false => opts.jobs.unwrap_or(SETTINGS.jobs),
        };
        thread::scope(|s| {
            #[allow(clippy::map_collect_result_unit)]
            (0..jobs)
                .map(|_| {
                    let queue = queue.clone();
                    let ts = &*self;
                    s.spawn(move || {
                        let next_job = || queue.lock().unwrap().pop();
                        let mut installed = vec![];
                        while let Some((t, versions)) = next_job() {
                            for tr in versions {
                                let tv = tr.resolve(&opts.resolve_options)?;
                                let ctx = InstallContext {
                                    ts,
                                    pr: mpr.add(&tv.style()),
                                    force: opts.force,
                                };
                                let old_tv = tv.clone();
                                let tv = t
                                    .install_version(ctx, tv)
                                    .wrap_err_with(|| format!("failed to install {old_tv}"))?;
                                installed.push(tv);
                            }
                        }
                        Ok(installed)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|t| match t.join() {
                    Ok(x) => x,
                    Err(e) => panic::resume_unwind(e),
                })
                .collect::<Result<Vec<Vec<ToolVersion>>>>()
                .map(|x| x.into_iter().flatten().rev().collect())
        })
    }

    pub fn list_missing_versions(&self) -> Vec<ToolVersion> {
        self.list_current_versions()
            .into_iter()
            .filter(|(p, tv)| tv.request.is_os_supported() && !p.is_version_installed(tv, true))
            .map(|(_, tv)| tv)
            .collect()
    }
    pub fn list_installed_versions(&self) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
        let current_versions: HashMap<(String, String), (Arc<dyn Backend>, ToolVersion)> = self
            .list_current_versions()
            .into_iter()
            .map(|(p, tv)| ((p.id().into(), tv.version.clone()), (p.clone(), tv)))
            .collect();
        let versions = backend::list()
            .into_par_iter()
            .map(|p| {
                let versions = p.list_installed_versions()?;
                versions
                    .into_iter()
                    .map(
                        |v| match current_versions.get(&(p.id().into(), v.clone())) {
                            Some((p, tv)) => Ok((p.clone(), tv.clone())),
                            None => {
                                let tv = ToolRequest::new(p.ba().clone(), &v, ToolSource::Unknown)?
                                    .resolve(&Default::default())
                                    .unwrap();
                                Ok((p.clone(), tv))
                            }
                        },
                    )
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();

        Ok(versions)
    }
    pub fn list_current_requests(&self) -> Vec<&ToolRequest> {
        self.versions
            .values()
            .flat_map(|tvl| &tvl.requests)
            .collect()
    }
    pub fn list_versions_by_plugin(&self) -> Vec<(Arc<dyn Backend>, &Vec<ToolVersion>)> {
        self.versions
            .iter()
            .flat_map(|(ba, v)| eyre::Ok((ba.backend()?, &v.versions)))
            .collect()
    }
    pub fn list_current_versions(&self) -> Vec<(Arc<dyn Backend>, ToolVersion)> {
        self.list_versions_by_plugin()
            .iter()
            .flat_map(|(p, v)| {
                v.iter().map(|v| {
                    // map cargo backend specific prefixes to ref
                    let tv = match v.version.split_once(':') {
                        Some((ref_type @ ("tag" | "branch" | "rev"), r)) => {
                            let request = ToolRequest::Ref {
                                backend: p.ba().clone(),
                                ref_: r.to_string(),
                                ref_type: ref_type.to_string(),
                                os: v.request.os().clone(),
                                options: v.request.options().clone(),
                                source: v.request.source().clone(),
                            };
                            let version = format!("ref:{r}");
                            ToolVersion::new(request, version)
                        }
                        _ => v.clone(),
                    };
                    (p.clone(), tv)
                })
            })
            .collect()
    }
    pub fn list_all_versions(&self) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
        let versions = self
            .list_current_versions()
            .into_iter()
            .chain(self.list_installed_versions()?)
            .unique_by(|(ba, tv)| (ba.clone(), tv.tv_pathname().to_string()))
            .collect();
        Ok(versions)
    }
    pub fn list_current_installed_versions(&self) -> Vec<(Arc<dyn Backend>, ToolVersion)> {
        self.list_current_versions()
            .into_iter()
            .filter(|(p, v)| p.is_version_installed(v, true))
            .collect()
    }
    pub fn list_outdated_versions(&self, bump: bool) -> Vec<OutdatedInfo> {
        self.list_current_versions()
            .into_iter()
            .filter_map(|(t, tv)| {
                match t.outdated_info(&tv, bump) {
                    Ok(Some(oi)) => return Some(oi),
                    Ok(None) => {}
                    Err(e) => {
                        warn!("Error getting outdated info for {tv}: {e:#}");
                        return None;
                    }
                }
                if t.symlink_path(&tv).is_some() {
                    trace!("skipping symlinked version {tv}");
                    // do not consider symlinked versions to be outdated
                    return None;
                }
                OutdatedInfo::resolve(tv.clone(), bump).unwrap_or_else(|e| {
                    warn!("Error creating OutdatedInfo for {tv}: {e:#}");
                    None
                })
            })
            .collect()
    }
    pub fn full_env(&self) -> Result<BTreeMap<String, String>> {
        let mut env = env::PRISTINE_ENV
            .clone()
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        env.extend(self.env_with_path(&Config::get())?);
        Ok(env)
    }
    pub fn env_with_path(&self, config: &Config) -> Result<BTreeMap<String, String>> {
        let mut env = self.env(config)?;
        let mut path_env = PathEnv::from_iter(env::PATH.clone());
        for p in self.list_final_paths()? {
            path_env.add(p);
        }
        env.insert(PATH_KEY.to_string(), path_env.to_string());
        Ok(env)
    }
    pub fn env_from_tools(&self, config: &Config) -> Vec<(String, String, String)> {
        self.list_current_installed_versions()
            .into_par_iter()
            .filter(|(_, tv)| !matches!(tv.request, ToolRequest::System { .. }))
            .flat_map(|(p, tv)| match p.exec_env(config, self, &tv) {
                Ok(env) => env
                    .into_iter()
                    .map(|(k, v)| (k, v, p.id().into()))
                    .collect(),
                Err(e) => {
                    warn!("Error running exec-env: {:#}", e);
                    Vec::new()
                }
            })
            .filter(|(_, k, _)| k.to_uppercase() != "PATH")
            .collect::<Vec<(String, String, String)>>()
    }
    pub fn env(&self, config: &Config) -> Result<BTreeMap<String, String>> {
        time!("env start");
        let entries = self
            .env_from_tools(config)
            .into_iter()
            .map(|(k, v, _)| (k, v))
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
            entries.insert(PATH_KEY.to_string(), add_paths);
        }
        entries.extend(config.env()?.clone());
        if let Some(venv) = &*UV_VENV {
            for (k, v) in &venv.env {
                entries.insert(k.clone(), v.clone());
            }
        }
        time!("env end");
        Ok(entries)
    }
    pub fn list_paths(&self) -> Vec<PathBuf> {
        self.list_current_installed_versions()
            .into_par_iter()
            .filter(|(_, tv)| !matches!(tv.request, ToolRequest::System { .. }))
            .flat_map(|(p, tv)| {
                p.list_bin_paths(&tv).unwrap_or_else(|e| {
                    warn!("Error listing bin paths for {tv}: {e:#}");
                    Vec::new()
                })
            })
            .filter(|p| p.parent().is_some())
            .collect()
    }
    /// same as list_paths but includes config.list_paths, venv paths, and MISE_ADD_PATHs from self.env()
    pub fn list_final_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = IndexSet::new();
        for p in Config::get().path_dirs()?.clone() {
            paths.insert(p);
        }
        if let Some(venv) = &*UV_VENV {
            paths.insert(venv.venv_path.clone());
        }
        if let Some(path) = self.env(&Config::get())?.get(&*PATH_KEY) {
            paths.insert(PathBuf::from(path));
        }
        for p in self.list_paths() {
            paths.insert(p);
        }
        Ok(paths.into_iter().collect())
    }
    pub fn which(&self, bin_name: &str) -> Option<(Arc<dyn Backend>, ToolVersion)> {
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
    pub fn which_bin(&self, bin_name: &str) -> Option<PathBuf> {
        self.which(bin_name)
            .and_then(|(p, tv)| p.which(&tv, bin_name).ok())
            .flatten()
    }
    pub fn install_missing_bin(&mut self, bin_name: &str) -> Result<Option<Vec<ToolVersion>>> {
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
                .filter(|tv| tv.ba() == plugin.ba())
                .map(|tv| tv.request)
                .collect_vec();
            if !versions.is_empty() {
                let mpr = MultiProgressReport::get();
                let versions =
                    self.install_all_versions(versions.clone(), &mpr, &InstallOptions::default())?;
                if !versions.is_empty() {
                    config::rebuild_shims_and_runtime_symlinks(&versions)?;
                }
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
        let missing = self
            .list_missing_versions()
            .into_iter()
            .filter(|tv| match SETTINGS.status.missing_tools() {
                SettingsStatusMissingTools::Never => false,
                SettingsStatusMissingTools::Always => true,
                SettingsStatusMissingTools::IfOtherVersionsInstalled => tv
                    .backend()
                    .is_ok_and(|b| b.list_installed_versions().is_ok_and(|f| !f.is_empty())),
            })
            .collect_vec();
        if missing.is_empty() || *env::__MISE_SHIM {
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

    fn is_disabled(&self, ba: &BackendArg) -> bool {
        !ba.is_os_supported() || SETTINGS.disable_tools().contains(&ba.short)
    }
}

fn show_python_install_hint(versions: &[ToolRequest]) {
    let num_python = versions
        .iter()
        .filter(|tr| tr.ba().tool_name == "python")
        .count();
    if num_python != 1 {
        return;
    }
    hint!(
        "python_multi",
        "use multiple versions simultaneously with",
        "mise use python@3.12 python@3.11"
    );
}

impl Display for Toolset {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let plugins = &self
            .versions
            .iter()
            .map(|(_, v)| v.requests.iter().map(|tvr| tvr.to_string()).join(" "))
            .collect_vec();
        write!(f, "{}", plugins.join(", "))
    }
}

impl From<ToolRequestSet> for Toolset {
    fn from(trs: ToolRequestSet) -> Self {
        let mut ts = Toolset::default();
        for (ba, versions, source) in trs.into_iter() {
            ts.source = Some(source.clone());
            let mut tvl = ToolVersionList::new(ba.clone(), source);
            for tr in versions {
                tvl.requests.push(tr);
            }
            ts.versions.insert(ba, tvl);
        }
        ts
    }
}

fn get_leaf_dependencies(requests: &[ToolRequest]) -> eyre::Result<Vec<ToolRequest>> {
    // reverse maps potential shorts like "cargo-binstall" for "cargo:cargo-binstall"
    let versions_hash = requests
        .iter()
        .flat_map(|tr| tr.ba().all_fulls())
        .collect::<HashSet<_>>();
    let leaves = requests
        .iter()
        .map(|tr| {
            match tr.backend()?.get_all_dependencies(true)?.iter().all(|dep| {
                // dep is a dependency of tr so if it is in versions_hash (meaning it's also being installed) then it is not a leaf node
                !dep.all_fulls()
                    .iter()
                    .any(|full| versions_hash.contains(full))
            }) {
                true => Ok(Some(tr)),
                false => Ok(None),
            }
        })
        .flatten_ok()
        .map_ok(|tr| tr.clone())
        .collect::<Result<Vec<_>>>()?;
    Ok(leaves)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use test_log::test;

    use super::ToolVersionOptions;
    #[test]
    fn test_tool_version_options() {
        let t = |input, f| {
            let opts = super::parse_tool_options(input);
            assert_eq!(opts, f);
        };
        t("", ToolVersionOptions::new());
        t(
            "exe=rg",
            [("exe".to_string(), "rg".to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        t(
            "exe=rg,match=musl",
            [
                ("exe".to_string(), "rg".to_string()),
                ("match".to_string(), "musl".to_string()),
            ]
            .iter()
            .cloned()
            .collect(),
        );
    }
}
