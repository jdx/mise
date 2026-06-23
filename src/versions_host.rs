use crate::backend::VersionInfo;
use crate::config::Settings;
use crate::github::GithubRelease;
use crate::http;
use crate::http::HTTP_FETCH;
use crate::plugins::core::CORE_PLUGINS;
use crate::registry::REGISTRY;
use mise_sigstore::Attestation;
use reqwest::header::{HeaderMap, HeaderValue};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;

/// Headers for requests to mise-versions, including CI detection
static VERSIONS_HOST_HEADERS: LazyLock<HeaderMap> = LazyLock::new(|| {
    let mut headers = HeaderMap::new();
    if ci_info::is_ci() {
        headers.insert("x-mise-ci", HeaderValue::from_static("true"));
    }
    headers
});

/// Tools that use the versions host for listing versions
/// (excludes java/python due to complex version schemes)
static PLUGINS_USE_VERSION_HOST: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    CORE_PLUGINS
        .keys()
        .map(|name| name.as_str())
        .chain(REGISTRY.keys())
        .filter(|name| !matches!(*name, "java" | "python"))
        .collect()
});

/// Tools that should have downloads tracked
/// (all core plugins and registry tools, including java/python)
static PLUGINS_TRACK_DOWNLOADS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    CORE_PLUGINS
        .keys()
        .map(|name| name.as_str())
        .chain(REGISTRY.keys())
        .collect()
});

/// Response format from the versions host TOML endpoint
#[derive(serde::Deserialize)]
struct VersionsResponse {
    versions: indexmap::IndexMap<String, VersionEntry>,
}

const VERSION_LIST_RETRIES: i64 = 1;

#[derive(serde::Deserialize)]
struct VersionEntry {
    created_at: toml::value::Datetime,
    #[serde(default)]
    release_url: Option<String>,
    /// Pre-release flag, when the producing source can distinguish it. Defaults
    /// to false so old host data — and entries from sources that don't track
    /// prereleases — stay correct without any schema upgrade. Old mise clients
    /// that don't know about this field ignore it (toml-rs accepts unknown
    /// fields by default), so populating it in mise-versions is forward-compatible.
    #[serde(default)]
    prerelease: bool,
}

#[derive(serde::Deserialize)]
struct AttestationsResponse {
    attestations: Vec<Attestation>,
}

#[derive(Clone, Copy)]
struct VersionsHostLogContext<'a> {
    endpoint: &'static str,
    tool: Option<&'a str>,
    repo: Option<&'a str>,
    tag: Option<&'a str>,
    digest: Option<&'a str>,
    full: Option<&'a str>,
    version: Option<&'a str>,
}

impl<'a> VersionsHostLogContext<'a> {
    fn version_list(tool: &'a str) -> Self {
        Self {
            endpoint: "version_list",
            tool: Some(tool),
            repo: None,
            tag: None,
            digest: None,
            full: None,
            version: None,
        }
    }

    fn github_release(repo: &'a str, tag: &'a str) -> Self {
        Self {
            endpoint: "github_release",
            tool: None,
            repo: Some(repo),
            tag: Some(tag),
            digest: None,
            full: None,
            version: None,
        }
    }

    fn github_attestations(repo: &'a str, digest: &'a str) -> Self {
        Self {
            endpoint: "github_attestations",
            tool: None,
            repo: Some(repo),
            tag: None,
            digest: Some(digest),
            full: None,
            version: None,
        }
    }

    fn install_track(tool: &'a str, full: &'a str, version: &'a str) -> Self {
        Self {
            endpoint: "install_track",
            tool: Some(tool),
            repo: None,
            tag: None,
            digest: None,
            full: Some(full),
            version: Some(version),
        }
    }

