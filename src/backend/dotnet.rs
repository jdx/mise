use std::sync::Arc;

use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::http::HTTP_FETCH;
use crate::toolset::ToolVersionOptions;
use crate::{backend::Backend, config::Config};
use async_trait::async_trait;
use eyre::eyre;

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

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        let feed_url = self.get_search_url().await?;

        let feed: NugetFeedSearch = HTTP_FETCH
            .json(format!(
                "{}?q={}&packageType=dotnettool&take=1&prerelease={}",
                feed_url,
                &self.tool_name(),
                true
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

    fn include_prereleases(&self, opts: &ToolVersionOptions) -> bool {
        if Settings::get().prereleases {
            return true;
        }

        opts.opts.get("prerelease").is_some_and(tool_option_bool)
            || dotnet_legacy_prerelease_package_flag_enabled()
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
}

fn dotnet_legacy_prerelease_package_flag_enabled() -> bool {
    let enabled = Settings::get()
        .dotnet
        .package_flags
        .iter()
        .any(|flag| flag == "prerelease");
    if enabled {
        deprecated_at!(
            "2026.11.0",
            "2027.11.0",
            "setting.dotnet.package_flags.prerelease",
            "`dotnet.package_flags = [\"prerelease\"]` is deprecated. Use the `prerelease = true` tool option instead."
        );
    }
    enabled
}

fn tool_option_bool(value: &toml::Value) -> bool {
    match value {
        toml::Value::Boolean(b) => *b,
        toml::Value::String(s) => s.parse::<bool>().unwrap_or(false),
        _ => false,
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
