use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::settings::{Settings, SettingsStatusMissingTools};
use crate::env::TERM_WIDTH;
use crate::registry::REGISTRY;
use crate::registry::tool_enabled;
use crate::{backend, parallel};
pub use builder::ToolsetBuilder;
use console::truncate_str;
use eyre::{Result, bail};
use helpers::TVTuple;
use indexmap::IndexMap;
use itertools::Itertools;
use outdated_info::OutdatedInfo;
pub use outdated_info::is_outdated_version;
use petgraph::Direction;
use petgraph::graphmap::DiGraphMap;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
};
use tokio::sync::OnceCell;

pub use install_options::InstallOptions;
pub use tool_request::ToolRequest;
pub use tool_request_set::{ToolRequestSet, ToolRequestSetBuilder};
pub use tool_source::ToolSource;
pub use tool_version::{ResolveOptions, ToolVersion};
pub use tool_version_list::ToolVersionList;
pub use tool_version_options::{ToolVersionOptions, parse_tool_options};

mod builder;
pub mod env_cache;
mod helpers;
mod install_options;
pub(crate) mod install_state;
pub(crate) mod outdated_info;
mod tool_deps;
pub(crate) mod tool_request;
mod tool_request_set;
mod tool_source;
mod tool_version;
mod tool_version_list;
mod tool_version_options;
mod toolset_env;
mod toolset_install;
mod toolset_paths;

#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ToolInfos {
    Single(ToolInfo),
    Multiple(Vec<ToolInfo>),
}

/// a toolset is a collection of tools for various plugins
///
/// one example is a .tool-versions file
/// the idea is that we start with an empty toolset, then
/// merge in other toolsets from various sources
#[derive(Debug, Default, Clone)]
pub struct Toolset {
    pub versions: IndexMap<Arc<BackendArg>, ToolVersionList>,
    pub source: Option<ToolSource>,
    tera_ctx: OnceCell<tera::Context>,
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

    #[async_backtrace::framed]
    pub async fn resolve(&mut self, config: &Arc<Config>) -> eyre::Result<()> {
        self.resolve_with_opts(config, &Default::default()).await
    }