    fn fields(&self) -> String {
        let mut fields = format!("endpoint={}", self.endpoint);
        if let Some(tool) = self.tool {
            fields.push_str(&format!(" tool={}", log_value(tool)));
        }
        if let Some(repo) = self.repo {
            fields.push_str(&format!(" repo={}", log_value(repo)));
        }
        if let Some(tag) = self.tag {
            fields.push_str(&format!(" tag={}", log_value(tag)));
        }
        if let Some(digest) = self.digest {
            fields.push_str(&format!(" digest={}", log_value(digest)));
        }
        if let Some(full) = self.full {
            fields.push_str(&format!(
                " full={}",
                log_value(&sanitize_full_for_log(full))
            ));
        }
        if let Some(version) = self.version {
            fields.push_str(&format!(" version={}", log_value(version)));
        }
        fields
    }
}

fn log_value(value: &str) -> String {
    if value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/' | b':' | b'@'))
    {
        value.to_string()
    } else {
        format!("{:?}", value)
    }
}

fn sanitize_full_for_log(full: &str) -> String {
    let Some((backend, value)) = full.split_once(':') else {
        return full.to_string();
    };
    if let Ok(mut url) = url::Url::parse(value) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        return format!("{backend}:{url}");
    }
    full.to_string()
}

fn log_versions_host_trace(ctx: VersionsHostLogContext<'_>, outcome: &str, extra: &str) {
    if extra.is_empty() {
        trace!("mise-versions {} outcome={outcome}", ctx.fields());
    } else {
        trace!("mise-versions {} outcome={outcome} {extra}", ctx.fields());
    }
}

fn log_versions_host_warn(ctx: VersionsHostLogContext<'_>, outcome: &str, extra: &str) {
    if extra.is_empty() {
        warn!("mise-versions {} outcome={outcome}", ctx.fields());
    } else {
        warn!("mise-versions {} outcome={outcome} {extra}", ctx.fields());
    }
}

/// List versions from the versions host (mise-versions.jdx.dev).
/// Returns Vec<VersionInfo> with created_at timestamps from the TOML endpoint.
pub async fn list_versions(tool: &str) -> eyre::Result<Option<Vec<VersionInfo>>> {
    let ctx = VersionsHostLogContext::version_list(tool);
    let settings = Settings::get();
    if settings.prefer_offline()
        || !settings.use_versions_host
        || !PLUGINS_USE_VERSION_HOST.contains(tool)
    {
        log_versions_host_trace(ctx, "disabled", "fallback=true");
        return Ok(None);
    }

    static CACHE: LazyLock<Mutex<HashMap<String, Vec<VersionInfo>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    static RATE_LIMITED: AtomicBool = AtomicBool::new(false);

    if let Some(versions) = CACHE.lock().await.get(tool) {
        log_versions_host_trace(ctx, "cache_hit", &format!("versions={}", versions.len()));
        return Ok(Some(versions.clone()));
    }
    if RATE_LIMITED.load(Ordering::Relaxed) {
        log_versions_host_warn(ctx, "skipped_rate_limited", "fallback=true");
        return Ok(None);
    }

    // Use the static TOML asset which includes created_at timestamps.
    let url = version_list_url(tool);
    let versions: Vec<VersionInfo> = match HTTP_FETCH
        .get_text_request(&url)
        .headers(&VERSIONS_HOST_HEADERS)
        .retries(VERSION_LIST_RETRIES)
        .send()
        .await
    {
        Ok(body) => {
            let response: VersionsResponse = toml::from_str(&body)?;
            response
                .versions
                .into_iter()
                .map(|(version, entry)| VersionInfo {
                    version,
                    created_at: Some(entry.created_at.to_string()),
                    release_url: entry.release_url,
                    prerelease: entry.prerelease,
                    ..Default::default()
                })
                .collect()
        }
        Err(err) => match http::error_code(&err).unwrap_or(0) {
            404 => {
                log_versions_host_trace(ctx, "not_found", "status=404 fallback=true");
                return Ok(None);
            }
            429 => {
                RATE_LIMITED.store(true, Ordering::Relaxed);
                log_versions_host_warn(ctx, "rate_limited", "status=429 fallback=true");
                return Ok(None);
            }
            status => {
                log_versions_host_warn(
                    ctx,
                    "failed",
                    &format!(
                        "status={status} fallback=false error={}",
                        log_value(&err.to_string())
                    ),
                );
                return Err(err);
            }
        },
    };

    if versions.is_empty() {
        log_versions_host_trace(ctx, "empty", "fallback=true");
        return Ok(None);
    }

    log_versions_host_trace(ctx, "success", &format!("versions={}", versions.len()));

    CACHE
        .lock()
        .await
        .insert(tool.to_string(), versions.clone());
    Ok(Some(versions))
}

