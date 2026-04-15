use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::{CmdLineRunner, cmd};
use crate::config::Config;
use crate::config::Settings;
use crate::hash::hash_to_str;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::{ToolRequest, ToolVersion};
use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Deserializer;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Semaphore;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct GoBackend {
    ba: Arc<BackendArg>,
    module_versions_cache: DashMap<String, CacheManager<Option<Vec<VersionInfo>>>>,
}

#[async_trait]
impl Backend for GoBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Go
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["go"])
    }

    fn supports_lockfile_url(&self) -> bool {
        false
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        // Check if go is available
        self.warn_if_dependency_missing(
            config,
            "go",
            &["go"],
            "To use go packages with mise, you need to install Go first:\n\
              mise use go@latest\n\n\
            Or install Go via https://go.dev/dl/",
        )
        .await;

        timeout::run_with_timeout_async(
            async || {
                let tool_name = self.tool_name();

                if let Some(versions) = self.fetch_proxy_versions(&tool_name).await? {
                    return Ok(versions);
                }

                // Fall back to `go list -m -versions` for GOPROXY=direct
                if let Some(versions) = self.fetch_go_module_versions(config, &tool_name).await?
                    && !versions.is_empty()
                {
                    return Ok(versions);
                }

                Ok(vec![])
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        // Check if go is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "go",
            &["go"],
            "To use go packages with mise, you need to install Go first:\n\
              mise use go@latest\n\n\
            Or install Go via https://go.dev/dl/",
        )
        .await;

        // Some modules have no tagged versions but resolve via @latest pseudo-version.
        let mut install_version = tv.version.clone();
        if tv.request.version() == "latest"
            && tv.version != "latest"
            && self
                .fetch_go_module_versions(&ctx.config, &self.tool_name())
                .await?
                .is_some_and(|v| v.is_empty())
        {
            install_version = "latest".to_string();
            tv.version = "latest".to_string();
        }

        let opts = self.ba.opts();

        let install = async |v| {
            let mut cmd = CmdLineRunner::new("go").arg("install").arg("-mod=readonly");

            if let Some(tags) = opts.get("tags") {
                cmd = cmd.arg("-tags").arg(tags);
            }

            cmd.arg(format!("{}@{v}", self.tool_name()))
                .with_pr(ctx.pr.as_ref())
                .envs(self.dependency_env(&ctx.config).await?)
                .env("GOBIN", tv.install_path().join("bin"))
                .execute()
        };

        // try "v" prefix if the version starts with semver
        let use_v = regex!(r"^\d+\.\d+\.\d+").is_match(&install_version);

        if use_v {
            if install(format!("v{}", install_version)).await.is_err() {
                warn!("Failed to install, trying again without added 'v' prefix");
            } else {
                return Ok(tv);
            }
        }

        install(install_version).await?;

        Ok(tv)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        let opts = request.options();
        let mut result = BTreeMap::new();

        // tags affect compilation
        if let Some(value) = opts.get("tags") {
            result.insert("tags".to_string(), value.to_string());
        }

        result
    }
}

/// Returns install-time-only option keys for Go backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec!["tags".into()]
}

const DEFAULT_GOPROXY: &str = "https://proxy.golang.org,direct";
const GO_PROXY_VERSION_INFO_CONCURRENCY: usize = 20;
const GO_LIST_VERSION_INFO_BATCH_SIZE: usize = 50;

