use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::config::Settings;
use crate::hash::hash_to_str;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::{ToolRequest, ToolVersion};
use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::BTreeMap;
use std::{fmt::Debug, sync::Arc};
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
                    let mut version_infos: Vec<VersionInfo> = versions
                        .iter()
                        .map(|v| VersionInfo {
                            version: v.trim_start_matches('v').to_string(),
                            ..Default::default()
                        })
                        .collect();
                    version_infos.retain(|v| Versioning::new(&v.version).is_some());
                    version_infos.sort_by_cached_key(|v| Versioning::new(&v.version));
                    return Ok(Some(version_infos));
                }
                ProxyListResult::Versions(_) => {
                    // Check if @latest resolves (module using pseudo-versions)
                    let encoded = encode_module_path(path);
                    match query_proxy_latest(&proxies, &encoded).await {
                        ProxyListResult::Versions(_) => return Ok(Some(vec![])),
                        ProxyListResult::NotFound => continue,
                        ProxyListResult::Error => return Ok(None),
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

                // remove the leading v from the versions
                let versions = mod_info
                    .versions
                    .into_iter()
                    .map(|v| VersionInfo {
                        version: v.trim_start_matches('v').to_string(),
                        ..Default::default()
                    })
                    .collect();

                Ok(Some(versions))
            })
            .await
            .cloned()
    }
}

enum ProxyListResult {
    Versions(Vec<String>),
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

async fn query_proxy_latest(proxies: &[GoProxy], encoded_path: &str) -> ProxyListResult {
    for proxy in proxies {
        let url = format!("{}/{}/@latest", proxy.url, encoded_path);
        match HTTP_FETCH.get_text(&url).await {
            Ok(_) => return ProxyListResult::Versions(vec![]),
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