/// Fetch cached public GitHub release metadata from the versions host.
///
/// This endpoint is intentionally shaped like GitHub's release object so the
/// normal backend asset-selection code remains authoritative on the client.
pub async fn github_release(repo: &str, tag: &str) -> eyre::Result<Option<GithubRelease>> {
    if !enabled_for_github_metadata() {
        return Ok(None);
    }

    let Some((owner, repo_name)) = split_github_repo(repo) else {
        return Ok(None);
    };
    let url = format!(
        "https://mise-versions.jdx.dev/api/github/repos/{}/{}/releases/{}",
        encode_path_segment(owner),
        encode_path_segment(repo_name),
        encode_path_segment(tag)
    );

    let ctx = VersionsHostLogContext::github_release(repo, tag);
    let Some(release) = fetch_optional_json(&url, ctx).await? else {
        return Ok(None);
    };
    if !valid_github_release_asset_urls(&release, owner, repo_name) {
        log_versions_host_warn(ctx, "invalid_asset_urls", "fallback=true");
        return Ok(None);
    }
    if !valid_github_release_tag(&release, tag) {
        log_versions_host_warn(
            ctx,
            "tag_mismatch",
            &format!(
                "returned_tag={} fallback=true",
                log_value(&release.tag_name)
            ),
        );
        return Ok(None);
    }
    log_versions_host_trace(ctx, "success", "");
    Ok(Some(release))
}

/// Fetch cached GitHub Artifact Attestation payloads by artifact digest.
///
/// The returned bundles are not trusted by virtue of coming from mise-versions;
/// callers still verify them cryptographically against the downloaded artifact.
pub async fn github_attestations(
    repo: &str,
    digest: &str,
) -> eyre::Result<Option<Vec<Attestation>>> {
    if !enabled_for_github_metadata() {
        return Ok(None);
    }

    let Some((owner, repo_name)) = split_github_repo(repo) else {
        return Ok(None);
    };
    let url = format!(
        "https://mise-versions.jdx.dev/api/github/repos/{}/{}/attestations/{}",
        encode_path_segment(owner),
        encode_path_segment(repo_name),
        encode_digest_path_segment(digest)
    );

    let ctx = VersionsHostLogContext::github_attestations(repo, digest);
    let response: Option<AttestationsResponse> = fetch_optional_json(&url, ctx).await?;
    if let Some(response) = &response {
        log_versions_host_trace(
            ctx,
            "success",
            &format!("attestations={}", response.attestations.len()),
        );
    }
    Ok(response.map(|r| r.attestations))
}

async fn fetch_optional_json<T>(
    url: &str,
    ctx: VersionsHostLogContext<'_>,
) -> eyre::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    if Settings::get().offline() {
        log_versions_host_trace(ctx, "disabled", "fallback=true");
        return Ok(None);
    }

    debug!("GET {url}");
    match HTTP_FETCH
        .get_async_with_headers_allow_error_status(url, &VERSIONS_HOST_HEADERS)
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            debug!("GET {url} {status}");
            if status.is_success() {
                return Ok(Some(resp.json().await?));
            }
            let body = resp.text().await.unwrap_or_default();
            match status.as_u16() {
                404 => {
                    log_versions_host_trace(ctx, "not_found", "status=404 fallback=true");
                    Ok(None)
                }
                429 => {
                    log_versions_host_warn(ctx, "rate_limited", "status=429 fallback=true");
                    Ok(None)
                }
                status => {
                    log_versions_host_warn(
                        ctx,
                        "failed",
                        &format!(
                            "status={status} fallback=true error={}",
                            log_value(&versions_host_error_message(status, &body))
                        ),
                    );
                    Ok(None)
                }
            }
        }
        Err(err) => {
            log_versions_host_warn(
                ctx,
                "failed",
                &format!(
                    "status={} fallback=true error={}",
                    http::error_code(&err).unwrap_or(0),
                    log_value(&err.to_string())
                ),
            );
            Ok(None)
        }
    }
}

