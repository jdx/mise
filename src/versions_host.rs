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
    /// Whether this name is a moving release channel. The versions host keeps
    /// this platform-independent signal, while platform-specific checksums are
    /// fetched directly from the backend when needed.
    #[serde(default)]
    rolling: bool,
    /// Pre-release flag, when the producing source can distinguish it. Absent
    /// in old host data — and for entries from sources that don't track
    /// prereleases — which maps to `None` ("unknown") without any schema
    /// upgrade. Old mise clients that don't know about this field ignore it
    /// (toml-rs accepts unknown fields by default), so populating it in
    /// mise-versions is forward-compatible.
    #[serde(default)]
    prerelease: Option<bool>,
}

/// The most release-list pages mise-versions serves.
pub(crate) const GITHUB_RELEASES_MAX_PAGES: usize = 10;

/// One page of a repository's releases from mise-versions.
#[derive(serde::Deserialize)]
pub(crate) struct GithubReleasesPage {
    /// GitHub's order, drafts removed.
    pub releases: Vec<GithubRelease>,
    /// The page to request next, or `None` when there is none to request
    /// here. Counted before drafts were removed, so a short page is not
    /// necessarily the last.
    pub next_page: Option<u32>,
    /// More releases exist than mise-versions serves; the rest are GitHub's.
    #[serde(default)]
    pub truncated: bool,
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
    page: Option<u32>,
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
            page: None,
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
            page: None,
            full: None,
            version: None,
        }
    }

    fn github_releases(repo: &'a str, page: u32) -> Self {
        Self {
            endpoint: "github_releases",
            tool: None,
            repo: Some(repo),
            tag: None,
            digest: None,
            page: Some(page),
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
            page: None,
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
            page: None,
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
        if let Some(page) = self.page {
            fields.push_str(&format!(" page={page}"));
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
pub(crate) async fn list_versions(tool: &str) -> eyre::Result<Option<Vec<VersionInfo>>> {
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
                    rolling: entry.rolling,
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
                // fallback=true: the sole caller
                // (`Backend::list_remote_versions_with_refresh`) logs this error
                // at debug and then lists versions from the backend's upstream
                // source anyway, so a failure here is not the end of the road.
                log_versions_host_warn(
                    ctx,
                    "failed",
                    &format!(
                        "status={status} fallback=true error={}",
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
pub(crate) async fn github_release(repo: &str, tag: &str) -> eyre::Result<Option<GithubRelease>> {
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
    let Some(mut release) = fetch_optional_json::<GithubRelease>(&url, ctx).await? else {
        return Ok(None);
    };
    if release.assets.is_empty() {
        log_versions_host_warn(ctx, "no_assets", "fallback=true");
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
    if let Err(asset) = pin_github_release_assets(&mut release, owner, repo_name) {
        log_versions_host_warn(
            ctx,
            "invalid_asset_url",
            &format!("asset={} fallback=true", log_value(&asset)),
        );
        return Ok(None);
    }
    log_versions_host_trace(ctx, "success", "");
    Ok(Some(release))
}

/// Fetch one page (100 releases) of a public repository's release list from
/// the versions host, shaped like GitHub's so the caller's filtering stays
/// authoritative.
pub(crate) async fn github_releases(
    repo: &str,
    page: u32,
) -> eyre::Result<Option<GithubReleasesPage>> {
    if !enabled_for_github_metadata() {
        return Ok(None);
    }

    let Some((owner, repo_name)) = split_github_repo(repo) else {
        return Ok(None);
    };
    let url = github_releases_url(owner, repo_name, page);
    let ctx = VersionsHostLogContext::github_releases(repo, page);
    let Some(mut list) = fetch_optional_json::<GithubReleasesPage>(&url, ctx).await? else {
        return Ok(None);
    };
    if list.next_page.is_some_and(|next| next != page + 1) {
        log_versions_host_warn(ctx, "invalid_next_page", "fallback=true");
        return Ok(None);
    }
    if let Err(tag) = pin_listed_releases(&mut list, owner, repo_name) {
        log_versions_host_warn(
            ctx,
            "invalid_release",
            &format!("tag={} fallback=true", log_value(&tag)),
        );
        return Ok(None);
    }
    log_versions_host_trace(ctx, "success", &format!("releases={}", list.releases.len()));
    Ok(Some(list))
}

fn github_releases_url(owner: &str, repo: &str, page: u32) -> String {
    format!(
        "https://mise-versions.jdx.dev/api/github/repos/{}/{}/releases?page={page}",
        encode_path_segment(owner),
        encode_path_segment(repo),
    )
}

/// Check and pin every listed release (see [`pin_github_release_assets`]).
/// Drafts are refused too: the mirror must never publish them. Returns the
/// tag of the first release that fails.
fn pin_listed_releases(
    list: &mut GithubReleasesPage,
    owner: &str,
    repo: &str,
) -> Result<(), String> {
    for release in &mut list.releases {
        if release.draft || pin_github_release_assets(release, owner, repo).is_err() {
            return Err(release.tag_name.clone());
        }
    }
    Ok(())
}

/// Fetch cached GitHub Artifact Attestation payloads by artifact digest.
///
/// The returned bundles are not trusted by virtue of coming from mise-versions;
/// callers still verify them cryptographically against the downloaded artifact.
pub(crate) async fn github_attestations(
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
                return match resp.json().await {
                    Ok(value) => Ok(Some(value)),
                    Err(err) => {
                        log_versions_host_warn(
                            ctx,
                            "invalid_response",
                            &format!("fallback=true error={}", log_value(&err.to_string())),
                        );
                        Ok(None)
                    }
                };
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

pub(crate) fn enabled_for_github_metadata() -> bool {
    let settings = Settings::get();
    !settings.prefer_offline() && settings.use_versions_host && !github_is_url_replaced()
}

/// Whether `url_replacements` sends GitHub somewhere else (a proxy, a mirror,
/// a test fixture). mise-versions answers for github.com itself, so it must
/// not stand in for whatever the user routed GitHub to.
fn github_is_url_replaced() -> bool {
    if Settings::get().url_replacements.is_none() {
        return false;
    }
    ["https://api.github.com/", "https://github.com/"]
        .iter()
        .any(|original| {
            let original = url::Url::parse(original).expect("valid GitHub URL");
            let mut replaced = original.clone();
            http::apply_url_replacements(&mut replaced);
            replaced != original
        })
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

/// Check a mirrored release's asset URLs and pin them to `owner/repo`.
///
/// mise-versions chose these URLs, so it must not get to choose what they
/// download. Each browser URL has to be a github.com download of this
/// release's tag and of this asset's name, and each API URL a GitHub release
/// asset. When either names a repository other than `owner/repo` (GitHub
/// reports the new name after a rename or transfer), it is rewritten to
/// `owner/repo`: GitHub redirects the old name itself, and the mirror can no
/// longer send a download anywhere but where GitHub sends `owner/repo`.
///
/// Returns the name of the first asset that fails.
fn pin_github_release_assets(
    release: &mut GithubRelease,
    owner: &str,
    repo: &str,
) -> Result<(), String> {
    for asset in &mut release.assets {
        let browser = pinned_browser_download_url(
            &asset.browser_download_url,
            owner,
            repo,
            &release.tag_name,
            &asset.name,
        );
        let api = pinned_asset_api_url(&asset.url, owner, repo);
        let (Some(browser), Some(api)) = (browser, api) else {
            return Err(asset.name.clone());
        };
        if browser != asset.browser_download_url {
            trace!(
                "mise-versions lists {} under another repository; pinning it to {owner}/{repo}",
                asset.browser_download_url
            );
        }
        asset.browser_download_url = browser;
        asset.url = api;
    }
    Ok(())
}

fn valid_github_release_tag(release: &GithubRelease, tag: &str) -> bool {
    tag == "latest" || release.tag_name == tag
}

/// `https://github.com/{o}/{r}/releases/download/{tag}/{asset}` with `{o}/{r}`
/// pinned to `owner/repo`, or `None` when the URL is not a download of
/// `asset` from `tag`.
fn pinned_browser_download_url(
    url: &str,
    owner: &str,
    repo: &str,
    tag: &str,
    asset: &str,
) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("github.com") {
        return None;
    }
    let segments: Vec<_> = parsed.path_segments()?.collect();
    let [o, r, "releases", "download", tag_segments @ .., file] = segments.as_slice() else {
        return None;
    };
    if o.is_empty()
        || r.is_empty()
        || !path_segments_match(tag_segments, tag)
        || !path_segment_matches(file, asset)
    {
        return None;
    }
    if github_repo_segment_matches(o, owner) && github_repo_segment_matches(r, repo) {
        return Some(url.to_string());
    }
    Some(format!(
        "https://github.com/{}/{}/{}",
        encode_path_segment(owner),
        encode_path_segment(repo),
        segments[2..].join("/")
    ))
}

/// `https://api.github.com/repos/{o}/{r}/releases/assets/{id}` with `{o}/{r}`
/// pinned to `owner/repo`, or `None` when the URL is not a release asset.
fn pinned_asset_api_url(url: &str, owner: &str, repo: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("api.github.com") {
        return None;
    }
    let segments: Vec<_> = parsed.path_segments()?.collect();
    let ["repos", o, r, "releases", "assets", id] = segments.as_slice() else {
        return None;
    };
    if o.is_empty() || r.is_empty() || id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if github_repo_segment_matches(o, owner) && github_repo_segment_matches(r, repo) {
        return Some(url.to_string());
    }
    Some(format!(
        "https://api.github.com/repos/{}/{}/releases/assets/{id}",
        encode_path_segment(owner),
        encode_path_segment(repo),
    ))
}

fn github_repo_segment_matches(segment: &str, expected: &str) -> bool {
    segment.eq_ignore_ascii_case(expected)
}

fn path_segment_matches(segment: &str, expected: &str) -> bool {
    segment == expected || urlencoding::decode(segment).is_ok_and(|decoded| decoded == expected)
}

fn path_segments_match(segments: &[&str], expected: &str) -> bool {
    !segments.is_empty() && path_segment_matches(&segments.join("/"), expected)
}

/// Tracks a tool installation asynchronously (fire-and-forget)
/// Tracks all core plugins and registry tools (including java/python)
pub(crate) fn track_install(tool: &str, full: &str, version: &str) {
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
    use crate::platform::{ARCH, OS};

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
    fn test_version_entry_deserializes_rolling_metadata() {
        let response: VersionsResponse = toml::from_str(
            r#"
[versions]
"1.0.0" = { created_at = 2026-01-01T00:00:00Z }
"nightly" = { created_at = 2026-01-02T00:00:00Z, rolling = true }
"#,
        )
        .unwrap();

        assert!(!response.versions["1.0.0"].rolling);
        assert!(response.versions["nightly"].rolling);
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
    fn test_github_releases_url_encodes_repo() {
        assert_eq!(
            github_releases_url("jdx", "mise.test", 3),
            "https://mise-versions.jdx.dev/api/github/repos/jdx/mise.test/releases?page=3"
        );
    }

    #[test]
    fn test_github_releases_page_deserializes() {
        let page: GithubReleasesPage = serde_json::from_str(
            r#"{"releases":[{"tag_name":"v1.0.0","draft":false,"prerelease":false,
                "created_at":"2026-01-01T00:00:00Z","published_at":"2026-01-02T00:00:00Z",
                "assets":[]}],"next_page":null,"truncated":false}"#,
        )
        .unwrap();
        assert_eq!(page.releases.len(), 1);
        assert_eq!(
            page.releases[0].published_at.as_deref(),
            Some("2026-01-02T00:00:00Z")
        );
        assert_eq!(page.next_page, None);
    }

    #[test]
    fn test_pinned_browser_download_url() {
        let pin = |url: &str, tag: &str, asset: &str| {
            pinned_browser_download_url(url, "jdx", "mise-test-fixtures", tag, asset)
        };
        let fixture =
            "https://github.com/jdx/mise-test-fixtures/releases/download/v1.0.0/hello-world.tar.gz";
        assert_eq!(
            pin(fixture, "v1.0.0", "hello-world.tar.gz").as_deref(),
            Some(fixture)
        );
        // Case differences are the same repository; the URL is kept as is.
        let upper =
            "https://github.com/JDX/Mise-Test-Fixtures/releases/download/v1.0.0/hello-world.tar.gz";
        assert_eq!(
            pin(upper, "v1.0.0", "hello-world.tar.gz").as_deref(),
            Some(upper)
        );
        // Tags and asset names are compared decoded.
        let encoded = "https://github.com/jdx/mise-test-fixtures/releases/download/release%2F2026/hello%20world.tar.gz";
        assert_eq!(
            pin(encoded, "release/2026", "hello world.tar.gz").as_deref(),
            Some(encoded)
        );
        assert!(
            pin(
                "https://github.com/jdx/mise-test-fixtures/releases/download/%40biomejs/biome%402.5.2/biome-linux-x64",
                "@biomejs/biome@2.5.2",
                "biome-linux-x64"
            )
            .is_some()
        );

        // Another repository (GitHub reports a renamed repository's new
        // name) is pinned back to the requested one.
        assert_eq!(
            pin(
                "https://github.com/new-owner/new-name/releases/download/v1.0.0/hello-world.tar.gz",
                "v1.0.0",
                "hello-world.tar.gz"
            )
            .as_deref(),
            Some(fixture)
        );

        // Another tag, another file, another host, or not a download at all.
        assert!(
            pin(
                "https://github.com/jdx/mise-test-fixtures/releases/download/v0.9.0/hello-world.tar.gz",
                "v1.0.0",
                "hello-world.tar.gz"
            )
            .is_none()
        );
        assert!(pin(fixture, "v1.0.0", "hello-world-windows.zip").is_none());
        assert!(
            pin(
                "https://evil.example.com/jdx/mise-test-fixtures/releases/download/v1.0.0/hello-world.tar.gz",
                "v1.0.0",
                "hello-world.tar.gz"
            )
            .is_none()
        );
        assert!(
            pin(
                "http://github.com/jdx/mise-test-fixtures/releases/download/v1.0.0/hello-world.tar.gz",
                "v1.0.0",
                "hello-world.tar.gz"
            )
            .is_none()
        );
        assert!(
            pin(
                "https://github.com/jdx/mise-test-fixtures/releases/download",
                "v1.0.0",
                "download"
            )
            .is_none()
        );
        assert!(
            pin(
                "https://github.com/jdx/mise-test-fixtures/archive/v1.0.0/hello-world.tar.gz",
                "v1.0.0",
                "hello-world.tar.gz"
            )
            .is_none()
        );
    }

    #[test]
    fn test_pinned_asset_api_url() {
        let pin = |url: &str| pinned_asset_api_url(url, "jdx", "mise-test-fixtures");
        let fixture = "https://api.github.com/repos/jdx/mise-test-fixtures/releases/assets/1";
        assert_eq!(pin(fixture).as_deref(), Some(fixture));
        assert_eq!(
            pin("https://api.github.com/repos/other/renamed/releases/assets/1").as_deref(),
            Some(fixture)
        );
        assert!(pin("https://github.com/jdx/mise-test-fixtures/releases/assets/1").is_none());
        assert!(
            pin("https://api.github.com/repos/jdx/mise-test-fixtures/releases/assets/1/extra")
                .is_none()
        );
        assert!(
            pin("https://api.github.com/repos/jdx/mise-test-fixtures/releases/assets/x").is_none()
        );
        assert!(pin("https://api.github.com/repos/jdx/mise-test-fixtures/git/blobs/1").is_none());
    }

    #[test]
    fn test_pin_listed_releases() {
        let release = |tag: &str, draft: bool, asset_owner: &str, file: &str| GithubRelease {
            tag_name: tag.into(),
            draft,
            prerelease: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            published_at: None,
            assets: vec![crate::github::GithubAsset {
                name: "tool.tar.gz".into(),
                browser_download_url: format!(
                    "https://github.com/{asset_owner}/mise/releases/download/{tag}/{file}"
                ),
                url: format!("https://api.github.com/repos/{asset_owner}/mise/releases/assets/1"),
                digest: None,
                updated_at: None,
            }],
        };
        let page = |releases| GithubReleasesPage {
            releases,
            next_page: None,
            truncated: false,
        };

        let mut ok = page(vec![
            release("v1.0.0", false, "jdx", "tool.tar.gz"),
            GithubRelease {
                assets: vec![],
                ..release("v0.1.0", false, "jdx", "tool.tar.gz")
            },
            release("v0.9.0", false, "renamed-from", "tool.tar.gz"),
        ]);
        assert_eq!(pin_listed_releases(&mut ok, "jdx", "mise"), Ok(()));
        assert_eq!(
            ok.releases[2].assets[0].browser_download_url,
            "https://github.com/jdx/mise/releases/download/v0.9.0/tool.tar.gz"
        );
        assert_eq!(
            ok.releases[2].assets[0].url,
            "https://api.github.com/repos/jdx/mise/releases/assets/1"
        );

        let mut draft = page(vec![release("v1.0.0", true, "jdx", "tool.tar.gz")]);
        assert_eq!(
            pin_listed_releases(&mut draft, "jdx", "mise"),
            Err("v1.0.0".to_string())
        );

        // The listed name must be the file the URL downloads.
        let mut swapped = page(vec![release("v1.0.0", false, "jdx", "other.tar.gz")]);
        assert_eq!(
            pin_listed_releases(&mut swapped, "jdx", "mise"),
            Err("v1.0.0".to_string())
        );
    }

    #[test]
    fn test_valid_github_release_tag() {
        let release = GithubRelease {
            tag_name: "v1.0.0".into(),
            draft: false,
            prerelease: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            published_at: None,
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
