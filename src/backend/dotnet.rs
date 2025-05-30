use std::sync::Arc;

use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::http::HTTP_FETCH;
use crate::{backend::Backend, config::Config};
use async_trait::async_trait;
use eyre::eyre;

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

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        let feed_url = self.get_search_url().await?;

        let feed: NugetFeedSearch = HTTP_FETCH
            .json(format!(
                "{}?q={}&packageType=dotnettool&take=1&prerelease={}",
                feed_url,
                &self.tool_name(),
                Settings::get()
                    .dotnet
                    .package_flags
                    .contains(&"prerelease".to_string())
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

        Ok(data.versions.iter().map(|x| x.version.clone()).collect())
    }

    async fn install_version_(
        &self,
        ctx: &crate::install_context::InstallContext,
        tv: crate::toolset::ToolVersion,
    ) -> eyre::Result<crate::toolset::ToolVersion> {
        Settings::get().ensure_experimental("dotnet backend")?;

        let mut cli = CmdLineRunner::new("dotnet")
            .arg("tool")
            .arg("install")
            .arg(self.tool_name())
            .arg("--tool-path")
            .arg(tv.install_path().join("bin"));

        if &tv.version != "latest" {
            cli = cli.arg("--version").arg(&tv.version);
        }

        cli.with_pr(&ctx.pr)
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
            .ok_or_else(|| eyre!("No SearchQueryService/3.5.0 found"))?;

        Ok(feed.id.clone())
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
