use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use eyre::Result;
use serde_derive::Deserialize;

use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, plugins};

#[derive(Debug)]
pub struct DotnetPlugin {
    ba: Arc<BackendArg>,
}

impl DotnetPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("dotnet").into(),
        }
    }

    async fn test_dotnet(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("dotnet --version".into());
        let ts = ctx.config.get_toolset().await?;
        CmdLineRunner::new(DOTNET_BIN)
            .with_pr(ctx.pr.as_ref())
            .arg("--version")
            .envs(self.exec_env(&ctx.config, ts, tv).await?)
            .prepend_path(self.list_bin_paths(&ctx.config, tv).await?)?
            .execute()
    }
}

#[async_trait]
impl Backend for DotnetPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn supports_lockfile_url(&self) -> bool {
        false
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let index: ReleasesIndex = HTTP_FETCH
            .json("https://builds.dotnet.microsoft.com/dotnet/release-metadata/releases-index.json")
            .await?;

        let mut versions = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for channel in &index.releases_index {
            let releases_url = match &channel.releases_json {
                Some(url) if !url.is_empty() => url,
                _ => continue,
            };

            let channel_data: ChannelReleases = match HTTP_FETCH.json(releases_url).await {
                Ok(data) => data,
                Err(_) => continue,
            };

            for release in &channel_data.releases {
                // Primary SDK version
                if let Some(ref sdk) = release.sdk
                    && let Some(ref version) = sdk.version
                        && seen.insert(version.clone()) {
                            versions.push(VersionInfo {
                                version: version.clone(),
                                ..Default::default()
                            });
                        }

                // Additional SDKs
                if let Some(ref sdks) = release.sdks {
                    for sdk in sdks {
                        if let Some(ref version) = sdk.version
                            && seen.insert(version.clone()) {
                                versions.push(VersionInfo {
                                    version: version.clone(),
                                    ..Default::default()
                                });
                            }
                    }
                }
            }
        }

        Ok(versions)
    }

    async fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec!["global.json".into()])
    }

    async fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
        let content = file::read_to_string(path)?;
        let global_json: GlobalJson = serde_json::from_str(&content)?;
        Ok(global_json.sdk.version)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let root = dotnet_root();
        file::create_dir_all(&root)?;

        // Download install script to cache
        let script_path = install_script_path();
        if !script_path.exists() {
            file::create_dir_all(script_path.parent().unwrap())?;
            ctx.pr
                .set_message("Downloading dotnet-install script".into());
            HTTP.download_file(install_script_url(), &script_path, Some(ctx.pr.as_ref()))
                .await?;
            #[cfg(unix)]
            file::make_executable(&script_path)?;
        }

        // Run install script
        ctx.pr
            .set_message(format!("Installing .NET SDK {}", tv.version));
        let ts = ctx.config.get_toolset().await?;
        install_cmd(&script_path, &root, &tv.version)
            .with_pr(ctx.pr.as_ref())
            .envs(self.exec_env(&ctx.config, ts, &tv).await?)
            .execute()?;

        // Symlink install_path -> DOTNET_ROOT so mise can track the installation
        file::remove_all(tv.install_path())?;
        file::make_symlink(&root, &tv.install_path())?;

        self.test_dotnet(ctx, &tv).await?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        _config: &Arc<Config>,
        _pr: &dyn SingleReport,
        tv: &ToolVersion,
    ) -> Result<()> {
        let sdk_dir = dotnet_root().join("sdk").join(&tv.version);
        if sdk_dir.exists() {
            file::remove_all(&sdk_dir)?;
        }
        Ok(())
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        _tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        Ok(vec![dotnet_root()])
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        _tv: &ToolVersion,
    ) -> Result<BTreeMap<String, String>> {
        let root = dotnet_root();
        Ok([
            (
                "DOTNET_ROOT".to_string(),
                root.to_string_lossy().to_string(),
            ),
            ("DOTNET_CLI_TELEMETRY_OPTOUT".to_string(), "1".to_string()),
            ("DOTNET_MULTILEVEL_LOOKUP".to_string(), "0".to_string()),
        ]
        .into())
    }
}

fn dotnet_root() -> PathBuf {
    Settings::get()
        .dotnet
        .dotnet_root
        .clone()
        .or(env::var_path("DOTNET_ROOT"))
        .unwrap_or(dirs::DATA.join("dotnet-root"))
}

fn install_script_path() -> PathBuf {
    dirs::CACHE.join("dotnet").join(INSTALL_SCRIPT_NAME)
}

#[cfg(unix)]
const DOTNET_BIN: &str = "dotnet";

#[cfg(windows)]
const DOTNET_BIN: &str = "dotnet.exe";

#[cfg(unix)]
const INSTALL_SCRIPT_NAME: &str = "dotnet-install.sh";

#[cfg(windows)]
const INSTALL_SCRIPT_NAME: &str = "dotnet-install.ps1";

#[cfg(unix)]
fn install_script_url() -> &'static str {
    "https://dot.net/v1/dotnet-install.sh"
}

#[cfg(windows)]
fn install_script_url() -> &'static str {
    "https://dot.net/v1/dotnet-install.ps1"
}

#[cfg(unix)]
fn install_cmd<'a>(script_path: &Path, install_dir: &Path, version: &str) -> CmdLineRunner<'a> {
    CmdLineRunner::new(script_path)
        .arg("--install-dir")
        .arg(install_dir)
        .arg("--version")
        .arg(version)
        .arg("--no-path")
}

#[cfg(windows)]
fn install_cmd<'a>(script_path: &Path, install_dir: &Path, version: &str) -> CmdLineRunner<'a> {
    CmdLineRunner::new("powershell")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(script_path)
        .arg("-InstallDir")
        .arg(install_dir)
        .arg("-Version")
        .arg(version)
        .arg("-NoPath")
}

// --- Microsoft releases API types ---

#[derive(Deserialize)]
struct ReleasesIndex {
    #[serde(rename = "releases-index")]
    releases_index: Vec<ChannelEntry>,
}

#[derive(Deserialize)]
struct ChannelEntry {
    #[serde(rename = "releases.json")]
    releases_json: Option<String>,
}

#[derive(Deserialize)]
struct ChannelReleases {
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    sdk: Option<Sdk>,
    sdks: Option<Vec<Sdk>>,
}

#[derive(Deserialize)]
struct Sdk {
    version: Option<String>,
}

// --- global.json ---

#[derive(Deserialize)]
struct GlobalJson {
    sdk: GlobalJsonSdk,
}

#[derive(Deserialize)]
struct GlobalJsonSdk {
    version: String,
}
