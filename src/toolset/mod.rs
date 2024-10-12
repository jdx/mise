use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use std::{panic, thread};

pub use builder::ToolsetBuilder;
use console::truncate_str;
use eyre::{eyre, Result};
use indexmap::IndexMap;
use itertools::Itertools;
use rayon::prelude::*;
use serde_derive::Serialize;
use tabled::Tabled;
pub use tool_request::ToolRequest;
pub use tool_request_set::{ToolRequestSet, ToolRequestSetBuilder};
pub use tool_source::ToolSource;
pub use tool_version::ToolVersion;
pub use tool_version_list::ToolVersionList;
use versions::{Version, Versioning};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::settings::{SettingsStatusMissingTools, SETTINGS};
use crate::config::Config;
use crate::env::{PATH_KEY, TERM_WIDTH};
use crate::errors::Error;
use crate::install_context::InstallContext;
use crate::path_env::PathEnv;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{backend, env, runtime_symlinks, shims};

mod builder;
mod tool_request;
mod tool_request_set;
mod tool_source;
mod tool_version;
mod tool_version_list;

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
        InstallOptions {
            jobs: Some(SETTINGS.jobs),
            raw: SETTINGS.raw,
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
        let fa = tvr.backend();
        if self.is_disabled(fa) {
            return;
        }
        let tvl = self
            .versions
            .entry(tvr.backend().clone())
            .or_insert_with(|| ToolVersionList::new(fa.clone(), self.source.clone().unwrap()));
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
            .map(|(_, v)| v.resolve(false))
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
    pub fn install_arg_versions(
        &mut self,
        config: &Config,
        opts: &InstallOptions,
    ) -> Result<Vec<ToolVersion>> {
        let mpr = MultiProgressReport::get();
        let versions = self
            .list_current_versions()
            .into_iter()
            .filter(|(p, tv)| opts.force || !p.is_version_installed(tv, true))
            .map(|(_, tv)| tv)
            .filter(|tv| matches!(self.versions[&tv.backend].source, ToolSource::Argument))
            .map(|tv| tv.request)
            .collect_vec();
        self.install_versions(config, versions, &mpr, opts)
    }

    pub fn list_missing_plugins(&self) -> Vec<String> {
        self.versions
            .keys()
            .map(backend::get)
            .filter(|b| b.plugin().is_some_and(|p| !p.is_installed()))
            .map(|p| p.id().into())
            .collect()
    }

    pub fn install_versions(
        &mut self,
        config: &Config,
        versions: Vec<ToolRequest>,
        mpr: &MultiProgressReport,
        opts: &InstallOptions,
    ) -> Result<Vec<ToolVersion>> {
        if versions.is_empty() {
            return Ok(vec![]);
        }
        show_python_install_hint(&versions);
        let leaf_deps = get_leaf_dependencies(&versions)?;
        if leaf_deps.len() < versions.len() {
            debug!("installing {} leaf tools first", leaf_deps.len());
            self.install_versions(config, leaf_deps.into_iter().cloned().collect(), mpr, opts)?;
        }
        debug!("install_versions: {}", versions.iter().join(" "));
        let queue: Vec<_> = versions
            .into_iter()
            .rev()
            .chunk_by(|v| v.backend().clone())
            .into_iter()
            .map(|(fa, v)| (backend::get(&fa), v.collect_vec()))
            .collect();
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
        let installing: HashSet<String> = HashSet::new();
        let installing = Arc::new(Mutex::new(installing));
        let installed = thread::scope(|s| {
            #[allow(clippy::map_collect_result_unit)]
            (0..jobs)
                .map(|_| {
                    let queue = queue.clone();
                    let installing = installing.clone();
                    let ts = &*self;
                    s.spawn(move || {
                        let next_job = || queue.lock().unwrap().pop();
                        let mut installed = vec![];
                        while let Some((t, versions)) = next_job() {
                            installing.lock().unwrap().insert(t.id().into());
                            for tr in versions {
                                // TODO: this logic should be able to be removed now I think
                                for dep in t.get_all_dependencies(&tr)? {
                                    while installing.lock().unwrap().contains(&dep.to_string()) {
                                        trace!(
                                        "{tr} waiting for dependency {dep} to finish installing"
                                    );
                                        sleep(Duration::from_millis(100));
                                    }
                                }
                                let tv = tr.resolve(t.as_ref(), opts.latest_versions)?;
                                let ctx = InstallContext {
                                    ts,
                                    pr: mpr.add(&tv.style()),
                                    tv: tv.clone(),
                                    force: opts.force,
                                };
                                t.install_version(ctx)?;
                                installed.push(tv);
                            }
                            installing.lock().unwrap().remove(t.id());
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
        })?;
        trace!("install: resolving");
        if let Err(err) = self.resolve() {
            debug!("error resolving versions after install: {err:#}");
        }
        trace!("install: reshimming");
        shims::reshim(self, false)?;
        runtime_symlinks::rebuild(config)?;
        trace!("install: done");
        Ok(installed)
    }

    pub fn list_missing_versions(&self) -> Vec<ToolVersion> {
        self.list_current_versions()
            .into_iter()
            .filter(|(p, tv)| !p.is_version_installed(tv, true))
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
                                let tv = ToolRequest::new(p.fa().clone(), &v)?
                                    .resolve(p.as_ref(), false)
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
    pub fn list_plugins(&self) -> Vec<Arc<dyn Backend>> {
        self.list_versions_by_plugin()
            .into_iter()
            .map(|(p, _)| p)
            .collect()
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
            .map(|(p, v)| (backend::get(p), &v.versions))
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
                                backend: p.fa().clone(),
                                ref_: r.to_string(),
                                ref_type: ref_type.to_string(),
                                options: v.request.options().clone(),
                            };
                            let version = format!("ref:{r}");
                            ToolVersion::new(p.as_ref(), request, version)
                        }
                        _ => v.clone(),
                    };
                    (p.clone(), tv.clone())
                })
            })
            .collect()
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
                if t.symlink_path(&tv).is_some() {
                    trace!("skipping symlinked version {tv}");
                    // do not consider symlinked versions to be outdated
                    return None;
                }
                // prefix is something like "temurin-" or "corretto-"
                let prefix = regex!(r"^[a-zA-Z]+-")
                    .find(&tv.request.version())
                    .map(|m| m.as_str().to_string());
                let latest_result = if bump {
                    t.latest_version(prefix.clone())
                } else {
                    tv.latest_version(t.as_ref()).map(Option::from)
                };
                let mut out =
                    OutdatedInfo::new(tv.clone(), self.find_source(&tv.request).unwrap().clone());
                out.current = if t.is_version_installed(&tv, true) {
                    Some(tv.version.clone())
                } else {
                    None
                };
                out.latest = match latest_result {
                    Ok(Some(latest)) => latest,
                    Ok(None) => {
                        warn!("Error getting latest version for {t}: no latest version found");
                        return None;
                    }
                    Err(e) => {
                        warn!("Error getting latest version for {t}: {e:#}");
                        return None;
                    }
                };
                if out
                    .current
                    .as_ref()
                    .is_some_and(|c| !is_outdated_version(c, &out.latest))
                {
                    trace!("skipping up-to-date version {tv}");
                    return None;
                }
                if bump {
                    let prefix = prefix.unwrap_or_default();
                    let old = tv.request.version();
                    let old = old.strip_prefix(&prefix).unwrap_or_default();
                    let new = out.latest.strip_prefix(&prefix).unwrap_or_default();
                    if let Some(bumped_version) = check_semver_bump(old, new) {
                        if bumped_version != tv.request.version() {
                            out.bump = Some(format!("{prefix}{bumped_version}"));
                            match out.tool_request.clone() {
                                ToolRequest::Version {
                                    backend,
                                    version: _version,
                                    options,
                                } => {
                                    out.tool_request = ToolRequest::Version {
                                        backend,
                                        options,
                                        version: out.bump.clone().unwrap(),
                                    };
                                }
                                _ => {
                                    warn!("upgrading non-version tool requests");
                                    out.bump = None;
                                }
                            }
                        }
                    }
                }
                Some(out)
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
        let mut path_env = PathEnv::from_iter(env::PATH.clone());
        for p in config.path_dirs()?.clone() {
            path_env.add(p);
        }
        let mut env = self.env(config)?;
        if let Some(path) = env.get(&*PATH_KEY) {
            path_env.add(PathBuf::from(path));
        }
        for p in self.list_paths() {
            path_env.add(p);
        }
        env.insert(PATH_KEY.to_string(), path_env.to_string());
        Ok(env)
    }
    pub fn env(&self, config: &Config) -> Result<BTreeMap<String, String>> {
        let entries = self
            .list_current_installed_versions()
            .into_par_iter()
            .filter(|(_, tv)| !matches!(tv.request, ToolRequest::System(_)))
            .flat_map(|(p, tv)| match p.exec_env(config, self, &tv) {
                Ok(env) => env.into_iter().collect(),
                Err(e) => {
                    warn!("Error running exec-env: {:#}", e);
                    Vec::new()
                }
            })
            .filter(|(k, _)| k.to_uppercase() != "PATH")
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
        Ok(entries)
    }
    pub fn list_paths(&self) -> Vec<PathBuf> {
        self.list_current_installed_versions()
            .into_par_iter()
            .filter(|(_, tv)| !matches!(tv.request, ToolRequest::System(_)))
            .flat_map(|(p, tv)| {
                p.list_bin_paths(&tv).unwrap_or_else(|e| {
                    warn!("Error listing bin paths for {tv}: {e:#}");
                    Vec::new()
                })
            })
            .collect()
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
                .filter(|tv| &tv.backend == plugin.fa())
                .map(|tv| tv.request)
                .collect_vec();
            if !versions.is_empty() {
                let mpr = MultiProgressReport::get();
                let versions =
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
        let missing = self
            .list_missing_versions()
            .into_iter()
            .filter(|tv| match SETTINGS.status.missing_tools() {
                SettingsStatusMissingTools::Never => false,
                SettingsStatusMissingTools::Always => true,
                SettingsStatusMissingTools::IfOtherVersionsInstalled => tv
                    .get_backend()
                    .list_installed_versions()
                    .is_ok_and(|f| !f.is_empty()),
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

    fn is_disabled(&self, fa: &BackendArg) -> bool {
        let fa = fa.to_string();
        SETTINGS.disable_tools.iter().any(|s| s == &fa)
    }

    pub fn find_source(&self, tr: &ToolRequest) -> Option<&ToolSource> {
        self.versions.get(tr.backend()).map(|tvl| &tvl.source)
    }
}

fn show_python_install_hint(versions: &[ToolRequest]) {
    let num_python = versions
        .iter()
        .filter(|tr| tr.backend().name == "python")
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
        for (fa, versions, source) in trs.into_iter() {
            ts.source = Some(source.clone());
            let mut tvl = ToolVersionList::new(fa.clone(), source);
            for tr in versions {
                tvl.requests.push(tr);
            }
            ts.versions.insert(fa, tvl);
        }
        ts
    }
}

fn get_leaf_dependencies(requests: &[ToolRequest]) -> eyre::Result<Vec<&ToolRequest>> {
    let versions_hash = requests
        .iter()
        .map(|tr| tr.backend())
        .collect::<HashSet<_>>();
    let leaves = requests
        .iter()
        .map(|tr| {
            match tr
                .dependencies()?
                .iter()
                .all(|dep| !versions_hash.contains(dep))
            {
                true => Ok(Some(tr)),
                false => Ok(None),
            }
        })
        .flatten_ok()
        .collect::<Result<Vec<_>>>()?;
    Ok(leaves)
}

pub fn is_outdated_version(current: &str, latest: &str) -> bool {
    if let (Some(c), Some(l)) = (Version::new(current), Version::new(latest)) {
        c.lt(&l)
    } else {
        current != latest
    }
}

/// check if the new version is a bump from the old version and return the new version
/// at the same specifity level as the old version
/// used with `mise outdated --bump` to determine what new semver range to use
/// given old: "20" and new: "21.2.3", return Some("21")
fn check_semver_bump(old: &str, new: &str) -> Option<String> {
    let old = Versioning::new(old);
    let new = Versioning::new(new);
    let chunkify = |v: &Versioning| {
        let mut chunks = vec![];
        while let Some(chunk) = v.nth(chunks.len()) {
            chunks.push(chunk);
        }
        chunks
    };
    if let (Some(old), Some(new)) = (old, new) {
        let old = chunkify(&old);
        let new = chunkify(&new);
        if old.len() > new.len() {
            warn!(
                "something weird happened with versioning, old: {old}, new: {new}, skipping",
                old = old
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join("."),
                new = new
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join("."),
            );
            return None;
        }
        let bump = new.into_iter().take(old.len()).collect::<Vec<_>>();
        if bump == old {
            None
        } else {
            Some(
                bump.iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join("."),
            )
        }
    } else {
        None
    }
}

#[derive(Debug, Serialize, Clone, Tabled)]
pub struct OutdatedInfo {
    pub name: String,
    #[serde(skip)]
    #[tabled(skip)]
    pub tool_request: ToolRequest,
    #[serde(skip)]
    #[tabled(skip)]
    pub tool_version: ToolVersion,
    pub requested: String,
    #[tabled(display_with("Self::display_current", self))]
    pub current: Option<String>,
    #[tabled(skip)]
    pub bump: Option<String>,
    pub latest: String,
    pub source: ToolSource,
}

impl OutdatedInfo {
    fn new(tv: ToolVersion, source: ToolSource) -> Self {
        Self {
            name: tv.request.backend().name.clone(),
            current: None,
            requested: tv.request.version(),
            tool_request: tv.request.clone(),
            tool_version: tv,
            bump: None,
            latest: "".to_string(),
            source,
        }
    }

    fn display_current(&self) -> String {
        if let Some(current) = &self.current {
            current.clone()
        } else {
            "[MISSING]".to_string()
        }
    }
}

impl Display for OutdatedInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        if let Some(current) = &self.current {
            write!(
                f,
                "{:<20} {:<10} -> {:<10} ({})",
                self.name, current, self.latest, self.source
            )
        } else {
            write!(
                f,
                "{:<20} {:<10} -> {:<10} ({})",
                self.name, "MISSING", self.latest, self.source
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::reset;
    use pretty_assertions::assert_eq;
    use test_log::test;

    use super::{check_semver_bump, is_outdated_version};

    #[test]
    fn test_is_outdated_version() {
        reset();

        assert_eq!(is_outdated_version("1.10.0", "1.12.0"), true);
        assert_eq!(is_outdated_version("1.12.0", "1.10.0"), false);

        assert_eq!(
            is_outdated_version("1.10.0-SNAPSHOT", "1.12.0-SNAPSHOT"),
            true
        );
        assert_eq!(
            is_outdated_version("1.12.0-SNAPSHOT", "1.10.0-SNAPSHOT"),
            false
        );

        assert_eq!(
            is_outdated_version("temurin-17.0.0", "temurin-17.0.1"),
            true
        );
        assert_eq!(
            is_outdated_version("temurin-17.0.1", "temurin-17.0.0"),
            false
        );
    }

    #[test]
    fn test_check_semver_bump() {
        crate::test::reset();
        std::assert_eq!(check_semver_bump("20", "20.0.0"), None);
        std::assert_eq!(check_semver_bump("20.0", "20.0.0"), None);
        std::assert_eq!(check_semver_bump("20.0.0", "20.0.0"), None);
        std::assert_eq!(check_semver_bump("20", "21.0.0"), Some("21".to_string()));
        std::assert_eq!(
            check_semver_bump("20.0", "20.1.0"),
            Some("20.1".to_string())
        );
        std::assert_eq!(
            check_semver_bump("20.0.0", "20.0.1"),
            Some("20.0.1".to_string())
        );
    }
}