fn versions_host_error_message(status: u16, body: &str) -> String {
    let body = body.trim();
    let status_code = reqwest::StatusCode::from_u16(status);
    let label = match status_code {
        Ok(status) if status.is_client_error() => "HTTP status client error",
        Ok(status) if status.is_server_error() => "HTTP status server error",
        _ => "HTTP status error",
    };
    let status = status_code
        .map(|status| status.to_string())
        .unwrap_or_else(|_| status.to_string());
    if body.is_empty() {
        return format!("{label} ({status})");
    }
    format!(
        "{label} ({status}): {}",
        body.chars().take(200).collect::<String>()
    )
}

fn enabled_for_github_metadata() -> bool {
    let settings = Settings::get();
    !settings.prefer_offline() && settings.use_versions_host
}

fn split_github_repo(repo: &str) -> Option<(&str, &str)> {
    let (owner, name) = repo.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then_some((owner, name))
}

fn encode_path_segment(segment: &str) -> String {
    urlencoding::encode(segment).into_owned()
}

fn encode_digest_path_segment(digest: &str) -> String {
    encode_path_segment(digest).replace("%3A", ":")
}

fn valid_github_release_asset_urls(release: &GithubRelease, owner: &str, repo: &str) -> bool {
    !release.assets.is_empty()
        && release.assets.iter().all(|asset| {
            valid_github_browser_download_url(
                &asset.browser_download_url,
                owner,
                repo,
                &release.tag_name,
            ) && valid_github_asset_api_url(&asset.url, owner, repo)
        })
}

fn valid_github_release_tag(release: &GithubRelease, tag: &str) -> bool {
    tag == "latest" || release.tag_name == tag
}

fn valid_github_browser_download_url(url: &str, owner: &str, repo: &str, tag: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return false;
    }
    let mut segments = url.path_segments().into_iter().flatten();
    matches!(
        (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next()
        ),
        (Some(o), Some(r), Some("releases"), Some("download"), Some(t), Some(_asset), None)
            if github_repo_segment_matches(o, owner)
                && github_repo_segment_matches(r, repo)
                && path_segment_matches(t, tag)
    )
}

fn github_repo_segment_matches(segment: &str, expected: &str) -> bool {
    segment.eq_ignore_ascii_case(expected)
}

fn path_segment_matches(segment: &str, expected: &str) -> bool {
    segment == expected || urlencoding::decode(segment).is_ok_and(|decoded| decoded == expected)
}

fn valid_github_asset_api_url(url: &str, owner: &str, repo: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    if url.scheme() != "https" || url.host_str() != Some("api.github.com") {
        return false;
    }
    let mut segments = url.path_segments().into_iter().flatten();
    matches!(
        (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next()
        ),
        (Some("repos"), Some(o), Some(r), Some("releases"), Some("assets"), Some(_), None)
            if github_repo_segment_matches(o, owner) && github_repo_segment_matches(r, repo)
    )
}

/// Tracks a tool installation asynchronously (fire-and-forget)
/// Tracks all core plugins and registry tools (including java/python)
pub fn track_install(tool: &str, full: &str, version: &str) {
    let settings = Settings::get();
    if settings.offline() {
        return;
    }

    // Check if tracking is enabled (also requires use_versions_host to be enabled)
    if !settings.use_versions_host || !settings.use_versions_host_track {
        return;
    }

    // Only track known tools (core plugins and registry tools)
    if !PLUGINS_TRACK_DOWNLOADS.contains(tool) {
        return;
    }

    let tool = tool.to_string();
    let full = full.to_string();
    let version = version.to_string();

    // Fire-and-forget: spawn a task that won't block installation
    tokio::spawn(async move {
        if let Err(e) = track_install_async(&tool, &full, &version).await {
            trace!("Failed to track install for {tool}@{version}: {e}");
        }
    });
}

