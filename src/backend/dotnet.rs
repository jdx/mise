use std::sync::Arc;

use crate::backend::backend_type::BackendType;
use crate::backend::{
    VersionInfo, filter_cached_prereleases, include_prereleases, mark_prerelease,
};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::http::HTTP_FETCH;
use crate::toolset::ToolVersionOptions;
use crate::{backend::Backend, config::Config};
use async_trait::async_trait;
use eyre::eyre;
use jiff::Timestamp;

/// Dotnet backend requires experimental mode to be enabled
pub const EXPERIMENTAL: bool = true;

#[derive(Debug)]
pub struct DotnetBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for DotnetBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Dotnet
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["dotnet"])
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        // TODO: Once dotnet remote listing always fetches the prerelease
        // superset and filters at read time, remove this override entirely.
        // Today `prerelease` changes the NuGet query, but there are no dotnet
        // backend registry tools using the versions host.
        &[]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        let feed_url = self.get_search_url().await?;
        let opts = self.tool_opts(config).await?;

        let feed: NugetFeedSearch = HTTP_FETCH
            .json(format!(
                "{}?q={}&packageType=dotnettool&take=1&prerelease={}",
                feed_url,
                &self.tool_name(),
                self.dotnet_prereleases_enabled(&opts)
            ))
            .await?;

        if feed.total_hits == 0 {
            return Err(eyre!("No tool found"));
        }

        let data = feed.data.first().ok_or_else(|| eyre!("No data found"))?;

        // Because the nuget API is a search API we need to check name of the tool we are looking for
        if data.id.to_lowercase() != self.tool_name().to_lowercase() {
            return Err(eyre!("Tool {} not found", &self.tool_name()));
        }

        Ok(data
            .versions
            .iter()
            .map(|x| VersionInfo {
                version: x.version.clone(),
                ..Default::default()
            })
            .collect())
    }

    /// Bypass the shared remote-versions cache because the dotnet package flags
    /// affect which versions NuGet returns. The override is on `_with_refresh`
    /// so install-time latest resolution uses the same dotnet-specific
    /// prerelease filtering as `ls-remote`.
    async fn list_remote_versions_with_info_with_refresh(
        &self,
        config: &Arc<Config>,
        _refresh: bool,
    ) -> eyre::Result<Vec<VersionInfo>> {
        let opts = self.tool_opts(config).await?;
        let want_prereleases = self.dotnet_prereleases_enabled(&opts);
        let versions = self
            ._list_remote_versions(config)
            .await?
            .into_iter()
            .map(mark_prerelease)
            .collect();
        Ok(filter_cached_prereleases(versions, want_prereleases))
    }

    async fn list_versions_matching_with_opts(
        &self,
        config: &Arc<Config>,
        query: &str,
        before_date: Option<Timestamp>,
        refresh: bool,
    ) -> eyre::Result<Vec<String>> {
        let versions = match before_date {
            Some(before) => {
                let versions_with_info = self
                    .list_remote_versions_with_info_with_refresh(config, refresh)
                    .await?;
                VersionInfo::filter_by_date(versions_with_info, before)
                    .into_iter()
                    .map(|v| v.version)
                    .collect()
            }
            None => {
                self.list_remote_versions_with_refresh(config, refresh)
                    .await?
            }
        };
        let opts = self.tool_opts(config).await?;
        let filter = !self.dotnet_prereleases_enabled(&opts);
        Ok(self.fuzzy_match_filter(versions, query, filter))
    }

    async fn install_version_(
        &self,
        ctx: &crate::install_context::InstallContext,
        tv: crate::toolset::ToolVersion,
    ) -> eyre::Result<crate::toolset::ToolVersion> {
        Settings::get().ensure_experimental("dotnet backend")?;

        // Check if dotnet is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "dotnet",
            &["dotnet"],
            "To use dotnet tools with mise, you need to install .NET SDK first:\n\
              mise use dotnet@latest\n\n\
            Or install .NET SDK via https://dotnet.microsoft.com/download",
        )
        .await;

        let mut cli = CmdLineRunner::new("dotnet")
            .arg("tool")
            .arg("install")
            .arg(self.tool_name())
            .arg("--tool-path")
            .arg(tv.install_path().join("bin"));

        if &tv.version != "latest" {
            cli = cli.arg("--version").arg(&tv.version);
        }

        cli.with_pr(ctx.pr.as_ref())
            .envs(self.dependency_env(&ctx.config).await?)
            .execute()?;

        Ok(tv)
    }
}

impl DotnetBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    async fn get_search_url(&self) -> eyre::Result<String> {
        let settings = Settings::get();
        let nuget_registry = settings.dotnet.registry_url.as_str();

        let services: NugetFeed = HTTP_FETCH.json(nuget_registry).await?;

        let feed = services
            .resources
            .iter()
            .find(|x| x.service_type == "SearchQueryService/3.5.0")
            .or_else(|| {
                services
                    .resources
                    .iter()
                    .find(|x| x.service_type == "SearchQueryService")
            })
            .ok_or_else(|| eyre!("No SearchQueryService found"))?;

        Ok(feed.id.clone())
    }

    async fn tool_opts(&self, config: &Arc<Config>) -> eyre::Result<ToolVersionOptions> {
        config.get_tool_opts_with_overrides(&self.ba).await
    }

    fn dotnet_prereleases_enabled(&self, opts: &ToolVersionOptions) -> bool {
        Settings::get().prereleases
            || Settings::get()
                .dotnet
                .package_flags
                .contains(&"prerelease".to_string())
            || include_prereleases(opts)
    }
}

#[derive(serde::Deserialize)]
struct NugetFeed {
    resources: Vec<NugetFeedResource>,
}

#[derive(serde::Deserialize)]
struct NugetFeedResource {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@type")]
    service_type: String,
}

#[derive(serde::Deserialize)]
struct NugetFeedSearch {
    #[serde(rename = "totalHits")]
    total_hits: i32,
    data: Vec<NugetFeedSearchData>,
}

#[derive(serde::Deserialize)]
struct NugetFeedSearchData {
    id: String,
    versions: Vec<NugetFeedSearchDataVersion>,
}

#[derive(serde::Deserialize)]
struct NugetFeedSearchDataVersion {
    version: String,
}
