//! The mise-versions host: GitHub release and attestation mirrors.

use crate::github::GithubRelease;
use crate::http;
use crate::http::HTTP_FETCH;
use eyre::bail;
use mise_settings::Settings;
use mise_sigstore::Attestation;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use std::{
    collections::HashSet,
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

/// Headers for every request to mise-versions. In CI they add `x-mise-ci: true`
/// and a trailing ` ci` token on the User-Agent, which the host's request logs
/// record so CI traffic can be left out of usage counts.
pub static VERSIONS_HOST_HEADERS: LazyLock<HeaderMap> =
    LazyLock::new(|| versions_host_headers(ci_info::is_ci(), crate::user_agent::get()));

fn versions_host_headers(ci: bool, user_agent: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if ci {
        headers.insert("x-mise-ci", HeaderValue::from_static("true"));
        if let Ok(user_agent) = HeaderValue::from_str(&format!("{user_agent} ci")) {
            headers.insert(USER_AGENT, user_agent);
        }
    }
    headers
}

/// The most release-list pages mise-versions serves.
pub const GITHUB_RELEASES_MAX_PAGES: usize = 10;

/// One page of a repository's releases from mise-versions.
#[derive(serde::Deserialize)]
pub struct GithubReleasesPage {
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

/// A public repository's whole release or tag list from mise-versions, in
/// GitHub's REST order (format 1 of jdx/mise-versions'
/// `docs/github-versions.md`). Entries carry no assets.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GithubVersionsList {
    /// `releases` or `tags`.
    pub source: String,
    /// When mise-versions last read page 1 of the list from GitHub.
    pub fetched_at: Option<String>,
    #[serde(default)]
    pub releases: Vec<GithubListedRelease>,
    #[serde(default)]
    pub tags: Vec<GithubListedTag>,
    /// The list is a newest-first prefix; older entries exist.
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GithubListedRelease {
    pub tag: String,
    /// `published_at`, or `created_at` without one.
    pub date: Option<String>,
    pub prerelease: bool,
    pub asset_count: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GithubListedTag {
    pub tag: String,
    /// The tagged commit's committer date.
    pub date: Option<String>,
}

/// Which whole list to read with [`github_versions_list`].
#[derive(Debug, Clone, Copy)]
pub enum GithubList<'a> {
    Releases,
    Tags,
    /// The releases carrying an asset with exactly this name.
    ReleasesWithAsset(&'a str),
}

/// One page (100 tags) of a repository's tags from mise-versions.
#[derive(serde::Deserialize)]
pub struct GithubTagsPage {
    /// GitHub's order: by name, highest version first.
    pub tags: Vec<GithubMirroredTag>,
    pub next_page: Option<u32>,
    /// More tags exist than mise-versions serves.
    #[serde(default)]
    pub truncated: bool,
}

#[derive(serde::Deserialize)]
pub struct GithubMirroredTag {
    pub name: String,
    /// The tagged commit's committer date, sent only when asked for.
    #[serde(default)]
    pub date: Option<String>,
}

/// One entry of a repository directory, as GitHub's contents API lists it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GithubContentsEntry {
    pub name: String,
    /// `file`, `dir`, `symlink` or `submodule`.
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(serde::Deserialize)]
struct AttestationsResponse {
    attestations: Vec<Attestation>,
}

#[derive(Clone, Copy)]
pub struct VersionsHostLogContext<'a> {
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
    pub fn version_list(tool: &'a str) -> Self {
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

    fn github_tags(repo: &'a str, page: u32) -> Self {
        Self {
            endpoint: "github_tags",
            tool: None,
            repo: Some(repo),
            tag: None,
            digest: None,
            page: Some(page),
            full: None,
            version: None,
        }
    }

    fn github_repo(endpoint: &'static str, repo: &'a str) -> Self {
        Self {
            endpoint,
            tool: None,
            repo: Some(repo),
            tag: None,
            digest: None,
            page: None,
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

    pub fn install_track(tool: &'a str, full: &'a str, version: &'a str) -> Self {
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

pub fn log_value(value: &str) -> String {
    if value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/' | b':' | b'@'))
    {
        value.to_string()
    } else {
        format!("{:?}", value)
    }
}

/// `full` for logs, with any URL credentials, query, and fragment removed
/// (see [`sanitize_full`]).
pub fn sanitize_full_for_log(full: &str) -> String {
    sanitize_full(full).unwrap_or_else(|| "[redacted]".to_string())
}

/// `full` without URL credentials, query, or fragment, for logs and request
/// URLs. The URL may follow a backend prefix (`http:https://…`) or be the
/// whole value (`https://…`). `None` when `full` holds a `://` URL that cannot
/// be parsed, so it cannot be cleaned and must not be sent.
pub fn sanitize_full(full: &str) -> Option<String> {
    fn strip(mut url: url::Url) -> url::Url {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        url
    }
    let with_host = |s: &str| url::Url::parse(s).ok().filter(|url| url.has_host());
    if let Some((backend, value)) = full.split_once(':') {
        // `full` is itself a URL: its scheme looks like a backend prefix.
        if value.starts_with("//") {
            return with_host(full).map(|url| strip(url).to_string());
        }
        if let Some(url) = with_host(value) {
            return Some(format!("{backend}:{}", strip(url)));
        }
    }
    if full.contains("://") {
        return None;
    }
    Some(full.to_string())
}

pub fn log_versions_host_trace(ctx: VersionsHostLogContext<'_>, outcome: &str, extra: &str) {
    if extra.is_empty() {
        trace!("mise-versions {} outcome={outcome}", ctx.fields());
    } else {
        trace!("mise-versions {} outcome={outcome} {extra}", ctx.fields());
    }
}

pub fn log_versions_host_warn(ctx: VersionsHostLogContext<'_>, outcome: &str, extra: &str) {
    if extra.is_empty() {
        warn!("mise-versions {} outcome={outcome}", ctx.fields());
    } else {
        warn!("mise-versions {} outcome={outcome} {extra}", ctx.fields());
    }
}

/// Fetch cached public GitHub release metadata from the versions host.
///
/// This endpoint is intentionally shaped like GitHub's release object so the
/// normal backend asset-selection code remains authoritative on the client.
///
/// `Ok(None)` when the mirror does not apply to `repo`, or answered 404 (a
/// private or missing repository, or no such tag). A failed request or an
/// answer that does not check out is an error: the caller must not take it
/// as "not found". A release without assets is returned as it is; GitHub
/// returns those too (kubernetes and terraform publish binaries elsewhere).
pub async fn github_release(repo: &str, tag: &str) -> eyre::Result<Option<GithubRelease>> {
    if !enabled_for_github_repo(repo) || github_release_is_url_replaced(repo, tag) {
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
    let Some(mut release) = fetch_json_unless_not_found::<GithubRelease>(&url, ctx).await? else {
        return Ok(None);
    };
    if !valid_github_release_tag(&release, tag) {
        log_versions_host_trace(
            ctx,
            "tag_mismatch",
            &format!("returned_tag={}", log_value(&release.tag_name)),
        );
        bail!(
            "mise-versions answered for {repo}@{tag} with release {}",
            release.tag_name
        );
    }
    if let Err(asset) = pin_github_release_assets(&mut release, owner, repo_name) {
        log_versions_host_trace(
            ctx,
            "invalid_asset_url",
            &format!("asset={}", log_value(&asset)),
        );
        bail!("mise-versions listed asset {asset} of {repo}@{tag} at an unexpected URL");
    }
    log_versions_host_trace(ctx, "success", "");
    Ok(Some(release))
}

/// Fetch one page (100 releases) of a public repository's release list from
/// the versions host, shaped like GitHub's so the caller's filtering stays
/// authoritative. `Ok(None)` and errors as in [`github_release`].
pub async fn github_releases(repo: &str, page: u32) -> eyre::Result<Option<GithubReleasesPage>> {
    if !enabled_for_github_repo(repo) {
        return Ok(None);
    }

    let Some((owner, repo_name)) = split_github_repo(repo) else {
        return Ok(None);
    };
    let url = github_releases_url(owner, repo_name, page);
    let ctx = VersionsHostLogContext::github_releases(repo, page);
    let Some(mut list) = fetch_json_unless_not_found::<GithubReleasesPage>(&url, ctx).await? else {
        return Ok(None);
    };
    if list.next_page.is_some_and(|next| next != page + 1) {
        log_versions_host_trace(ctx, "invalid_next_page", "");
        bail!("mise-versions pointed page {page} of {repo}'s releases at an unexpected next page");
    }
    if let Err(tag) = pin_listed_releases(&mut list, owner, repo_name) {
        log_versions_host_trace(ctx, "invalid_release", &format!("tag={}", log_value(&tag)));
        bail!("mise-versions listed release {tag} of {repo} with an unexpected asset URL");
    }
    log_versions_host_trace(ctx, "success", &format!("releases={}", list.releases.len()));
    Ok(Some(list))
}

/// Fetch a public repository's whole release or tag list from the versions
/// host. These are static files baked at deploy time, so most requests never
/// reach its Worker. `Ok(None)` and errors as in [`github_release`]; also
/// `Ok(None)` for a name the list paths can't spell.
pub async fn github_versions_list(
    repo: &str,
    list: GithubList<'_>,
) -> eyre::Result<Option<GithubVersionsList>> {
    if !enabled_for_github_repo(repo) {
        return Ok(None);
    }
    let Some(url) = github_versions_list_url(repo, list) else {
        return Ok(None);
    };
    let ctx = VersionsHostLogContext::github_repo("github_versions_list", repo);
    let Some(found) = fetch_json_unless_not_found::<GithubVersionsList>(&url, ctx).await? else {
        return Ok(None);
    };
    let expected = match list {
        GithubList::Tags => "tags",
        GithubList::Releases | GithubList::ReleasesWithAsset(_) => "releases",
    };
    if found.source != expected {
        bail!(
            "mise-versions answered {url} with a list of {}",
            found.source
        );
    }
    log_versions_host_trace(
        ctx,
        "success",
        &format!(
            "entries={} truncated={}",
            found.releases.len() + found.tags.len(),
            found.truncated
        ),
    );
    Ok(Some(found))
}

/// `/data/github/{owner}/{repo}/…`, lowercase as the files are baked, or
/// `None` for a name or asset those paths don't allow.
fn github_versions_list_url(repo: &str, list: GithubList<'_>) -> Option<String> {
    let (owner, name) = split_github_repo(repo)?;
    let part = |s: &str| {
        let s = s.to_ascii_lowercase();
        let valid = !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        valid.then_some(s)
    };
    let (owner, name) = (part(owner)?, part(name)?);
    let file = match list {
        GithubList::Releases => "releases.json".to_string(),
        GithubList::Tags => "tags.json".to_string(),
        GithubList::ReleasesWithAsset(asset) => {
            let valid = (1..=255).contains(&asset.len())
                && asset.chars().all(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '@' | '~' | '-')
                });
            if !valid {
                return None;
            }
            format!("assets/{asset}/releases.json")
        }
    };
    Some(format!(
        "https://mise-versions.jdx.dev/data/github/{owner}/{name}/{file}"
    ))
}

/// Fetch one page of a public repository's tags from the versions host, with
/// each tagged commit's date when `dates` is set. `Ok(None)` and errors as in
/// [`github_release`].
pub async fn github_tags(
    repo: &str,
    page: u32,
    dates: bool,
) -> eyre::Result<Option<GithubTagsPage>> {
    if !enabled_for_github_repo(repo) {
        return Ok(None);
    }
    let Some((owner, repo_name)) = split_github_repo(repo) else {
        return Ok(None);
    };
    let url = github_tags_url(owner, repo_name, page, dates);
    let ctx = VersionsHostLogContext::github_tags(repo, page);
    let Some(list) = fetch_json_unless_not_found::<GithubTagsPage>(&url, ctx).await? else {
        return Ok(None);
    };
    if list.next_page.is_some_and(|next| next != page + 1) {
        log_versions_host_trace(ctx, "invalid_next_page", "");
        bail!("mise-versions pointed page {page} of {repo}'s tags at an unexpected next page");
    }
    log_versions_host_trace(ctx, "success", &format!("tags={}", list.tags.len()));
    Ok(Some(list))
}

fn github_tags_url(owner: &str, repo: &str, page: u32, dates: bool) -> String {
    format!(
        "https://mise-versions.jdx.dev/api/github/repos/{}/{}/tags?page={page}{}",
        encode_path_segment(owner),
        encode_path_segment(repo),
        if dates { "&dates=1" } else { "" }
    )
}

/// List the directory `path` of a public repository at `commit`, a full
/// commit SHA, from the versions host. `Ok(None)` and errors as in
/// [`github_release`]; a `path` naming a file is a 404.
pub async fn github_contents(
    repo: &str,
    commit: &str,
    path: &str,
) -> eyre::Result<Option<Vec<GithubContentsEntry>>> {
    if !enabled_for_github_repo(repo) {
        return Ok(None);
    }
    let Some((owner, repo_name)) = split_github_repo(repo) else {
        return Ok(None);
    };
    let url = github_contents_url(owner, repo_name, commit, path);
    let ctx = VersionsHostLogContext::github_repo("github_contents", repo);
    let entries = fetch_json_unless_not_found(&url, ctx).await?;
    if let Some(entries) = &entries {
        let entries: &Vec<GithubContentsEntry> = entries;
        log_versions_host_trace(ctx, "success", &format!("entries={}", entries.len()));
    }
    Ok(entries)
}

fn github_contents_url(owner: &str, repo: &str, commit: &str, path: &str) -> String {
    let path = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(encode_path_segment)
        .collect::<Vec<_>>()
        .join("/");
    format!(
        "https://mise-versions.jdx.dev/api/github/repos/{}/{}/contents/{path}?ref={}",
        encode_path_segment(owner),
        encode_path_segment(repo),
        encode_path_segment(commit),
    )
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
/// `Ok(None)` and errors as in [`github_release`].
pub async fn github_attestations(
    repo: &str,
    digest: &str,
) -> eyre::Result<Option<Vec<Attestation>>> {
    if !enabled_for_github_repo(repo) {
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
    let response: Option<AttestationsResponse> = fetch_json_unless_not_found(&url, ctx).await?;
    if let Some(response) = &response {
        log_versions_host_trace(
            ctx,
            "success",
            &format!("attestations={}", response.attestations.len()),
        );
    }
    Ok(response.map(|r| r.attestations))
}

/// GET `url` from the versions host for data mise would otherwise ask
/// api.github.com for. `Ok(None)` is a 404 and nothing else; every other
/// failure is an error, so that it is never mistaken for "not found". Only
/// the errors [`retries_on_github`] accepts send the caller to GitHub.
async fn fetch_json_unless_not_found<T>(
    url: &str,
    ctx: VersionsHostLogContext<'_>,
) -> eyre::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    if crate::network::offline(&Settings::get()) {
        bail!("offline mode is enabled");
    }
    debug!("GET {url}");
    let resp = match HTTP_FETCH
        .get_async_with_headers_allow_error_status(url, &VERSIONS_HOST_HEADERS)
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            let status = transport_error_status(&err);
            log_versions_host_trace(
                ctx,
                "failed",
                &format!(
                    "status={} error={}",
                    status.unwrap_or(0),
                    log_value(&err.to_string())
                ),
            );
            return Err(VersionsHostUnavailable {
                status,
                message: format!("{err:#}"),
            }
            .into());
        }
    };
    let status = resp.status();
    debug!("GET {url} {status}");
    if status == reqwest::StatusCode::NOT_FOUND {
        log_versions_host_trace(ctx, "not_found", "status=404");
        return Ok(None);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let message = versions_host_error_message(status.as_u16(), &body);
        log_versions_host_trace(
            ctx,
            "failed",
            &format!("status={} error={}", status.as_u16(), log_value(&message)),
        );
        return Err(VersionsHostUnavailable {
            status: Some(status.as_u16()),
            message,
        }
        .into());
    }
    // The body is read first so that a connection lost while reading it is
    // mise-versions being unavailable, like one lost before the status;
    // only a body that arrived and doesn't parse is a bad answer.
    let body = resp.bytes().await.map_err(|err| VersionsHostUnavailable {
        status: None,
        message: format!("reading {url}: {err:#}"),
    })?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|err| eyre::eyre!("invalid response from {url}: {err}"))
}

/// The HTTP status of a request that failed without a response mise could
/// read, which is only ever one `reqwest` reports. Not [`http::error_code`]:
/// a timeout's message holds the URL, and a URL holding `404` (a tag such as
/// `v1.404.0`) would pass the timeout off as a 404, which isn't retried on
/// GitHub.
fn transport_error_status(err: &eyre::Report) -> Option<u16> {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<reqwest::Error>())
        .find_map(reqwest::Error::status)
        .map(|status| status.as_u16())
}

/// The versions host could not answer: a network failure or an error status
/// other than 404. In jdx/mise-versions' client contract a 403 means the
/// mirror is disabled or restricted, and anything else (429, 5xx, no
/// connection) is temporary: a copy cached earlier may stand in for the
/// answer. A timeout, a lost connection, a 429 or a 5xx also sends a GitHub
/// lookup to api.github.com (see [`retries_on_github`]).
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct VersionsHostUnavailable {
    pub status: Option<u16>,
    pub(crate) message: String,
}

impl VersionsHostUnavailable {
    /// Whether this failure is temporary, so a stale cached copy may be used.
    pub fn is_temporary(&self) -> bool {
        self.status != Some(403)
    }
}

/// Whether `err` is, or was caused by, a temporary [`VersionsHostUnavailable`],
/// or is a [`GithubRetryFailed`]: mise-versions failed that way and so did
/// api.github.com after it.
pub fn is_temporarily_unavailable(err: &eyre::Report) -> bool {
    err.chain().any(|cause| {
        cause.is::<GithubRetryFailed>()
            || cause
                .downcast_ref::<VersionsHostUnavailable>()
                .is_some_and(VersionsHostUnavailable::is_temporary)
    })
}

/// Whether a failed lookup of GitHub metadata on mise-versions is asked of
/// api.github.com instead: mise-versions timed out, lost the connection, or
/// answered 429 or 5xx. Anything it answered is trusted, including a 404
/// (a missing release, tag or repository), an empty list, and a 403 (the
/// mirror is disabled or restricted), so normal usage doesn't spend GitHub's
/// rate limit of 60 requests an hour without a token. An answer that doesn't
/// check out (a wrong tag, an asset URL off github.com) is an error too, and
/// not one GitHub is asked to paper over.
pub fn retries_on_github(err: &eyre::Report) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<VersionsHostUnavailable>()
            .is_some_and(|e| match e.status {
                None => true,
                Some(status) => status == 429 || (500..=599).contains(&status),
            })
    })
}

