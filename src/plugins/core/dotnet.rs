use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock as Lazy};

use async_trait::async_trait;
use eyre::Result;
use serde::{Deserialize, Serialize};
use versions::Versioning;

use crate::args::BackendArg;
use crate::backend::Backend;
use crate::backend::options::BackendOptions;
use crate::backend::{VersionInfo, platform_target::PlatformTarget};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cmd::CmdLineRunner;
use crate::config::settings::SettingsExt;
use crate::config::{Config, Settings};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::parallel;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, plugins};

#[derive(Debug)]
pub(super) struct DotnetPlugin {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct DotnetOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> DotnetOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn runtime(&self) -> Option<&'a str> {
        self.values.str("runtime")
    }

    fn runtime_framework_name(&self) -> Option<&'static str> {
        self.runtime().and_then(runtime_framework_name)
    }

    fn lockfile_options(&self) -> BTreeMap<String, String> {
        let mut opts = BTreeMap::new();
        if let Some(runtime) = self.runtime() {
            opts.insert("runtime".to_string(), runtime.to_string());
        }
        opts
    }
}

impl DotnetPlugin {
    pub(super) fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("dotnet").into(),
        }
    }

    fn is_isolated() -> bool {
        Settings::get().dotnet.isolated
    }

    async fn test_dotnet(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        let raw_opts = tv.request.options();
        let opts = DotnetOptions::new(&raw_opts);
        if opts.runtime().is_some() {
            // Skip SDK check for runtime-only installs; no SDK is expected.
            return Ok(());
        }
        ctx.pr.set_message("dotnet --list-sdks".into());
        // Run the binary by absolute path: with activate_aggressive=false an already-active SDK
        // ahead of the shims on PATH would otherwise shadow the one being validated.
        let bin_paths = self.list_bin_paths(&ctx.config, tv).await?;
        let dotnet = bin_paths
            .first()
            .map_or_else(|| PathBuf::from(DOTNET_BIN), |p| p.join(DOTNET_BIN));
        let sdks = CmdLineRunner::new(dotnet)
            .with_pr(ctx.pr.as_ref())
            .arg("--list-sdks")
            .env_values(tv.install_env())
            .envs(self.exec_env(&ctx.config, &ctx.ts, tv).await?)
            .prepend_path(bin_paths)?
            .read()
            .await?
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();

        if sdk_list_contains_version(&sdks, &tv.version) {
            Ok(())
        } else {
            eyre::bail!(
                "dotnet SDK {} was not found in `dotnet --list-sdks` output:\n{}",
                tv.version,
                sdk_list_display(&sdks)
            )
        }
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

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        Ok(DotnetOptions::new(&raw_opts).lockfile_options())
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let index: ReleasesIndex = HTTP_FETCH.json(RELEASES_INDEX_URL).await?;

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

        Ok(sdk_versions(&channels))
    }

    async fn resolve_dynamic_alias(&self, version: &str) -> Result<Option<String>> {
        let release_type = match version {
            "lts" => "lts",
            "sts" => "sts",
            _ => return Ok(None),
        };
        static CHANNELS: Lazy<CacheManager<Vec<Channel>>> = Lazy::new(|| {
            CacheManagerBuilder::new(dirs::CACHE.join("dotnet").join("channels.msgpack.z"))
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build()
        });
        // Offline, an expired cache still answers: the channel list changes a few times
        // a year, and refusing here would break `latest --installed dotnet@lts`.
        let stale;
        let offline_cache = if Settings::get().offline() {
            CHANNELS.get_cached().ok()
        } else {
            None
        };
        let channels = if let Some(channels) = offline_cache {
            stale = channels;
            &stale
        } else {
            CHANNELS
                .get_or_try_init_async(async || {
                    let index: ReleasesIndex = HTTP_FETCH.json(RELEASES_INDEX_URL).await?;
                    Ok(index
                        .releases_index
                        .into_iter()
                        .filter_map(|ch| {
                            Some(Channel {
                                version: ch.channel_version?,
                                release_type: ch.release_type?,
                                support_phase: ch.support_phase?,
                            })
                        })
                        .collect())
                })
                .await?
        };
        select_channel(channels, release_type)
            .map(|channel| Some(channel.version.clone()))
            .ok_or_else(|| eyre::eyre!("no supported .NET {release_type} channel found"))
    }

    async fn _parse_idiomatic_file(&self, path: &Path) -> Result<Vec<String>> {
        let content = file::read_to_string(path)?;
        let global_json: GlobalJson = serde_json::from_str(&content)?;
        let sdk = global_json
            .sdk
            .ok_or_else(|| eyre::eyre!("no sdk.version found in {}", path.display()))?;
        if sdk.version.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![sdk.version])
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let isolated = Self::is_isolated();
        let install_dir = if isolated {
            tv.install_path()
        } else {
            dotnet_root()
        };
        file::create_dir_all(&install_dir)?;

        // Read and validate runtime options
        let raw_opts = tv.request.options();
        let opts = DotnetOptions::new(&raw_opts);
        let runtime = opts.runtime().map(str::to_string);
        if let Some(ref rt) = runtime
            && opts.runtime_framework_name().is_none()
        {
            return Err(eyre::eyre!(
                "Invalid runtime option '{}'. Valid options: dotnet, aspnetcore, windowsdesktop",
                rt
            ));
        }

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
        let install_type = if runtime.is_some() { "Runtime" } else { "SDK" };
        ctx.pr
            .set_message(format!("Installing .NET {} {}", install_type, tv.version));
        install_cmd(&script_path, &install_dir, &tv.version, runtime.as_deref())
            .with_pr(ctx.pr.as_ref())
            .env_values(tv.install_env())
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
            let raw_opts = tv.request.options();
            let opts = DotnetOptions::new(&raw_opts);
            if opts.runtime().is_some() {
                // Runtime: remove the shared runtime directory for this version
                let Some(framework) = opts.runtime_framework_name() else {
                    return Ok(());
                };
                let runtime_dir = dotnet_root()
                    .join("shared")
                    .join(framework)
                    .join(&tv.version);
                if runtime_dir.exists() {
                    file::remove_all(&runtime_dir)?;
                }
            } else {
                // SDK: only remove this SDK version from the shared root
                let sdk_dir = dotnet_root().join("sdk").join(&tv.version);
                if sdk_dir.exists() {
                    file::remove_all(&sdk_dir)?;
                }
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

fn runtime_framework_name(runtime: &str) -> Option<&'static str> {
    match runtime {
        "dotnet" => Some("Microsoft.NETCore.App"),
        "aspnetcore" => Some("Microsoft.AspNetCore.App"),
        "windowsdesktop" => Some("Microsoft.WindowsDesktop.App"),
        _ => None,
    }
}

