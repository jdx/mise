use crate::backend::backend_type::BackendType;
use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::http::HTTP_FETCH;
use eyre::eyre;

#[derive(Debug)]
pub struct DotnetBackend {
    ba: BackendArg,
}

impl Backend for DotnetBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Dotnet
    }

    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["dotnet"])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        let feed_url = self.get_search_url()?;

        let feed: NugetFeedSearch = HTTP_FETCH.json(format!(
            "{}?q={}&packageType=DotnetCliTool&take=1",
            feed_url,
            &self.tool_name()
        ))?;

        if feed.total_hits == 0 {
            return Err(eyre!("No tool found"));
        }

        let data = feed.data.first().ok_or_else(|| eyre!("No data found"))?;

        Ok(data.versions.iter().map(|x| x.version.clone()).collect())
    }

    fn install_version_(
        &self,
        ctx: &crate::install_context::InstallContext,
        tv: crate::toolset::ToolVersion,
    ) -> eyre::Result<crate::toolset::ToolVersion> {
        todo!()
    }
}

impl DotnetBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba }
    }

    fn get_search_url(&self) -> eyre::Result<String> {
        let nuget_registry = SETTINGS.dotnet.registry_url.as_ref().ok_or_else(|| {
            eyre!("No registry URL found in settings. Please set it in your config file.")
        })?;
        let services: NugetFeed = HTTP_FETCH.json(nuget_registry)?;
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
    version: String,
    resources: Vec<NugetFeedResource>,
}

#[derive(serde::Deserialize)]
struct NugetFeedResource {
    id: String,
    service_type: String,
}

#[derive(serde::Deserialize)]
struct NugetFeedSearch {
    total_hits: i32,
    data: Vec<NugetFeedSearchData>,
}

#[derive(serde::Deserialize)]
struct NugetFeedSearchData {
    id: String,
    version: String,
    versions: Vec<NugetFeedSearchDataVersion>,
}

#[derive(serde::Deserialize)]
struct NugetFeedSearchDataVersion {
    version: String,
}
