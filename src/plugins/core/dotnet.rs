use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use eyre::Result;
use serde_derive::Deserialize;
use versions::Versioning;

use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::parallel;
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

    fn is_isolated() -> bool {
        Settings::get().dotnet.isolated
    }

    async fn test_dotnet(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("dotnet --version".into());
        CmdLineRunner::new(DOTNET_BIN)
            .with_pr(ctx.pr.as_ref())
            .arg("--version")
            .envs(self.exec_env(&ctx.config, &ctx.ts, tv).await?)
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

        // Fetch all channel release data in parallel
        let urls: Vec<String> = index
            .releases_index
            .iter()
            .filter_map(|ch| ch.releases_json.as_ref())
            .filter(|url| !url.is_empty())
            .cloned()
            .collect();

        let channels: Vec<ChannelReleases> =
            parallel::parallel(urls, |url| async move { HTTP_FETCH.json(&url).await }).await?;

        let mut versions = std::collections::BTreeSet::new();
        for channel_data in &channels {
            for release in &channel_data.releases {
                let sdk_iter = release.sdk.iter();
                let sdks_iter = release.sdks.iter().flatten();
                for sdk in sdk_iter.chain(sdks_iter) {
                    if let Some(ref version) = sdk.version {
                        versions.insert(SortedVersion(version.clone()));
                    }
                }
            }
        }

        Ok(versions
            .into_iter()
            .map(|v| VersionInfo {
                version: v.0,
                ..Default::default()
            })
            .collect())
    }

    async fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec!["global.json".into()])
    }

    async fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
        let content = file::read_to_string(path)?;
        let global_json: GlobalJson = serde_json::from_str(&content)?;
        let sdk = global_json
            .sdk
            .ok_or_else(|| eyre::eyre!("no sdk.version found in {}", path.display()))?;
        Ok(sdk.version)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let isolated = Self::is_isolated();
        let install_dir = if isolated {
            tv.install_path()
        } else {
            dotnet_root()
        };
        file::create_dir_all(&install_dir)?;

        // Download install script (always refresh to pick up upstream fixes)
        let script_path = install_script_path();
        file::create_dir_all(script_path.parent().unwrap())?;
        ctx.pr
            .set_message("Downloading dotnet-install script".into());
        HTTP.download_file(install_script_url(), &script_path, Some(ctx.pr.as_ref()))
            .await?;
        #[cfg(unix)]
        file::make_executable(&script_path)?;

        // Run install script
        ctx.pr
            .set_message(format!("Installing .NET SDK {}", tv.version));
        install_cmd(&script_path, &install_dir, &tv.version)
            .with_pr(ctx.pr.as_ref())
            .envs(self.exec_env(&ctx.config, &ctx.ts, &tv).await?)
            .execute()?;

        if !isolated {
            // Symlink install_path -> DOTNET_ROOT so mise can track the installation
            file::remove_all(tv.install_path())?;
            file::make_symlink(&install_dir, &tv.install_path())?;
        }

        self.test_dotnet(ctx, &tv).await?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        _config: &Arc<Config>,
        _pr: &dyn SingleReport,
        tv: &ToolVersion,
    ) -> Result<()> {
        if Self::is_isolated() {
            // Isolated: mise handles removal of install_path by default
        } else {
            // Shared: only remove this SDK version from the shared root
            let sdk_dir = dotnet_root().join("sdk").join(&tv.version);
            if sdk_dir.exists() {
                file::remove_all(&sdk_dir)?;
            }
        }
        Ok(())
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        if Self::is_isolated() {
            Ok(vec![tv.install_path()])
        } else {
            Ok(vec![dotnet_root()])
        }
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<BTreeMap<String, String>> {
        let root = if Self::is_isolated() {
            tv.install_path()
        } else {
            dotnet_root()
        };
        let mut env = BTreeMap::from([
            (
                "DOTNET_ROOT".to_string(),
                root.to_string_lossy().to_string(),
            ),
            ("DOTNET_MULTILEVEL_LOOKUP".to_string(), "0".to_string()),
        ]);
        if let Some(optout) = Settings::get().dotnet.cli_telemetry_optout {
            env.insert(
                "DOTNET_CLI_TELEMETRY_OPTOUT".to_string(),
                if optout { "1" } else { "0" }.to_string(),
            );
        }
        Ok(env)
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
    sdk: Option<GlobalJsonSdk>,
}

#[derive(Deserialize)]
struct GlobalJsonSdk {
    version: String,
}

// --- semver-sorted wrapper for BTreeSet dedup + ordering ---

#[derive(Eq, PartialEq)]
struct SortedVersion(String);

impl Ord for SortedVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a = Versioning::new(&self.0);
        let b = Versioning::new(&other.0);
        a.cmp(&b).then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for SortedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