fn sdk_list_contains_version(lines: &[String], version: &str) -> bool {
    lines
        .iter()
        .any(|line| line.split_whitespace().next() == Some(version))
}

fn sdk_list_display(lines: &[String]) -> String {
    if lines.is_empty() {
        "(no SDKs listed)".to_string()
    } else {
        lines.join("\n")
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
fn install_cmd<'a>(
    script_path: &Path,
    install_dir: &Path,
    version: &str,
    runtime: Option<&str>,
) -> CmdLineRunner<'a> {
    let mut cmd = CmdLineRunner::new(script_path)
        .arg("--install-dir")
        .arg(install_dir)
        .arg("--version")
        .arg(version)
        .arg("--no-path");
    if let Some(rt) = runtime {
        cmd = cmd.arg("--runtime").arg(rt);
    }
    cmd
}

#[cfg(windows)]
fn install_cmd<'a>(
    script_path: &Path,
    install_dir: &Path,
    version: &str,
    runtime: Option<&str>,
) -> CmdLineRunner<'a> {
    let mut cmd = CmdLineRunner::new("powershell")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(script_path)
        .arg("-InstallDir")
        .arg(install_dir)
        .arg("-Version")
        .arg(version)
        .arg("-NoPath");
    if let Some(rt) = runtime {
        cmd = cmd.arg("-Runtime").arg(rt);
    }
    cmd
}

// --- Microsoft releases API types ---

const RELEASES_INDEX_URL: &str =
    "https://builds.dotnet.microsoft.com/dotnet/release-metadata/releases-index.json";

/// A .NET release channel such as `10.0`, as the releases index describes it.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Channel {
    version: String,
    release_type: String,
    support_phase: String,
}