/// A GitHub lookup failed on mise-versions (see [`retries_on_github`]) and
/// again on api.github.com. Its source is GitHub's failure, so the
/// rate-limit hints that look for an api.github.com 403 still find it.
#[derive(Debug, thiserror::Error)]
#[error(
    "failed to read {what} from mise-versions.jdx.dev ({mirror}), and from api.github.com after it"
)]
pub struct GithubRetryFailed {
    what: String,
    mirror: String,
    #[source]
    github: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl GithubRetryFailed {
    pub fn new(what: &str, mirror: &eyre::Report, github: eyre::Report) -> Self {
        Self {
            what: what.to_string(),
            mirror: format!("{mirror:#}"),
            github: github.into(),
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

/// Whether mise-versions answers for api.github.com's release and
/// attestation endpoints.
///
/// Not gated on `prefer_offline`: those commands (`exec`, `x`, shims,
/// `hook-env`) still install missing tools, and the release metadata they need
/// then comes from GitHub when it doesn't come from here. Prefer-offline
/// already bounds each lookup to one short attempt, and `offline` stops it in
/// [`fetch_optional_json`].
pub fn enabled_for_github_metadata() -> bool {
    let settings = Settings::get();
    settings.use_versions_host && !github_is_url_replaced(None)
}

/// [`enabled_for_github_metadata`] for one repository (`owner/repo`), which
/// also catches `url_replacements` rules that only match its paths.
pub fn enabled_for_github_repo(repo: &str) -> bool {
    enabled_for_github_metadata() && !github_is_url_replaced(Some(repo))
}

/// Whether `url_replacements` sends GitHub's API, or this repository's
/// release and attestation endpoints, somewhere else (a proxy, a mirror, a
/// test fixture). mise-versions answers for api.github.com itself, so when
/// that is rerouted the replacement is the only source of GitHub metadata.
/// Rules that only reroute downloads (github.com release assets) don't
/// count: mise-versions' download URLs go through them like GitHub's do.
pub fn github_is_url_replaced(repo: Option<&str>) -> bool {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if Settings::get().url_replacements.is_none() {
        return false;
    }
    let mut urls = vec!["https://api.github.com/".to_string()];
    if let Some(repo) = repo {
        urls.extend([
            format!("https://api.github.com/repos/{repo}/releases"),
            // The listing request as mise makes it, for rules written with
            // its query.
            format!("https://api.github.com/repos/{repo}/releases?per_page=100"),
            format!("https://api.github.com/repos/{repo}/releases/latest"),
            format!("https://api.github.com/repos/{repo}/releases/tags/v1.0.0"),
            format!("https://api.github.com/repos/{repo}/attestations/sha256:0"),
        ]);
    }
    let replaced = urls.iter().any(|original| {
        let Ok(original) = url::Url::parse(original) else {
            return false;
        };
        let mut replaced = original.clone();
        http::apply_url_replacements(&mut replaced);
        replaced != original
    });
    if replaced && !LOGGED.swap(true, Ordering::Relaxed) {
        debug!("url_replacements reroutes GitHub; not using mise-versions for GitHub metadata");
    }
    replaced
}

/// Whether `url_replacements` reroutes `url`, the api.github.com request mise
/// would make without this host. A rule can name one endpoint (a repository's
/// tags, say) without touching any URL [`github_is_url_replaced`] tries; the
/// replacement is then the source for that endpoint.
pub fn github_api_url_is_replaced(url: &str) -> bool {
    if Settings::get().url_replacements.is_none() {
        return false;
    }
    let Ok(original) = url::Url::parse(url) else {
        return false;
    };
    let mut replaced = original.clone();
    http::apply_url_replacements(&mut replaced);
    replaced != original
}

/// Whether `url_replacements` reroutes the GitHub API URL for this exact
/// release, which a rule written for one tag can do without touching the
/// URLs [`github_is_url_replaced`] tries.
pub fn github_release_is_url_replaced(repo: &str, tag: &str) -> bool {
    if Settings::get().url_replacements.is_none() {
        return false;
    }
    let path = if tag == "latest" {
        "latest".to_string()
    } else {
        format!("tags/{}", encode_path_segment(tag))
    };
    let Ok(original) = url::Url::parse(&format!(
        "https://api.github.com/repos/{repo}/releases/{path}"
    )) else {
        return false;
    };
    let mut replaced = original.clone();
    http::apply_url_replacements(&mut replaced);
    replaced != original
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
        asset.from_versions_host = true;
        remember_mirrored_asset_api_url(&asset.url);
    }
    Ok(())
}

/// API asset URLs whose asset ID mise-versions chose. Only these need their
/// identity confirmed before use (see `github::checked_api_asset_url`);
/// GitHub's own release data, such as for private repos, is taken as given.
/// Filled when mise-versions data is pinned, and again from each
/// `GithubAsset::from_versions_host` when a release comes out of a cache.
static MIRRORED_ASSET_API_URLS: LazyLock<std::sync::Mutex<HashSet<String>>> =
    LazyLock::new(Default::default);

pub fn remember_mirrored_asset_api_url(url: &str) {
    MIRRORED_ASSET_API_URLS
        .lock()
        .unwrap()
        .insert(url.to_string());
}

pub fn is_mirrored_asset_api_url(url: &str) -> bool {
    MIRRORED_ASSET_API_URLS.lock().unwrap().contains(url)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_host_headers_mark_ci() {
        let headers = versions_host_headers(true, "mise/2026.10.10 linux-x64 (2026-10-11) bash");
        assert_eq!(headers["x-mise-ci"], "true");
        assert_eq!(
            headers[USER_AGENT],
            "mise/2026.10.10 linux-x64 (2026-10-11) bash ci"
        );
    }

    #[test]
    fn test_versions_host_headers_outside_ci_are_empty() {
        assert!(versions_host_headers(false, "mise/2026.10.10").is_empty());
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
    fn test_sanitize_full() {
        assert_eq!(
            sanitize_full("aqua:BurntSushi/ripgrep").unwrap(),
            "aqua:BurntSushi/ripgrep"
        );
        assert_eq!(sanitize_full("npm:@scope/pkg").unwrap(), "npm:@scope/pkg");
        assert_eq!(
            sanitize_full("http:https://user:pass@example.com/a.tar.gz?token=x#f").unwrap(),
            "http:https://example.com/a.tar.gz"
        );
        assert_eq!(
            sanitize_full("https://user:pass@example.com/a.tar.gz?token=x").unwrap(),
            "https://example.com/a.tar.gz"
        );
        assert_eq!(sanitize_full("http:https://user:pass@[bad/a"), None);
        assert_eq!(
            sanitize_full_for_log("http:https://user:pass@[bad/a"),
            "[redacted]"
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
    fn test_github_versions_list_url() {
        assert_eq!(
            github_versions_list_url("BurntSushi/ripgrep", GithubList::Releases).as_deref(),
            Some("https://mise-versions.jdx.dev/data/github/burntsushi/ripgrep/releases.json")
        );
        assert_eq!(
            github_versions_list_url("apache/tomcat", GithubList::Tags).as_deref(),
            Some("https://mise-versions.jdx.dev/data/github/apache/tomcat/tags.json")
        );
        assert_eq!(
            github_versions_list_url(
                "jdx/mise",
                GithubList::ReleasesWithAsset("packslip.sigstore.json")
            )
            .as_deref(),
            Some(
                "https://mise-versions.jdx.dev/data/github/jdx/mise/assets/packslip.sigstore.json/releases.json"
            )
        );
        assert!(github_versions_list_url("o/..", GithubList::Releases).is_none());
        assert!(github_versions_list_url("o/r", GithubList::ReleasesWithAsset("a/b")).is_none());
        assert!(github_versions_list_url("o/r%20", GithubList::Releases).is_none());
    }

    #[test]
    fn test_github_versions_list_deserializes() {
        // The shapes in jdx/mise-versions' docs/github-versions.md.
        let releases: GithubVersionsList = serde_json::from_str(
            r#"{"source":"releases","fetched_at":"2026-10-14T21:20:41Z","releases":[{"tag":"v2.102.0","date":"2026-09-30T02:40:02Z","prerelease":false,"asset_count":22}],"truncated":false}"#,
        )
        .unwrap();
        assert_eq!(releases.releases[0].asset_count, 22);
        assert!(releases.tags.is_empty());
        let tags: GithubVersionsList = serde_json::from_str(
            r#"{"source":"tags","fetched_at":"2026-10-14T21:20:43Z","tags":[{"tag":"11.0.27","sha":"52f3b36","date":null}],"truncated":true}"#,
        )
        .unwrap();
        assert_eq!(tags.tags[0].tag, "11.0.27");
        assert_eq!(tags.tags[0].date, None);
        assert!(tags.truncated);
    }

    #[test]
    fn test_github_tags_url() {
        assert_eq!(
            github_tags_url("apache", "tomcat", 2, false),
            "https://mise-versions.jdx.dev/api/github/repos/apache/tomcat/tags?page=2"
        );
        assert_eq!(
            github_tags_url("apache", "tomcat", 1, true),
            "https://mise-versions.jdx.dev/api/github/repos/apache/tomcat/tags?page=1&dates=1"
        );
    }

    #[test]
    fn test_github_tags_page_deserializes() {
        // The shape mise-versions serves (jdx/mise-versions#369).
        let page: GithubTagsPage = serde_json::from_str(
            r#"{"tags":[{"name":"11.0.27","commit":{"sha":"52f3b36","url":"https://api.github.com/repos/apache/tomcat/commits/52f3b36"},"date":"2026-10-06T18:22:36Z"},{"name":"11.0.26","commit":{"sha":"a1"}}],"next_page":2,"truncated":false}"#,
        )
        .unwrap();
        assert_eq!(page.tags[0].name, "11.0.27");
        assert_eq!(page.tags[0].date.as_deref(), Some("2026-10-06T18:22:36Z"));
        assert_eq!(page.tags[1].date, None);
        assert_eq!(page.next_page, Some(2));
    }

    #[test]
    fn test_github_contents_url_encodes_each_segment() {
        assert_eq!(
            github_contents_url("jdx", "fnox", "cc06c75", "skills/fnox tool/"),
            "https://mise-versions.jdx.dev/api/github/repos/jdx/fnox/contents/skills/fnox%20tool?ref=cc06c75"
        );
    }

    #[test]
    fn test_github_contents_deserialize() {
        let entries: Vec<GithubContentsEntry> = serde_json::from_str(
            r#"[{"name":"SKILL.md","path":"skills/fnox/SKILL.md","type":"file","size":12,"sha":"a"},{"name":"refs","path":"skills/fnox/refs","type":"dir","sha":"b"}]"#,
        )
        .unwrap();
        assert_eq!(entries[1].name, "refs");
        assert_eq!(entries[1].kind, "dir");
    }

    #[test]
    fn test_temporarily_unavailable() {
        let unavailable = |status| -> eyre::Report {
            VersionsHostUnavailable {
                status,
                message: "x".to_string(),
            }
            .into()
        };
        for status in [Some(429), Some(502), Some(503), None] {
            assert!(is_temporarily_unavailable(
                &unavailable(status).wrap_err("failed to read from mise-versions")
            ));
        }
        // Disabled or restricted: not something an old copy should paper over.
        assert!(!is_temporarily_unavailable(&unavailable(Some(403))));
        assert!(!is_temporarily_unavailable(&eyre::eyre!(
            "invalid response"
        )));
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
                from_versions_host: false,
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
        // Pinned API URLs are remembered as mirror-supplied, so their asset ID
        // is confirmed before use; other API URLs are not.
        assert!(is_mirrored_asset_api_url(&ok.releases[2].assets[0].url));
        assert!(!is_mirrored_asset_api_url(
            "https://api.github.com/repos/jdx/private-tool/releases/assets/1"
        ));

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

    #[test]
    fn test_transport_error_status_ignores_404_in_the_message() {
        // A timeout's message carries the URL, which may hold "404".
        let err = eyre::eyre!(
            "error sending request for url (https://mise-versions.jdx.dev/api/github/repos/o/r/releases/v1.404.0): operation timed out"
        );
        assert_eq!(transport_error_status(&err), None);
        let unavailable: eyre::Report = VersionsHostUnavailable {
            status: transport_error_status(&err),
            message: format!("{err:#}"),
        }
        .into();
        assert!(retries_on_github(&unavailable));
    }
}
