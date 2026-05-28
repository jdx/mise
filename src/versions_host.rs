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

/// List versions from the versions host (mise-versions.jdx.dev).
/// Returns Vec<VersionInfo> with created_at timestamps from the TOML endpoint.
pub async fn list_versions(tool: &str) -> eyre::Result<Option<Vec<VersionInfo>>> {
    let settings = Settings::get();
    if settings.prefer_offline()
        || !settings.use_versions_host
        || !PLUGINS_USE_VERSION_HOST.contains(tool)
    {
        return Ok(None);
    }

    static CACHE: LazyLock<Mutex<HashMap<String, Vec<VersionInfo>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    static RATE_LIMITED: AtomicBool = AtomicBool::new(false);

    if let Some(versions) = CACHE.lock().await.get(tool) {
        return Ok(Some(versions.clone()));
    }
    if RATE_LIMITED.load(Ordering::Relaxed) {
        warn!("{tool}: skipping versions host check due to rate limit");
        return Ok(None);
    }

    // Use TOML format which includes created_at timestamps
    let url = format!("https://mise-versions.jdx.dev/tools/{}.toml", tool);
    let versions: Vec<VersionInfo> = match HTTP_FETCH
        .get_text_with_headers(&url, &VERSIONS_HOST_HEADERS)
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
            404 => return Ok(None),
            429 => {
                RATE_LIMITED.store(true, Ordering::Relaxed);
                warn!("{tool}: mise-versions rate limited");
                return Ok(None);
            }
            _ => return Err(err),
        },
    };

    trace!(
        "got {} {} versions from versions host",
        versions.len(),
        tool
    );

    if versions.is_empty() {
        return Ok(None);
    }

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

    let Some(release) = fetch_optional_json(&url, "GitHub release metadata").await? else {
        return Ok(None);
    };
    if !valid_github_release_asset_urls(&release, owner, repo_name) {
        warn!("mise-versions returned invalid GitHub release asset URLs for {repo}@{tag}");
        return Ok(None);
    }
    if !valid_github_release_tag(&release, tag) {
        warn!(
            "mise-versions returned GitHub release tag {} for requested {repo}@{tag}",
            release.tag_name
        );
        return Ok(None);
    }
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

    let response: Option<AttestationsResponse> =
        fetch_optional_json(&url, "GitHub attestations").await?;
    Ok(response.map(|r| r.attestations))
}

async fn fetch_optional_json<T>(url: &str, label: &str) -> eyre::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    match HTTP_FETCH
        .json_with_headers(url, &VERSIONS_HOST_HEADERS)
        .await
    {
        Ok(value) => Ok(Some(value)),
        Err(err) => match http::error_code(&err).unwrap_or(0) {
            404 => Ok(None),
            429 => {
                debug!("mise-versions rate limited while fetching {label}");
                Ok(None)
            }
            _ => {
                debug!("mise-versions {label} lookup failed: {err:#}");
                Ok(None)
            }
        },
    }
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
            valid_github_browser_download_url(&asset.browser_download_url, owner, repo)
                && valid_github_asset_api_url(&asset.url, owner, repo)
        })
}

fn valid_github_release_tag(release: &GithubRelease, tag: &str) -> bool {
    tag == "latest" || release.tag_name == tag
}

fn valid_github_browser_download_url(url: &str, owner: &str, repo: &str) -> bool {
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
        (Some(o), Some(r), Some("releases"), Some("download"), Some(_tag), Some(_asset), None)
            if o == owner && r == repo
    )
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
            if o == owner && r == repo
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
        Ok(true) => trace!("Tracked install: {full}@{version}"),
        Ok(false) => trace!("Track request failed"),
        Err(e) => trace!("Track request error: {e}"),
    }

    Ok(())
}

fn track_install_url(tool: &str) -> String {
    format!(
        "https://mise-versions.jdx.dev/api/tools/{}",
        urlencoding::encode(tool)
    )
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
            "mise-test-fixtures"
        ));
        assert!(!valid_github_browser_download_url(
            "https://evil.example.com/jdx/mise-test-fixtures/releases/download/v1.0.0/hello-world.tar.gz",
            "jdx",
            "mise-test-fixtures"
        ));
        assert!(!valid_github_browser_download_url(
            "https://github.com/other/mise-test-fixtures/releases/download/v1.0.0/hello-world.tar.gz",
            "jdx",
            "mise-test-fixtures"
        ));
        assert!(!valid_github_browser_download_url(
            "https://github.com/jdx/mise-test-fixtures/releases/download",
            "jdx",
            "mise-test-fixtures"
        ));
    }

    #[test]
    fn test_valid_github_asset_api_url() {
        assert!(valid_github_asset_api_url(
            "https://api.github.com/repos/jdx/mise-test-fixtures/releases/assets/1",
            "jdx",
            "mise-test-fixtures"
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