/// The newest channel of a release type that Microsoft supports for production.
///
/// `preview` and `go-live` channels are not stable yet, and `eol` ones are out of
/// support, so `lts` and `sts` skip them, matching `dotnet-install --channel`.
/// Channel versions are always `major.minor`, so they are compared numerically.
fn select_channel<'a>(channels: &'a [Channel], release_type: &str) -> Option<&'a Channel> {
    let key = |ch: &Channel| -> Option<(u32, u32)> {
        let (major, minor) = ch.version.split_once('.')?;
        Some((major.parse().ok()?, minor.parse().ok()?))
    };
    channels
        .iter()
        .filter(|ch| ch.release_type.eq_ignore_ascii_case(release_type))
        .filter(|ch| matches!(ch.support_phase.as_str(), "active" | "maintenance"))
        .filter_map(|ch| Some((key(ch)?, ch)))
        .max_by_key(|(key, _)| *key)
        .map(|(_, ch)| ch)
}

#[derive(Deserialize)]
struct ReleasesIndex {
    #[serde(rename = "releases-index")]
    releases_index: Vec<ChannelEntry>,
}

#[derive(Deserialize)]
struct ChannelEntry {
    #[serde(rename = "releases.json")]
    releases_json: Option<String>,
    #[serde(rename = "channel-version")]
    channel_version: Option<String>,
    #[serde(rename = "release-type")]
    release_type: Option<String>,
    #[serde(rename = "support-phase")]
    support_phase: Option<String>,
}

#[derive(Deserialize)]
struct ChannelReleases {
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    #[serde(rename = "release-date")]
    release_date: Option<String>,
    sdk: Option<Sdk>,
    sdks: Option<Vec<Sdk>>,
}

#[derive(Deserialize)]
struct Sdk {
    version: Option<String>,
}

