use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::settings::{Settings, SettingsStatusMissingTools};
use crate::env::TERM_WIDTH;
use crate::registry::tool_enabled;
use crate::{backend, parallel};
pub use builder::ToolsetBuilder;
use console::truncate_str;
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use outdated_info::OutdatedInfo;
pub use outdated_info::is_outdated_version;
use tokio::sync::OnceCell;

pub use tool_request::ToolRequest;
pub use tool_request_set::{ToolRequestSet, ToolRequestSetBuilder};
pub use tool_source::ToolSource;
pub use tool_version::{ResolveOptions, ToolVersion};
pub use tool_version_list::ToolVersionList;
pub use tool_version_options::{ToolVersionOptions, parse_tool_options};

use helpers::TVTuple;
pub use install_options::InstallOptions;

mod builder;
mod helpers;
mod install_options;
pub(crate) mod install_state;
pub(crate) mod outdated_info;
pub(crate) mod tool_request;
mod tool_request_set;
mod tool_source;
mod tool_version;
mod tool_version_list;
mod tool_version_options;
mod toolset_env;
mod toolset_install;
mod toolset_paths;

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
        self.list_outdated_versions_filtered(config, bump, opts, None)
            .await
    }

    pub async fn list_outdated_versions_filtered(
        &self,
        config: &Arc<Config>,
        bump: bool,
        opts: &ResolveOptions,
        filter_tools: Option<&[crate::cli::args::ToolArg]>,
    ) -> Vec<OutdatedInfo> {
        let versions = self
            .list_current_versions()
            .into_iter()
            // Filter to only check specified tools if provided
            .filter(|(_, tv)| {
                if let Some(tools) = filter_tools {
                    tools.iter().any(|t| t.ba.as_ref() == tv.ba())
                } else {
                    true
                }
            })
            .map(|(t, tv)| (config.clone(), t, tv, bump, opts.clone()))
            .collect::<Vec<_>>();
        let outdated = parallel::parallel(versions, |(config, t, tv, bump, opts)| async move {
            let mut outdated = vec![];
            match t.outdated_info(&config, &tv, bump, &opts).await {
                Ok(Some(oi)) => outdated.push(oi),
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
                Ok(Some(oi)) => outdated.push(oi),
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

    pub async fn tera_ctx(&self, config: &Arc<Config>) -> Result<&tera::Context> {
        self.tera_ctx
            .get_or_try_init(async || {
                let env = self.full_env(config).await?;
                let mut ctx = config.tera_ctx.clone();
                ctx.insert("env", &env);
                Ok(ctx)
            })
            .await
    }

    pub async fn which(
        &self,
        config: &Arc<Config>,
        bin_name: &str,
    ) -> Option<(Arc<dyn Backend>, ToolVersion)> {
        for (p, tv) in self.list_current_installed_versions(config) {
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
        for (p, tv) in self.list_current_installed_versions(config) {
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
        if Settings::get().status.missing_tools() == SettingsStatusMissingTools::Never {
            return;
        }
        let mut missing = vec![];
        let missing_versions = self.list_missing_versions(config).await;
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