impl GoBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            ba: Arc::new(ba),
            module_versions_cache: Default::default(),
        }
    }

    /// Query `$GOPROXY` to find versions, matching `go install`'s resolution algorithm.
    /// Returns `None` if no proxy is configured (e.g. GOPROXY=direct).
    async fn fetch_proxy_versions(
        &self,
        tool_name: &str,
    ) -> eyre::Result<Option<Vec<VersionInfo>>> {
        let proxies = parse_goproxy();
        if proxies.is_empty() {
            return Ok(None);
        }

        let parts: Vec<&str> = tool_name.split('/').collect();
        let candidates: Vec<String> = (1..=parts.len())
            .rev()
            .map(|i| parts[..i].join("/"))
            .collect();

        let mut join_set = tokio::task::JoinSet::new();
        for (idx, path) in candidates.iter().enumerate() {
            let encoded = encode_module_path(path);
            let proxies = proxies.clone();
            join_set.spawn(async move {
                let result = query_proxy_list(&proxies, &encoded).await;
                (idx, result)
            });
        }

        let mut list_results: Vec<(usize, ProxyListResult)> = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(r) => list_results.push(r),
                Err(e) => warn!("proxy query task panicked: {e}"),
            }
        }
        list_results.sort_by_key(|(idx, _)| *idx);

        for (idx, result) in &list_results {
            let path = &candidates[*idx];
            match result {
                ProxyListResult::Versions(versions) if !versions.is_empty() => {
                    let versions: Vec<String> = versions
                        .iter()
                        .filter(|v| Versioning::new(v.trim_start_matches('v')).is_some())
                        .cloned()
                        .collect();
                    if versions.is_empty() {
                        let encoded = encode_module_path(path);
                        match query_proxy_latest(&proxies, &encoded).await {
                            ProxyVersionInfoResult::Found(info) => {
                                return Ok(Some(vec![version_info_from_metadata(info)]));
                            }
                            ProxyVersionInfoResult::NotFound => continue,
                            ProxyVersionInfoResult::Error => return Ok(None),
                        }
                    }
                    let mut version_infos = fetch_proxy_version_infos(&proxies, path, &versions).await;
                    version_infos.sort_by_cached_key(|v| Versioning::new(&v.version));
                    return Ok(Some(version_infos));
                }
                ProxyListResult::Versions(_) => {
                    // Check if @latest resolves (module using pseudo-versions)
                    let encoded = encode_module_path(path);
                    match query_proxy_latest(&proxies, &encoded).await {
                        ProxyVersionInfoResult::Found(info) => {
                            return Ok(Some(vec![version_info_from_metadata(info)]));
                        }
                        ProxyVersionInfoResult::NotFound => continue,
                        ProxyVersionInfoResult::Error => return Ok(None),
                    }
                }
                ProxyListResult::NotFound => continue,
                ProxyListResult::Error => return Ok(None),
            }
        }

        Ok(None)
    }

    async fn fetch_go_module_versions(
        &self,
        config: &Arc<Config>,
        mod_path: &str,
    ) -> eyre::Result<Option<Vec<VersionInfo>>> {
        let cache = self
            .module_versions_cache
            .entry(mod_path.to_string())
            .or_insert_with(|| {
                let filename = format!("{}.msgpack.z", hash_to_str(&mod_path.to_string()));
                CacheManagerBuilder::new(
                    self.ba.cache_path.join("go_module_versions").join(filename),
                )
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build()
            });

        cache
            .get_or_try_init_async(async || {
                let raw = match cmd!(
                    "go",
                    "list",
                    "-mod=readonly",
                    "-m",
                    "-versions",
                    "-json",
                    mod_path
                )
                .full_env(self.dependency_env(config).await?)
                .read()
                {
                    Ok(raw) => raw,
                    Err(_) => return Ok(None),
                };

                let mod_info = match serde_json::from_str::<GoModInfo>(&raw) {
                    Ok(info) => info,
                    Err(_) => return Ok(None),
                };

                let versions = self
                    .fetch_go_module_version_infos(config, mod_path, &mod_info.versions)
                    .await;

                Ok(Some(versions))
            })
            .await
            .cloned()
    }

    async fn fetch_go_module_version_infos(
        &self,
        config: &Arc<Config>,
        mod_path: &str,
        versions: &[String],
    ) -> Vec<VersionInfo> {
        let env = match self.dependency_env(config).await {
            Ok(env) => env,
            Err(_) => {
                return versions
                    .iter()
                    .map(|version| VersionInfo {
                        version: version.trim_start_matches('v').to_string(),
                        ..Default::default()
                    })
                    .collect();
            }
        };

        let mut metadata_by_version = HashMap::with_capacity(versions.len());
        for chunk in versions.chunks(GO_LIST_VERSION_INFO_BATCH_SIZE) {
            let mut args = vec![
                OsString::from("list"),
                OsString::from("-mod=readonly"),
                OsString::from("-m"),
                OsString::from("-json"),
            ];
            for version in chunk {
                args.push(format!("{mod_path}@{version}").into());
            }
            let Ok(raw) = cmd("go", args).full_env(&env).read() else {
                continue;
            };
            let Ok(infos) = Deserializer::from_str(&raw)
                .into_iter::<GoModuleVersionMetadata>()
                .collect::<Result<Vec<_>, _>>()
            else {
                continue;
            };
            for info in infos {
                metadata_by_version.insert(info.version.clone(), info);
            }
        }

        versions
            .iter()
            .map(|version| match metadata_by_version.remove(version) {
                Some(info) => version_info_from_metadata(info),
                None => VersionInfo {
                    version: version.trim_start_matches('v').to_string(),
                    ..Default::default()
                },
            })
            .collect()
    }
}

enum ProxyListResult {
    Versions(Vec<String>),
    NotFound,
    Error,
}