/// Every SDK version the channels list, oldest first, dated with the release
/// that first shipped it. Runtime-only releases repeat the SDKs of earlier
/// ones, so a version keeps the earliest `release-date` it appears under.
fn sdk_versions(channels: &[ChannelReleases]) -> Vec<VersionInfo> {
    let mut versions: std::collections::BTreeMap<SortedVersion, Option<String>> =
        Default::default();
    for release in channels.iter().flat_map(|c| &c.releases) {
        let date = release.release_date.as_ref().filter(|d| !d.is_empty());
        let sdks = release.sdk.iter().chain(release.sdks.iter().flatten());
        for version in sdks.filter_map(|sdk| sdk.version.clone()) {
            let entry = versions.entry(SortedVersion(version)).or_default();
            // ISO dates order lexically, so this is not a version comparison.
            if let Some(date) = date
                && entry.as_ref().is_none_or(|known| date < known)
            {
                *entry = Some(date.clone());
            }
        }
    }
    versions
        .into_iter()
        .map(|(v, created_at)| VersionInfo {
            version: v.0,
            created_at,
            ..Default::default()
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duration::parse_into_timestamp;
    use crate::toolset::ToolSource;

    fn opts_with_runtime(runtime: &str) -> ToolVersionOptions {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "runtime".to_string(),
            toml::Value::String(runtime.to_string()),
        );
        opts
    }

    #[test]
    fn dotnet_options_reads_runtime() {
        let opts = opts_with_runtime("aspnetcore");
        let parsed = DotnetOptions::new(&opts);

        assert_eq!(parsed.runtime(), Some("aspnetcore"));
        assert_eq!(
            parsed.runtime_framework_name(),
            Some("Microsoft.AspNetCore.App")
        );
    }

    #[test]
    fn dotnet_lockfile_options_include_runtime() {
        let opts = opts_with_runtime("dotnet");
        assert_eq!(
            DotnetOptions::new(&opts).lockfile_options(),
            BTreeMap::from([("runtime".to_string(), "dotnet".to_string())])
        );
        assert!(
            DotnetOptions::new(&ToolVersionOptions::default())
                .lockfile_options()
                .is_empty()
        );
    }

    #[test]
    fn resolve_lockfile_options_include_runtime() {
        let backend = DotnetPlugin::new();
        let request = ToolRequest::new_with_options(
            backend.ba().clone(),
            "8.0.14",
            opts_with_runtime("aspnetcore"),
            ToolSource::Unknown,
        )
        .unwrap();

        assert_eq!(
            backend
                .resolve_lockfile_options(&request, &PlatformTarget::from_current())
                .unwrap(),
            BTreeMap::from([("runtime".to_string(), "aspnetcore".to_string())])
        );

        let request_no_runtime = ToolRequest::new_with_options(
            backend.ba().clone(),
            "8.0.14",
            ToolVersionOptions::default(),
            ToolSource::Unknown,
        )
        .unwrap();
        assert!(
            backend
                .resolve_lockfile_options(&request_no_runtime, &PlatformTarget::from_current())
                .unwrap()
                .is_empty()
        );
    }

    fn channel_releases(json: &str) -> ChannelReleases {
        serde_json::from_str(json).unwrap()
    }

    const CHANNEL_JSON: &str = r#"{"releases": [
        {"release-date": "2024-02-13", "sdk": {"version": "8.0.200"},
         "sdks": [{"version": "8.0.200"}, {"version": "8.0.101"}]},
        {"release-date": "2024-01-09", "sdk": {"version": "8.0.101"}},
        {"release-date": "2024-03-12", "sdk": {"version": "8.0.201"}},
        {"release-date": "", "sdk": {"version": "8.0.300-preview.1"}}
    ]}"#;

    #[test]
    fn sdk_versions_carry_the_earliest_release_date() {
        let versions = sdk_versions(&[channel_releases(CHANNEL_JSON)]);
        let dated: Vec<_> = versions
            .iter()
            .map(|v| (v.version.as_str(), v.created_at.as_deref()))
            .collect();
        assert_eq!(
            dated,
            [
                ("8.0.101", Some("2024-01-09")),
                ("8.0.200", Some("2024-02-13")),
                ("8.0.201", Some("2024-03-12")),
                ("8.0.300-preview.1", None),
            ]
        );
    }

    #[test]
    fn sdk_versions_released_after_the_cutoff_are_hidden() {
        let versions = sdk_versions(&[channel_releases(CHANNEL_JSON)]);
        let cutoff = parse_into_timestamp("2024-02-20T00:00:00Z").unwrap();
        let kept: Vec<_> = VersionInfo::filter_by_date(versions, cutoff)
            .into_iter()
            .map(|v| v.version)
            .collect();
        assert_eq!(kept, ["8.0.101", "8.0.200", "8.0.300-preview.1"]);
    }

    fn channel(version: &str, release_type: &str, support_phase: &str) -> Channel {
        Channel {
            version: version.to_string(),
            release_type: release_type.to_string(),
            support_phase: support_phase.to_string(),
        }
    }

    fn channels() -> Vec<Channel> {
        vec![
            channel("11.0", "sts", "go-live"),
            channel("10.0", "lts", "active"),
            channel("9.0", "sts", "maintenance"),
            channel("8.0", "lts", "maintenance"),
            channel("7.0", "sts", "eol"),
            channel("6.0", "lts", "eol"),
        ]
    }

    #[test]
    fn select_channel_picks_newest_supported_lts() {
        let channels = channels();
        assert_eq!(select_channel(&channels, "lts").unwrap().version, "10.0");
    }

    #[test]
    fn select_channel_skips_go_live_and_eol_sts() {
        let channels = channels();
        assert_eq!(select_channel(&channels, "sts").unwrap().version, "9.0");
    }

    #[test]
    fn select_channel_compares_versions_numerically() {
        let channels = vec![
            channel("9.0", "lts", "active"),
            channel("10.0", "lts", "active"),
        ];
        assert_eq!(select_channel(&channels, "lts").unwrap().version, "10.0");
    }

    #[test]
    fn select_channel_is_none_without_a_supported_match() {
        let channels = vec![
            channel("7.0", "sts", "eol"),
            channel("11.0", "sts", "preview"),
        ];
        assert!(select_channel(&channels, "sts").is_none());
    }

    #[test]
    fn sdk_list_contains_exact_installed_version() {
        let lines = vec![
            "8.0.128 [/Users/test/.local/share/mise/dotnet-root/sdk]".to_string(),
            "10.0.301 [/opt/homebrew/Cellar/dotnet/10.0.301/libexec/sdk]".to_string(),
        ];

        assert!(sdk_list_contains_version(&lines, "8.0.128"));
    }

    #[test]
    fn sdk_list_does_not_match_prefix_or_path_only() {
        let lines = vec![
            "8.0.1280 [/Users/test/.local/share/mise/dotnet-root/sdk]".to_string(),
            "10.0.301 [/tmp/8.0.128/sdk]".to_string(),
        ];

        assert!(!sdk_list_contains_version(&lines, "8.0.128"));
    }

    #[test]
    fn sdk_list_display_explains_empty_output() {
        assert_eq!(sdk_list_display(&[]), "(no SDKs listed)");
    }

    #[test]
    fn sdk_list_display_joins_lines() {
        let lines = vec![
            "8.0.128 [/path]".to_string(),
            "10.0.301 [/path2]".to_string(),
        ];

        assert_eq!(
            sdk_list_display(&lines),
            "8.0.128 [/path]\n10.0.301 [/path2]"
        );
    }
}