async fn track_install_async(tool: &str, full: &str, version: &str) -> eyre::Result<()> {
    use crate::cli::version::{ARCH, OS};

    let url = track_install_url(tool);

    let body = serde_json::json!({
        "full": full,
        "version": version,
        "os": *OS,
        "arch": *ARCH
    });

    match HTTP_FETCH
        .post_json_with_headers(url, &body, &VERSIONS_HOST_HEADERS)
        .await
    {
        Ok(true) => log_versions_host_trace(
            VersionsHostLogContext::install_track(tool, full, version),
            "success",
            "",
        ),
        Ok(false) => log_versions_host_trace(
            VersionsHostLogContext::install_track(tool, full, version),
            "failed",
            "status=unknown",
        ),
        Err(err) => log_versions_host_trace(
            VersionsHostLogContext::install_track(tool, full, version),
            "failed",
            &format!(
                "status={} error={}",
                http::error_code(&err).unwrap_or(0),
                log_value(&err.to_string())
            ),
        ),
    }

    Ok(())
}

fn track_install_url(tool: &str) -> String {
    format!(
        "https://mise-versions.jdx.dev/api/tools/{}",
        urlencoding::encode(tool)
    )
}

fn version_list_url(tool: &str) -> String {
    format!("https://mise-versions.jdx.dev/data/{}.toml", tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_install_url_encodes_tool_path_segment() {
        assert_eq!(
            track_install_url("ubi:https://example.com/foo/bar"),
            "https://mise-versions.jdx.dev/api/tools/ubi%3Ahttps%3A%2F%2Fexample.com%2Ffoo%2Fbar"
        );
    }

    #[test]
    fn test_track_install_url_for_registered_tool_name() {
        assert_eq!(
            track_install_url("node"),
            "https://mise-versions.jdx.dev/api/tools/node"
        );
    }

    #[test]
    fn test_version_list_url_uses_static_asset_path() {
        assert_eq!(
            version_list_url("node"),
            "https://mise-versions.jdx.dev/data/node.toml"
        );
    }

    #[test]
    fn test_versions_host_log_context_fields_quotes_spaces() {
        let fields =
            VersionsHostLogContext::install_track("some tool", "github:jdx/mise", "1.2.3").fields();
        assert_eq!(
            fields,
            r#"endpoint=install_track tool="some tool" full=github:jdx/mise version=1.2.3"#
        );
    }

    #[test]
    fn test_versions_host_log_context_redacts_full_url_credentials() {
        let fields = VersionsHostLogContext::install_track(
            "private",
            "github:https://user:token@example.com/org/repo?token=secret#frag",
            "1.2.3",
        )
        .fields();
        assert_eq!(
            fields,
            r#"endpoint=install_track tool=private full=github:https://example.com/org/repo version=1.2.3"#
        );
    }

    #[test]
    fn test_split_github_repo() {
        assert_eq!(split_github_repo("cli/cli"), Some(("cli", "cli")));
        assert_eq!(split_github_repo("cli"), None);
        assert_eq!(split_github_repo("cli/cli/extra"), None);
    }

    #[test]
    fn test_encode_path_segment_encodes_digest() {
        assert_eq!(encode_path_segment("sha256:abc/def"), "sha256%3Aabc%2Fdef");
    }

    #[test]
    fn test_encode_digest_path_segment_preserves_algorithm_separator() {
        assert_eq!(
            encode_digest_path_segment("sha256:abc/def"),
            "sha256:abc%2Fdef"
        );
    }

    #[test]
    fn test_valid_github_browser_download_url() {
        assert!(valid_github_browser_download_url(
            "https://github.com/jdx/mise-test-fixtures/releases/download/v1.0.0/hello-world.tar.gz",
            "jdx",
            "mise-test-fixtures",
            "v1.0.0"
        ));
        assert!(valid_github_browser_download_url(
            "https://github.com/jdx/mise-test-fixtures/releases/download/release%2F2026/hello-world.tar.gz",
            "jdx",
            "mise-test-fixtures",
            "release/2026"
        ));
        assert!(valid_github_browser_download_url(
            "https://github.com/Dicklesworthstone/destructive_command_guard/releases/download/v0.5.6/dcg-aarch64-apple-darwin.tar.xz",
            "Dicklesworthstone",
            "Destructive_command_guard",
            "v0.5.6"
        ));
        assert!(!valid_github_browser_download_url(
            "https://github.com/jdx/mise-test-fixtures/releases/download/v0.9.0/hello-world.tar.gz",
            "jdx",
            "mise-test-fixtures",
            "v1.0.0"
        ));
        assert!(!valid_github_browser_download_url(
            "https://evil.example.com/jdx/mise-test-fixtures/releases/download/v1.0.0/hello-world.tar.gz",
            "jdx",
            "mise-test-fixtures",
            "v1.0.0"
        ));
        assert!(!valid_github_browser_download_url(
            "https://github.com/other/mise-test-fixtures/releases/download/v1.0.0/hello-world.tar.gz",
            "jdx",
            "mise-test-fixtures",
            "v1.0.0"
        ));
        assert!(!valid_github_browser_download_url(
            "https://github.com/jdx/mise-test-fixtures/releases/download",
            "jdx",
            "mise-test-fixtures",
            "v1.0.0"
        ));
    }

    #[test]
    fn test_valid_github_asset_api_url() {
        assert!(valid_github_asset_api_url(
            "https://api.github.com/repos/jdx/mise-test-fixtures/releases/assets/1",
            "jdx",
            "mise-test-fixtures"
        ));
        assert!(valid_github_asset_api_url(
            "https://api.github.com/repos/Dicklesworthstone/destructive_command_guard/releases/assets/430632958",
            "Dicklesworthstone",
            "Destructive_command_guard"
        ));
        assert!(!valid_github_asset_api_url(
            "https://api.github.com/repos/other/mise-test-fixtures/releases/assets/1",
            "jdx",
            "mise-test-fixtures"
        ));
        assert!(!valid_github_asset_api_url(
            "https://github.com/jdx/mise-test-fixtures/releases/assets/1",
            "jdx",
            "mise-test-fixtures"
        ));
        assert!(!valid_github_asset_api_url(
            "https://api.github.com/repos/jdx/mise-test-fixtures/releases/assets/1/extra",
            "jdx",
            "mise-test-fixtures"
        ));
    }

    #[test]
    fn test_valid_github_release_asset_urls_rejects_empty_assets() {
        let release = GithubRelease {
            tag_name: "v1.0.0".into(),
            draft: false,
            prerelease: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            assets: vec![],
        };

        assert!(!valid_github_release_asset_urls(
            &release,
            "jdx",
            "mise-test-fixtures"
        ));
    }

    #[test]
    fn test_valid_github_release_tag() {
        let release = GithubRelease {
            tag_name: "v1.0.0".into(),
            draft: false,
            prerelease: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            assets: vec![],
        };

        assert!(valid_github_release_tag(&release, "v1.0.0"));
        assert!(valid_github_release_tag(&release, "latest"));
        assert!(!valid_github_release_tag(&release, "v2.0.0"));
    }

    #[test]
    fn test_versions_host_error_message_includes_body() {
        assert_eq!(
            versions_host_error_message(403, "GitHub repo is not in the mise registry\n"),
            "HTTP status client error (403 Forbidden): GitHub repo is not in the mise registry"
        );
    }

    #[test]
    fn test_versions_host_error_message_caps_body() {
        let body = "x".repeat(250);
        let message = versions_host_error_message(502, &body);
        assert_eq!(
            message.len(),
            "HTTP status server error (502 Bad Gateway): ".len() + 200
        );
    }

    #[test]
    fn test_versions_host_error_message_uses_generic_label_for_other_statuses() {
        assert_eq!(
            versions_host_error_message(302, ""),
            "HTTP status error (302 Found)"
        );
    }

    #[test]
    fn test_attestations_response_requires_attestations_field() {
        assert!(serde_json::from_str::<AttestationsResponse>("{}").is_err());
        assert!(
            serde_json::from_str::<AttestationsResponse>(r#"{"attestations":[]}"#)
                .unwrap()
                .attestations
                .is_empty()
        );
    }
}