    #[async_backtrace::framed]
    pub async fn resolve_with_opts(
        &mut self,
        config: &Arc<Config>,
        opts: &ResolveOptions,
    ) -> eyre::Result<()> {
        self.list_missing_plugins();
        let versions = self
            .versions
            .clone()
            .into_iter()
            .map(|(ba, tvl)| (config.clone(), ba, tvl.clone(), opts.clone()))
            .collect::<Vec<_>>();
        let tvls = parallel::parallel(versions, |(config, ba, mut tvl, opts)| async move {
            if let Err(err) = tvl.resolve(&config, &opts).await {
                warn!("Failed to resolve tool version list for {ba}: {err}");
            }
            Ok((ba, tvl))
        })
        .await?;
        self.versions = tvls.into_iter().collect();
        Ok(())
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

    pub async fn list_missing_versions(&self, config: &Arc<Config>) -> Vec<ToolVersion> {
        trace!("list_missing_versions");
        measure!("toolset::list_missing_versions", {
            self.list_current_versions()
                .into_iter()
                .filter(|(p, tv)| !p.is_version_installed(config, tv, true))
                .map(|(_, tv)| tv)
                .collect()
        })
    }

    pub async fn list_installed_versions(&self, config: &Arc<Config>) -> Result<Vec<TVTuple>> {
        let current_versions: HashMap<(String, String), TVTuple> = self
            .list_current_versions()
            .into_iter()
            .map(|(p, tv)| ((p.id().into(), tv.version.clone()), (p.clone(), tv)))
            .collect();
        let current_versions = Arc::new(current_versions);
        let mut versions = vec![];
        for b in backend::list().into_iter() {
            for v in b.list_installed_versions() {
                if let Some((p, tv)) = current_versions.get(&(b.id().into(), v.clone())) {
                    versions.push((p.clone(), tv.clone()));
                } else {
                    let tv = ToolRequest::new(b.ba().clone(), &v, ToolSource::Unknown)?
                        .resolve(config, &Default::default())
                        .await?;
                    versions.push((b.clone(), tv));
                }
            }
        }
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
        trace!("list_current_versions");
        self.list_versions_by_plugin()
            .iter()
            .flat_map(|(p, v)| {
                v.iter().filter(|v| v.request.is_os_supported()).map(|v| {
                    // map cargo backend specific prefixes to ref
                    let tv = match v.version.split_once(':') {
                        Some((ref_type @ ("tag" | "branch" | "rev"), r)) => {
                            let request = ToolRequest::Ref {
                                backend: p.ba().clone(),
                                ref_: r.to_string(),
                                ref_type: ref_type.to_string(),
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

    pub async fn list_all_versions(
        &self,
        config: &Arc<Config>,
    ) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
        use itertools::Itertools;
        let versions = self
            .list_current_versions()
            .into_iter()
            .chain(self.list_installed_versions(config).await?)
            .unique_by(|(ba, tv)| (ba.clone(), tv.tv_pathname().to_string()))
            .collect();
        Ok(versions)
    }

    pub fn list_current_installed_versions(
        &self,
        config: &Arc<Config>,
    ) -> Vec<(Arc<dyn Backend>, ToolVersion)> {
        self.list_current_versions()
            .into_iter()
            .filter(|(p, tv)| p.is_version_installed(config, tv, true))
            .collect()
    }

    pub async fn list_outdated_versions(
        &self,
        config: &Arc<Config>,
        bump: bool,
        opts: &ResolveOptions,
    ) -> Vec<OutdatedInfo> {
        self.list_outdated_versions_filtered(config, bump, opts, None, None)
            .await
    }

    pub async fn list_outdated_versions_filtered(
        &self,
        config: &Arc<Config>,
        bump: bool,
        opts: &ResolveOptions,
        filter_tools: Option<&[crate::cli::args::ToolArg]>,
        exclude_tools: Option<&[crate::cli::args::ToolArg]>,
    ) -> Vec<OutdatedInfo> {
        let versions = self
            .list_current_versions()
            .into_iter()
            // Filter to only check specified tools if provided
            .filter(|(_, tv)| {
                // Exclude tools if specified
                if let Some(exclude) = exclude_tools
                    && exclude.iter().any(|t| t.ba.as_ref() == tv.ba())
                {
                    return false;
                }
                // Include only specified tools if provided
                if let Some(tools) = filter_tools {
                    tools.iter().any(|t| t.ba.as_ref() == tv.ba())
                } else {
                    true
                }
            })
            .map(|(t, tv)| (config.clone(), t, tv, bump, opts.clone()))
            .collect::<Vec<_>>();
        let outdated = parallel::parallel(versions, |(config, t, tv, bump, opts)| async move {
            let mut outdated = HashSet::new();
            match t.outdated_info(&config, &tv, bump, &opts).await {
                Ok(Some(oi)) => {
                    outdated.insert(oi);
                }
                Ok(None) => {}
                Err(e) => {
                    warn!("Error getting outdated info for {tv}: {e:#}");
                }
            }
            if t.symlink_path(&tv).is_some() {
                trace!("skipping symlinked version {tv}");
                // do not consider symlinked versions to be outdated
                return Ok(outdated);
            }
            match OutdatedInfo::resolve(&config, tv.clone(), bump, &opts).await {
                Ok(Some(oi)) => {
                    outdated.insert(oi);
                }
                Ok(None) => {}
                Err(e) => {
                    warn!("Error creating OutdatedInfo for {tv}: {e:#}");
                }
            }
            Ok(outdated)
        })
        .await
        .unwrap_or_else(|e| {
            warn!("Error in parallel outdated version check: {e:#}");
            vec![]
        });
        outdated.into_iter().flatten().collect()
    }

    pub fn build_tools_tera_map(&self, config: &Arc<Config>) -> HashMap<String, ToolInfos> {
        let mut tools_map: HashMap<String, Vec<ToolInfo>> = HashMap::new();
        for (_, tv) in self.list_current_installed_versions(config) {
            let tool_name = tv.ba().tool_name.clone();
            let short = tv.ba().short.clone();
            let info = ToolInfo {
                version: tv.version.clone(),
                path: tv.install_path().to_string_lossy().to_string(),
            };
            tools_map
                .entry(tool_name.clone())
                .or_default()
                .push(info.clone());
            if short != tool_name {
                tools_map.entry(short).or_default().push(info);
            }
        }
        tools_map
            .into_iter()
            .map(|(k, v)| {
                let infos = if v.len() == 1 {
                    ToolInfos::Single(v.into_iter().next().unwrap())
                } else {
                    ToolInfos::Multiple(v)
                };
                (k, infos)
            })
            .collect()
    }

    pub async fn tera_ctx(&self, config: &Arc<Config>) -> Result<&tera::Context> {
        self.tera_ctx
            .get_or_try_init(async || {
                let env = self.full_env(config).await?;
                let mut ctx = config.tera_ctx.clone();
                ctx.insert("env", &env);
                ctx.insert("tools", &self.build_tools_tera_map(config));
                Ok(ctx)
            })
            .await
    }

    /// Sort installed tools so that tools with `overrides` in the registry
    /// appear before the tools they override. e.g., npm overrides node so that
    /// the explicitly-installed npm binary is found before node's bundled npm.
    pub(crate) fn sort_by_overrides(
        installed: &mut Vec<(Arc<dyn Backend>, ToolVersion)>,
    ) -> Result<()> {
        let mut graph = DiGraphMap::<&str, ()>::new();

        // Collect unique IDs to build the graph (deduplicates multi-version tools)
        let unique_ids: HashSet<String> =
            installed.iter().map(|(b, _)| b.id().to_string()).collect();
        let unique_ids: Vec<String> = unique_ids.into_iter().collect();

        let mut original_index: HashMap<&str, usize> = HashMap::new();
        for (i, (b, _)) in installed.iter().enumerate() {
            let id = b.id();
            original_index.entry(id).or_insert(i);
        }

        for id in &unique_ids {
            graph.add_node(id.as_str());
        }

        for id in &unique_ids {
            let id_str = id.as_str();
            if let Some(tool) = REGISTRY.get(id_str) {
                for overridden in tool.overrides {
                    // Edge: id -> overridden (overrider -> overridden)
                    // Only add edge if overridden tool is also in the list
                    if graph.contains_node(overridden) {
                        graph.add_edge(id_str, overridden, ());
                    }
                }
            }
        }

        if graph.edge_count() == 0 {
            return Ok(());
        }

        // Priority = min(priority, priority_of_dependencies)
        let mut priorities: HashMap<&str, usize> = original_index.clone();
        let mut changed = true;
        while changed {
            changed = false;
            for (overrider, overridden, _) in graph.all_edges() {
                let p_overridden = *priorities.get(overridden).unwrap_or(&usize::MAX);
                let p_overrider = *priorities.get(overrider).unwrap_or(&usize::MAX);
                if p_overridden < p_overrider {
                    priorities.insert(overrider, p_overridden);
                    changed = true;
                }
            }
        }

        // Topological Sort with Priority Queue
        let mut in_degree: HashMap<&str, usize> = graph
            .nodes()
            .map(|node| {
                (
                    node,
                    graph.neighbors_directed(node, Direction::Incoming).count(),
                )
            })
            .collect();

        let mut pq = BinaryHeap::new();
        for (&node, &deg) in &in_degree {
            if deg == 0 {
                let p = priorities[node];
                let idx = original_index[node];
                pq.push(Reverse((p, idx, node)));
            }
        }

        let mut sorted_ids: Vec<&str> = Vec::with_capacity(graph.node_count());
        while let Some(Reverse((_, _, id))) = pq.pop() {
            sorted_ids.push(id);

            for neighbor in graph.neighbors(id) {
                if let Some(deg) = in_degree.get_mut(neighbor) {
                    *deg -= 1;
                    if *deg == 0 {
                        let p = priorities[neighbor];
                        let idx = original_index[neighbor];
                        pq.push(Reverse((p, idx, neighbor)));
                    }
                }
            }
        }

        if sorted_ids.len() != graph.node_count() {
            bail!("Cycle detected in tool overrides");
        }

        let order: HashMap<&str, usize> = sorted_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();
        installed.sort_by_cached_key(|(b, _)| order.get(b.id()).copied().unwrap_or(usize::MAX));

        Ok(())
    }

    pub async fn which(
        &self,
        config: &Arc<Config>,
        bin_name: &str,
    ) -> Option<(Arc<dyn Backend>, ToolVersion)> {
        let mut installed = self.list_current_installed_versions(config);
        Self::sort_by_overrides(&mut installed).unwrap();
        for (p, tv) in installed {
            match Box::pin(p.which(config, &tv, bin_name)).await {
                Ok(Some(_bin)) => return Some((p, tv)),
                Ok(None) => {}
                Err(e) => {
                    debug!("Error running which: {:#}", e);
                }
            }
        }
        None
    }

    pub async fn which_bin(&self, config: &Arc<Config>, bin_name: &str) -> Option<PathBuf> {
        let mut installed = self.list_current_installed_versions(config);
        Self::sort_by_overrides(&mut installed).unwrap();
        for (p, tv) in installed {
            if let Ok(Some(bin)) = Box::pin(p.which(config, &tv, bin_name)).await {
                return Some(bin);
            }
        }
        None
    }

    pub async fn list_rtvs_with_bin(
        &self,
        config: &Arc<Config>,
        bin_name: &str,
    ) -> Result<Vec<ToolVersion>> {
        let mut rtvs = vec![];
        for (p, tv) in self.list_installed_versions(config).await? {
            match p.which(config, &tv, bin_name).await {
                Ok(Some(_bin)) => rtvs.push(tv),
                Ok(None) => {}
                Err(e) => {
                    warn!("Error running which: {:#}", e);
                }
            }
        }
        Ok(rtvs)
    }

    pub async fn notify_if_versions_missing(&self, config: &Arc<Config>) {
        let missing_versions = self.list_missing_versions(config).await;
        self.notify_missing_versions(missing_versions);
    }

    pub fn notify_missing_versions(&self, missing_versions: Vec<ToolVersion>) {
        if Settings::get().status.missing_tools() == SettingsStatusMissingTools::Never {
            return;
        }
        let mut missing = vec![];
        for tv in missing_versions.into_iter() {
            if Settings::get().status.missing_tools() == SettingsStatusMissingTools::Always {
                missing.push(tv);
                continue;
            }
            if let Ok(backend) = tv.backend() {
                let installed = backend.list_installed_versions();
                if !installed.is_empty() {
                    missing.push(tv);
                }
            }
        }
        if missing.is_empty() || *crate::env::__MISE_SHIM {
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
        !ba.is_os_supported()
            || !tool_enabled(
                &Settings::get().enable_tools(),
                &Settings::get().disable_tools(),
                &ba.short.to_string(),
            )
    }
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

/// Get all tool versions that are needed by tracked config files.
/// Returns a set of (short_name, tv_pathname) pairs.
/// This is used by both `mise prune` and `mise upgrade` to avoid
/// uninstalling versions that other projects still need.
pub async fn get_versions_needed_by_tracked_configs(
    config: &Arc<Config>,
) -> Result<std::collections::HashSet<(String, String)>> {
    let mut needed = std::collections::HashSet::new();
    // Use use_locked_version: false to resolve based on what config files actually
    // request, not what was previously locked. This is important during upgrade
    // because the lockfile hasn't been updated yet when this is called.
    let opts = ResolveOptions {
        use_locked_version: false,
        ..Default::default()
    };
    for cf in config.get_tracked_config_files().await?.values() {
        let mut ts = Toolset::from(cf.to_tool_request_set()?);
        ts.resolve_with_opts(config, &opts).await?;
        for (_, tv) in ts.list_current_versions() {
            needed.insert((tv.ba().short.to_string(), tv.tv_pathname()));
        }
    }
    Ok(needed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::arg_to_backend;
    use crate::cli::args::BackendArg;
    use crate::toolset::{ToolRequest, ToolSource, ToolVersion};

    #[tokio::test]
    async fn test_sort_by_overrides() {
        crate::toolset::install_state::init().await.unwrap();
        let node = arg_to_backend(BackendArg::from("node")).unwrap();
        let npm = arg_to_backend(BackendArg::from("npm")).unwrap();
        let jc = arg_to_backend(BackendArg::from("jc")).unwrap();
        let jq = arg_to_backend(BackendArg::from("jq")).unwrap();

        let mk_tv = |backend: Arc<dyn Backend>, version: &str| {
            let ba = backend.ba().clone();
            let req = ToolRequest::System {
                backend: ba,
                source: ToolSource::Argument,
                options: Default::default(),
            };
            ToolVersion::new(req, version.into())
        };

        let tv_node = mk_tv(node.clone(), "20.0.0");
        let tv_npm = mk_tv(npm.clone(), "10.2.5");
        let tv_jc = mk_tv(jc.clone(), "1.0.0");
        let tv_jq = mk_tv(jq.clone(), "1.0.0");

        let mut input = vec![
            (node.clone(), tv_node.clone()),
            (jc.clone(), tv_jc.clone()),
            (jq.clone(), tv_jq.clone()),
            (npm.clone(), tv_npm.clone()),
        ];
        Toolset::sort_by_overrides(&mut input).unwrap();
        let ids: Vec<&str> = input.iter().map(|(b, _)| b.id()).collect();
        assert_eq!(ids, vec!["npm", "node", "jc", "jq"]);

        let mut input = vec![
            (node.clone(), tv_node.clone()),
            (jq.clone(), tv_jq.clone()),
            (npm.clone(), tv_npm.clone()),
            (jc.clone(), tv_jc.clone()),
        ];
        Toolset::sort_by_overrides(&mut input).unwrap();
        let ids: Vec<&str> = input.iter().map(|(b, _)| b.id()).collect();
        assert_eq!(ids, vec!["npm", "node", "jq", "jc"]);

        let mut input = vec![
            (jc.clone(), tv_jc.clone()),
            (npm.clone(), tv_npm.clone()),
            (jq.clone(), tv_jq.clone()),
            (node.clone(), tv_node.clone()),
        ];
        Toolset::sort_by_overrides(&mut input).unwrap();
        let ids: Vec<&str> = input.iter().map(|(b, _)| b.id()).collect();
        assert_eq!(ids, vec!["jc", "npm", "jq", "node"]);

        // Test with multiple versions of the same tool
        let tv_node_18 = mk_tv(node.clone(), "18.0.0");
        let tv_node_20 = mk_tv(node.clone(), "20.0.0");
        let tv_npm_9 = mk_tv(npm.clone(), "9.0.0");

        let mut input = vec![
            (node.clone(), tv_node_20.clone()),
            (node.clone(), tv_node_18.clone()),
            (jc.clone(), tv_jc.clone()),
            (npm.clone(), tv_npm_9.clone()),
            (npm.clone(), tv_npm.clone()),
        ];
        Toolset::sort_by_overrides(&mut input).unwrap();

        // npm should come before node (due to override)
        // Multiple versions of same tool should maintain original order
        let result: Vec<(&str, &str)> = input
            .iter()
            .map(|(b, tv)| (b.id(), tv.version.as_str()))
            .collect();
        assert_eq!(
            result,
            vec![
                ("npm", "9.0.0"),
                ("npm", "10.2.5"),
                ("node", "20.0.0"),
                ("node", "18.0.0"),
                ("jc", "1.0.0"),
            ]
        );
    }
}