enum ProxyVersionInfoResult {
    Found(GoModuleVersionMetadata),
    NotFound,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
enum FallThrough {
    OnNotFound,
    OnAnyError,
}

#[derive(Clone)]
struct GoProxy {
    url: String,
    fall_through: FallThrough,
}

async fn query_proxy_list(proxies: &[GoProxy], encoded_path: &str) -> ProxyListResult {
    for proxy in proxies {
        let url = format!("{}/{}/@v/list", proxy.url, encoded_path);
        match HTTP_FETCH.get_text(&url).await {
            Ok(body) => {
                let versions: Vec<String> = body
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                return ProxyListResult::Versions(versions);
            }
            Err(e) => {
                let is_not_found_or_gone = e
                    .downcast_ref::<reqwest::Error>()
                    .and_then(|e| e.status())
                    .is_some_and(|s| {
                        s == reqwest::StatusCode::NOT_FOUND || s == reqwest::StatusCode::GONE
                    });

                if is_not_found_or_gone || proxy.fall_through == FallThrough::OnAnyError {
                    continue;
                }
                return ProxyListResult::Error;
            }
        }
    }

    ProxyListResult::NotFound
}

async fn query_proxy_latest(proxies: &[GoProxy], encoded_path: &str) -> ProxyVersionInfoResult {
    query_proxy_version_metadata(proxies, &format!("{encoded_path}/@latest")).await
}

async fn query_proxy_version_metadata(
    proxies: &[GoProxy],
    endpoint: &str,
) -> ProxyVersionInfoResult {
    for proxy in proxies {
        let url = format!("{}/{}", proxy.url, endpoint);
        match HTTP_FETCH.get_text(&url).await {
            Ok(body) => match serde_json::from_str::<GoModuleVersionMetadata>(&body) {
                Ok(info) => return ProxyVersionInfoResult::Found(info),
                Err(_) => return ProxyVersionInfoResult::Error,
            },
            Err(e) => {
                let is_not_found_or_gone = e
                    .downcast_ref::<reqwest::Error>()
                    .and_then(|e| e.status())
                    .is_some_and(|s| {
                        s == reqwest::StatusCode::NOT_FOUND || s == reqwest::StatusCode::GONE
                    });

                if is_not_found_or_gone || proxy.fall_through == FallThrough::OnAnyError {
                    continue;
                }
                return ProxyVersionInfoResult::Error;
            }
        }
    }
    ProxyVersionInfoResult::NotFound
}

fn parse_goproxy() -> Vec<GoProxy> {
    let goproxy = std::env::var("GOPROXY").unwrap_or_else(|_| DEFAULT_GOPROXY.to_string());
    parse_goproxy_value(&goproxy)
}

/// Parse GOPROXY value per https://go.dev/ref/mod#goproxy-protocol:
/// - Comma after a URL: fall through to next entry only on 404/410.
/// - Pipe after a URL: fall through to next entry on any error.
fn parse_goproxy_value(goproxy: &str) -> Vec<GoProxy> {
    let mut proxies = Vec::new();
    let mut rest = goproxy;
    while !rest.is_empty() {
        let (entry, separator) = match rest.find([',', '|']) {
            Some(pos) => {
                let sep = rest.as_bytes()[pos];
                let entry = &rest[..pos];
                rest = &rest[pos + 1..];
                (entry, Some(sep))
            }
            None => {
                let entry = rest;
                rest = "";
                (entry, None)
            }
        };
        let entry = entry.trim();
        match entry {
            "" | "direct" => continue,
            "off" => break,
            url => {
                proxies.push(GoProxy {
                    url: url.trim_end_matches('/').to_string(),
                    fall_through: if separator == Some(b'|') {
                        FallThrough::OnAnyError
                    } else {
                        FallThrough::OnNotFound
                    },
                });
            }
        }
    }
    proxies
}

/// Encode a module path per https://go.dev/ref/mod#goproxy-protocol
fn encode_module_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_ascii_uppercase() {
            encoded.push('!');
            encoded.push(c.to_ascii_lowercase());
        } else {
            encoded.push(c);
        }
    }
    encoded
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GoModInfo {
    #[serde(default)]
    versions: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GoModuleVersionMetadata {
    version: String,
    #[serde(default)]
    time: Option<String>,
}

fn version_info_from_metadata(info: GoModuleVersionMetadata) -> VersionInfo {
    VersionInfo {
        version: info.version.trim_start_matches('v').to_string(),
        created_at: info.time,
        ..Default::default()
    }
}

async fn fetch_proxy_version_infos(
    proxies: &[GoProxy],
    path: &str,
    versions: &[String],
) -> Vec<VersionInfo> {
    let encoded = Arc::new(encode_module_path(path));
    let proxies = Arc::new(proxies.to_vec());
    let sem = Arc::new(Semaphore::new(GO_PROXY_VERSION_INFO_CONCURRENCY));
    let mut join_set = tokio::task::JoinSet::new();

    for version in versions {
        let proxies = proxies.clone();
        let encoded = encoded.clone();
        let sem = sem.clone();
        let version = version.clone();
        join_set.spawn(async move {
            let _permit = sem.acquire_owned().await.expect("semaphore closed");
            let endpoint = format!("{encoded}/@v/{version}.info");
            let info = query_proxy_version_metadata(proxies.as_slice(), &endpoint).await;
            (version, info)
        });
    }

    let mut times = BTreeMap::new();
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok((version, ProxyVersionInfoResult::Found(info))) => {
                times.insert(version, info.time);
            }
            Ok((version, ProxyVersionInfoResult::NotFound | ProxyVersionInfoResult::Error)) => {
                times.insert(version, None);
            }
            Err(e) => warn!("proxy version info task panicked: {e}"),
        }
    }

    versions
        .iter()
        .map(|version| VersionInfo {
            version: version.trim_start_matches('v').to_string(),
            created_at: times.get(version).cloned().flatten(),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_go_mod_info_without_versions() {
        let raw = r#"{"Path":"github.com/go-kratos/kratos/cmd/kratos/v2"}"#;
        let info: GoModInfo = serde_json::from_str(raw).unwrap();
        assert!(info.versions.is_empty());
    }

    #[test]
    fn parse_go_mod_info_with_versions() {
        let raw = r#"{"Path":"example.com/mod","Versions":["v1.0.0","v1.1.0"]}"#;
        let info: GoModInfo = serde_json::from_str(raw).unwrap();
        assert_eq!(info.versions, vec!["v1.0.0", "v1.1.0"]);
    }

    #[test]
    fn parse_go_module_version_metadata() {
        let raw = r#"{"Version":"v1.2.3","Time":"2026-04-08T12:56:30Z"}"#;
        let info: GoModuleVersionMetadata = serde_json::from_str(raw).unwrap();
        assert_eq!(info.version, "v1.2.3");
        assert_eq!(info.time, Some("2026-04-08T12:56:30Z".to_string()));
    }

    #[test]
    fn encode_module_path_lowercase() {
        assert_eq!(
            encode_module_path("github.com/foo/bar"),
            "github.com/foo/bar"
        );
    }

    #[test]
    fn encode_module_path_uppercase() {
        assert_eq!(
            encode_module_path("github.com/GoogleCloudPlatform/scion"),
            "github.com/!google!cloud!platform/scion"
        );
    }

    #[test]
    fn parse_goproxy_default() {
        let proxies = parse_goproxy_value("https://proxy.golang.org,direct");
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].url, "https://proxy.golang.org");
        assert_eq!(proxies[0].fall_through, FallThrough::OnNotFound);
    }

    #[test]
    fn parse_goproxy_pipe_separated() {
        let proxies =
            parse_goproxy_value("https://corp-proxy.example.com|https://proxy.golang.org|direct");
        assert_eq!(proxies.len(), 2);
        assert_eq!(proxies[0].url, "https://corp-proxy.example.com");
        assert_eq!(proxies[0].fall_through, FallThrough::OnAnyError);
        assert_eq!(proxies[1].url, "https://proxy.golang.org");
        assert_eq!(proxies[1].fall_through, FallThrough::OnAnyError);
    }

    #[test]
    fn parse_goproxy_mixed_separators() {
        let proxies =
            parse_goproxy_value("https://corp-proxy.example.com|https://proxy.golang.org,direct");
        assert_eq!(proxies.len(), 2);
        assert_eq!(proxies[0].url, "https://corp-proxy.example.com");
        assert_eq!(proxies[0].fall_through, FallThrough::OnAnyError);
        assert_eq!(proxies[1].url, "https://proxy.golang.org");
        assert_eq!(proxies[1].fall_through, FallThrough::OnNotFound);
    }

    #[test]
    fn parse_goproxy_direct_only() {
        let proxies = parse_goproxy_value("direct");
        assert!(proxies.is_empty());
    }

    #[test]
    fn parse_goproxy_off() {
        let proxies = parse_goproxy_value("off");
        assert!(proxies.is_empty());
    }

    #[test]
    fn parse_goproxy_off_stops_parsing() {
        let proxies =
            parse_goproxy_value("https://corp-proxy.example.com,off,https://proxy.golang.org");
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].url, "https://corp-proxy.example.com");
    }
}
