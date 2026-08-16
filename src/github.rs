use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::config::Settings;
use crate::tokens;
use crate::{dirs, env};
use eyre::{Result, WrapErr};
use heck::ToKebabCase;
use reqwest::IntoUrl;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{LazyLock as Lazy, Mutex};
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use xx::regex;

pub(crate) mod oauth;
pub(crate) mod sigstore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    // pub name: Option<String>,
    // pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
    pub assets: Vec<GithubAsset>,
}

impl GithubRelease {
    /// The time this release became public. GitHub's `created_at` is the date
    /// of the tagged commit, which may be much older than the publication date.
    pub fn released_at(&self) -> &str {
        self.published_at.as_deref().unwrap_or(&self.created_at)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTag {
    pub name: String,
    pub commit: Option<GithubTagCommit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTagCommit {
    pub sha: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCommit {
    pub commit: GithubCommitInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCommitInfo {
    pub committer: GithubCommitPerson,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCommitPerson {
    pub date: String,
}

/// Tag with date information
#[derive(Debug, Clone)]
pub struct GithubTagWithDate {
    pub name: String,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    // pub size: u64,
    pub browser_download_url: String,
    pub url: String,
    /// SHA256 digest provided by GitHub API (format: "sha256:hash")
    /// Will be null for releases created before this feature was added
    #[serde(default)]
    pub digest: Option<String>,
}

type CacheGroup<T> = HashMap<String, CacheManager<T>>;

static RELEASES_CACHE: Lazy<RwLock<CacheGroup<Vec<GithubRelease>>>> = Lazy::new(Default::default);

static RELEASE_CACHE: Lazy<RwLock<CacheGroup<GithubRelease>>> = Lazy::new(Default::default);

static TAGS_CACHE: Lazy<RwLock<CacheGroup<Vec<String>>>> = Lazy::new(Default::default);

pub static API_URL: &str = "https://api.github.com";

pub static API_PATH: &str = "/api/v3";

/// Without `MISE_LIST_ALL_VERSIONS`, mise normally fetches only the first page of
/// releases to save API quota. The read path filters out prereleases/drafts by
/// default, so a repo whose most recent releases are all prereleases (e.g. nightly
/// builds) would yield zero candidates. `list_releases_` therefore keeps paginating
/// until at least one stable release is seen, bounded to this many pages. (#10343)
const MAX_RELEASE_FALLBACK_PAGES: usize = 3;

async fn get_tags_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<Vec<String>>> {
    TAGS_CACHE
        .write()
        .await
        .entry(key.to_string())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}-tags.msgpack.z")))
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build()
        });
    TAGS_CACHE.read().await
}

async fn get_releases_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<Vec<GithubRelease>>> {
    RELEASES_CACHE
        .write()
        .await
        .entry(key.to_string())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}-all-releases.msgpack.z")))
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build()
        });
    RELEASES_CACHE.read().await
}

async fn get_release_cache<'a>(key: &str) -> RwLockReadGuard<'a, CacheGroup<GithubRelease>> {
    RELEASE_CACHE
        .write()
        .await
        .entry(key.to_string())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}.msgpack.z")))
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build()
        });
    RELEASE_CACHE.read().await
}

pub async fn list_releases(repo: &str) -> Result<Vec<GithubRelease>> {
    Ok(list_releases_including_prereleases(repo)
        .await?
        .into_iter()
        .filter(|r| !r.prerelease)
        .collect())
}

pub async fn list_releases_from_url(api_url: &str, repo: &str) -> Result<Vec<GithubRelease>> {
    Ok(list_releases_including_prereleases_from_url(api_url, repo)
        .await?
        .into_iter()
        .filter(|r| !r.prerelease)
        .collect())
}

/// Like [`list_releases`] but includes releases flagged `prerelease: true`.
/// Drafts are always filtered out. Callers opting in to pre-releases (e.g. the
/// `github:` backend with `prerelease = true`) use this variant; the cache is
/// shared with [`list_releases`] so there's no extra API cost.
pub async fn list_releases_including_prereleases(repo: &str) -> Result<Vec<GithubRelease>> {
    let key = repo.to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || list_releases_(API_URL, repo).await)
        .await?
        .to_vec())
}

pub async fn list_releases_including_prereleases_from_url(
    api_url: &str,
    repo: &str,
) -> Result<Vec<GithubRelease>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || list_releases_(api_url, repo).await)
        .await?
        .to_vec())
}

async fn list_releases_(api_url: &str, repo: &str) -> Result<Vec<GithubRelease>> {
    let mut url = format!("{api_url}/repos/{repo}/releases?per_page=100");
    let headers = get_headers(&url)?;
    let (mut releases, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubRelease>, _>(&url, &headers)
        .await?;

    // Fetch additional pages when MISE_LIST_ALL_VERSIONS is set, or (bounded) while
    // every release seen so far is a prerelease/draft so a stable release is still
    // discovered on a repo dominated by nightlies. (#10343)
    // pages_fetched counts the initial page already fetched above, so the cap
    // applies to the total number of pages rather than to extra requests.
    let mut pages_fetched = 1;
    while let Some(next) = next_page(&headers) {
        if !*env::MISE_LIST_ALL_VERSIONS
            && (releases.iter().any(|r| !r.prerelease && !r.draft)
                || pages_fetched >= MAX_RELEASE_FALLBACK_PAGES)
        {
            break;
        }
        url = crate::http::resolve_pagination_url(&url, &next)?;
        headers = get_headers(&url)?;
        let (more, h) = crate::http::HTTP_FETCH
            .json_headers_with_headers::<Vec<GithubRelease>, _>(&url, &headers)
            .await?;
        releases.extend(more);
        headers = h;
        pages_fetched += 1;
    }
    releases.retain(|r| !r.draft);

    Ok(releases)
}

pub async fn list_tags(repo: &str) -> Result<Vec<String>> {
    let key = repo.to_kebab_case();
    let cache = get_tags_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || {
            list_tags_(API_URL, repo, *env::MISE_LIST_ALL_VERSIONS).await
        })
        .await?
        .to_vec())
}

pub async fn list_tags_from_url(api_url: &str, repo: &str) -> Result<Vec<String>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_tags_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || {
            list_tags_(api_url, repo, *env::MISE_LIST_ALL_VERSIONS).await
        })
        .await?
        .to_vec())
}

/// `list_all` is `MISE_LIST_ALL_VERSIONS`, taken as an argument rather than read here so
/// tests can exercise the pagination loop: the env var is a process-wide `Lazy` that other
/// modules' tests have already forced by the time this one runs.
async fn list_tags_(api_url: &str, repo: &str, list_all: bool) -> Result<Vec<String>> {
    let mut url = format!("{api_url}/repos/{repo}/tags?per_page=100");
    let headers = get_headers(&url)?;
    let (mut tags, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &headers)
        .await?;

    if list_all {
        while let Some(next) = next_page(&headers) {
            url = crate::http::resolve_pagination_url(&url, &next)?;
            headers = get_headers(&url)?;
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &headers)
                .await?;
            tags.extend(more);
            headers = h;
        }
    }

    Ok(tags.into_iter().map(|t| t.name).collect())
}

/// List tags with their commit dates. This is slower than `list_tags` as it requires
/// fetching commit info for each tag. Use only when MISE_LIST_ALL_VERSIONS is set.
pub async fn list_tags_with_dates(repo: &str) -> Result<Vec<GithubTagWithDate>> {
    list_tags_with_dates_(API_URL, repo).await
}

async fn list_tags_with_dates_(api_url: &str, repo: &str) -> Result<Vec<GithubTagWithDate>> {
    let mut url = format!("{api_url}/repos/{repo}/tags?per_page=100");
    let headers = get_headers(&url)?;
    let (mut tags, mut response_headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &headers)
        .await?;

    // Fetch all pages when MISE_LIST_ALL_VERSIONS is set
    while let Some(next) = next_page(&response_headers) {
        url = crate::http::resolve_pagination_url(&url, &next)?;
        response_headers = get_headers(&url)?;
        let (more, h) = crate::http::HTTP_FETCH
            .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &response_headers)
            .await?;
        tags.extend(more);
        response_headers = h;
    }

    // Fetch commit dates in parallel using the parallel utility
    let results = crate::parallel::parallel(tags, |tag| async move {
        let date = if let Some(commit) = tag.commit {
            let headers = get_headers(&commit.url)?;
            match crate::http::HTTP_FETCH
                .json_with_headers::<GithubCommit, _>(&commit.url, &headers)
                .await
            {
                Ok(commit_info) => Some(commit_info.commit.committer.date),
                Err(e) => {
                    warn!("Failed to fetch commit date for tag {}: {}", tag.name, e);
                    None
                }
            }
        } else {
            None
        };
        Ok((tag.name, date))
    })
    .await?;

    Ok(results
        .into_iter()
        .map(|(name, date)| GithubTagWithDate { name, date })
        .collect())
}

pub async fn get_release(repo: &str, tag: &str) -> Result<GithubRelease> {
    get_release_with_versions_host(repo, tag, true).await
}

pub async fn get_release_with_versions_host(
    repo: &str,
    tag: &str,
    use_versions_host: bool,
) -> Result<GithubRelease> {
    let key = release_cache_key(API_URL, repo, tag, use_versions_host);
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    cache
        .get_or_try_init_async_if(
            async || get_release_with_options(API_URL, repo, tag, use_versions_host).await,
            should_cache_release,
        )
        .await
}

pub async fn get_release_for_url_with_versions_host(
    api_url: &str,
    repo: &str,
    tag: &str,
    use_versions_host: bool,
) -> Result<GithubRelease> {
    let key = release_cache_key(api_url, repo, tag, use_versions_host);
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    cache
        .get_or_try_init_async_if(
            async || get_release_with_options(api_url, repo, tag, use_versions_host).await,
            should_cache_release,
        )
        .await
}

fn release_cache_key(api_url: &str, repo: &str, tag: &str, use_versions_host: bool) -> String {
    let source = if use_versions_host {
        "hosted"
    } else {
        "direct"
    };
    format!("{api_url}-{repo}-{tag}-{source}").to_kebab_case()
}

fn should_cache_release(release: &GithubRelease) -> bool {
    !release.assets.is_empty()
}

/// Find the latest build revision for a version in a GitHub repo.
///
/// Build revisions use the pattern `{version}-{N}` where N is an incrementing integer.
/// For example, given version "3.3.11", this will prefer tag "3.3.11-2" over "3.3.11-1"
/// over "3.3.11". Returns the release with the highest build revision and whether
/// a numeric build revision tag was found.
///
/// This is used by precompiled binary repos (e.g., jdx/ruby) where binaries may be
/// rebuilt with different checksums while keeping the same upstream version.
///
/// Note: this relies on `list_releases` which may only return the first page of results
/// when `MISE_LIST_ALL_VERSIONS` is not set. For repos with many releases, older versions
/// may not be found, falling back to the exact version tag via `get_release`.
#[cfg_attr(windows, allow(dead_code))]
pub async fn get_release_with_build_revision_status(
    repo: &str,
    version: &str,
    use_versions_host: bool,
) -> Result<(GithubRelease, bool)> {
    let releases = list_releases(repo).await?;
    match pick_best_numeric_build_revision(releases.clone(), version) {
        Some(release) => Ok((release, true)),
        None => match pick_best_build_revision(releases, version) {
            Some(release) => Ok((release, false)),
            None => Ok((
                get_release_with_versions_host(repo, version, use_versions_host).await?,
                false,
            )),
        },
    }
}

/// Select the highest numeric build revision for a given version.
///
/// Given releases with tags like "3.3.11", "3.3.11-1", "3.3.11-2", picks the
/// highest numeric `-N` suffix and ignores the base version.
#[cfg_attr(windows, allow(dead_code))]
fn pick_best_numeric_build_revision(
    releases: Vec<GithubRelease>,
    version: &str,
) -> Option<GithubRelease> {
    let prefix = format!("{version}-");
    releases
        .into_iter()
        .filter_map(|r| {
            let revision = r
                .tag_name
                .strip_prefix(&prefix)
                .and_then(|suffix| suffix.parse::<u32>().ok())?;
            Some((revision, r))
        })
        .max_by_key(|(revision, _)| *revision)
        .map(|(_, release)| release)
}

/// Select the release with the highest build revision for a given version.
///
/// Given releases with tags like "3.3.11", "3.3.11-1", "3.3.11-2", picks the one
/// with the highest numeric `-N` suffix. The base version (no suffix) is treated as
/// revision 0.
#[cfg_attr(windows, allow(dead_code))]
fn pick_best_build_revision(releases: Vec<GithubRelease>, version: &str) -> Option<GithubRelease> {
    let prefix = format!("{version}-");
    releases
        .into_iter()
        .filter(|r| {
            r.tag_name == version
                || r.tag_name
                    .strip_prefix(&prefix)
                    .is_some_and(|suffix| suffix.parse::<u32>().is_ok())
        })
        .max_by_key(|r| {
            r.tag_name
                .strip_prefix(&prefix)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0)
        })
}

async fn get_release_with_options(
    api_url: &str,
    repo: &str,
    tag: &str,
    use_versions_host: bool,
) -> Result<GithubRelease> {
    if use_versions_host
        && is_public_github_api_base(api_url)
        && let Ok(Some(release)) = crate::versions_host::github_release(repo, tag).await
    {
        trace!("got GitHub release {repo}@{tag} from mise-versions");
        return Ok(release);
    }

    let url = if tag == "latest" {
        format!("{api_url}/repos/{repo}/releases/latest")
    } else {
        format!("{api_url}/repos/{repo}/releases/tags/{tag}")
    };
    let headers = get_headers(&url)?;
    crate::http::HTTP_FETCH
        .json_with_headers(url, &headers)
        .await
}

fn is_public_github_api_base(api_url: &str) -> bool {
    api_url.trim_end_matches('/') == API_URL
}

fn next_page(headers: &HeaderMap) -> Option<String> {
    let link = headers
        .get("link")
        .map(|l| l.to_str().unwrap_or_default().to_string())
        .unwrap_or_default();
    regex!(r#"<([^>]+)>; rel="next""#)
        .captures(&link)
        .map(|c| c.get(1).unwrap().as_str().to_string())
}

fn cache_dir() -> PathBuf {
    dirs::CACHE.join("github")
}

/// The source from which a GitHub token was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    EnvVar(&'static str),
    TokensFile,
    GhCli,
    CredentialCommand,
    GithubOauth,
    GitCredential,
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenSource::EnvVar(name) => write!(f, "{name}"),
            TokenSource::TokensFile => write!(f, "github_tokens.toml"),
            TokenSource::GhCli => write!(f, "gh CLI (hosts.yml)"),
            TokenSource::CredentialCommand => write!(f, "credential_command"),
            TokenSource::GithubOauth => write!(f, "GitHub OAuth"),
            TokenSource::GitCredential => write!(f, "git credential fill"),
        }
    }
}

/// Map API hostnames to the hostnames where GitHub tokens are commonly stored.
fn canonical_token_host(host: &str) -> &str {
    match host {
        "api.github.com" => "github.com",
        h if is_ghe_com_api_host(h) => h.strip_prefix("api.").unwrap_or(h),
        other => other,
    }
}

fn is_github_release_asset_host(host: &str) -> bool {
    matches!(
        host,
        "objects.githubusercontent.com"
            | "objects-origin.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    )
}

fn is_ghe_com_api_host(host: &str) -> bool {
    host.starts_with("api.") && host.ends_with(".ghe.com")
}

fn is_ghes_api_path(path: &str) -> bool {
    path == API_PATH
        || path
            .strip_prefix(API_PATH)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn token_lookup_hosts(host: &str) -> Vec<&str> {
    let canonical = canonical_token_host(host);
    if canonical == host {
        vec![host]
    } else {
        vec![canonical, host]
    }
}

/// Returns true for GitHub REST API URLs.
///
/// Auth and API-version headers must be scoped to these URLs only. Browser URLs
/// such as github.com release downloads and content/CDN URLs under
/// githubusercontent.com are not REST API URLs and can reject or mishandle those
/// headers.
pub fn is_github_api_url(url: &url::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    host == "api.github.com"
        || is_ghe_com_api_host(host)
        || (host != "github.com"
            && !host.ends_with(".githubusercontent.com")
            && !host.ends_with(".ghe.com")
            && is_ghes_api_path(url.path()))
}

/// Pick which URL to use for a GitHub download.
///
/// Public repositories serve release assets, archives, and raw content at their browser-facing
/// URLs, so those URLs are used when reachable. Private repositories return 404 — or a 200 HTML
/// login page — there even with a valid token; in that case the file is fetched from its GitHub
/// API endpoint instead. `get_headers`/`host_auth_headers` add the bearer token and the media type
/// required by release assets or repository content. Shared by GitHub-backed installers so they
/// resolve private downloads consistently.
pub async fn pick_reachable_asset_url(browser_url: &str, api_url: &str) -> String {
    if browser_url == api_url {
        return browser_url.to_string();
    }
    match crate::http::HTTP.head(browser_url).await {
        Ok(resp) => {
            let is_html = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                // HTTP media types are case-insensitive, and the header may carry params
                // (e.g. `text/html; charset=utf-8`), so lowercase before matching.
                .is_some_and(|ct| ct.to_ascii_lowercase().contains("text/html"));
            if is_html {
                debug!(
                    "browser URL returned HTML (likely an auth page), \
                     using the API asset endpoint"
                );
                api_url.to_string()
            } else {
                browser_url.to_string()
            }
        }
        Err(e) => {
            debug!("HEAD on browser URL failed ({e}), using the API asset endpoint");
            api_url.to_string()
        }
    }
}

/// Standard GitHub token env vars, in precedence order (applies to every host).
const GITHUB_TOKEN_ENV_VARS: &[&str] = &["MISE_GITHUB_TOKEN", "GITHUB_API_TOKEN", "GITHUB_TOKEN"];

static TOKEN_SOURCES: Lazy<Mutex<HashMap<String, (String, TokenSource)>>> =
    Lazy::new(Default::default);

/// Remembers the source of the token used to build a request header.
///
/// The 401 error path must not call [`resolve_token`] again: OAuth resolution can
/// synchronously refresh a token, which would block inside the async HTTP send path.
pub(crate) fn remember_token_source(host: &str, token: &str, source: TokenSource) {
    TOKEN_SOURCES
        .lock()
        .unwrap()
        .insert(host.to_string(), (token.to_string(), source));
}

/// Returns the recorded source only when `token` is the token sent for `host`.
///
/// Matching both values prevents a netrc or caller-provided Authorization header
/// from being attributed to an unrelated GitHub credential.
pub(crate) fn token_source_for_token(host: &str, token: &str) -> Option<TokenSource> {
    TOKEN_SOURCES
        .lock()
        .unwrap()
        .get(host)
        .filter(|(recorded, _)| recorded == token)
        .map(|(_, source)| source.clone())
}

/// Resolve the GitHub token for the given hostname, returning the token and its source.
///
/// Priority:
/// 1. `MISE_GITHUB_ENTERPRISE_TOKEN` env var (non-github.com only)
/// 2. `MISE_GITHUB_TOKEN` / `GITHUB_API_TOKEN` / `GITHUB_TOKEN` env vars
/// 3. `credential_command` (if set)
/// 4. native GitHub OAuth device-flow token (if configured)
/// 5. `github_tokens.toml` (per-host)
/// 6. gh CLI token (from `hosts.yml`)
/// 7. `git credential fill` (if enabled)
pub fn resolve_token(host: &str) -> Option<(String, TokenSource)> {
    let settings = Settings::get();

    if is_github_release_asset_host(host) {
        return None;
    }

    #[cfg(test)]
    if let Some(token) = test_support::lookup_tokens_file_override(&token_lookup_hosts(host)) {
        return Some((token, TokenSource::TokensFile));
    }

    let is_ghcom = host == "github.com" || host == "api.github.com";
    let lookup_hosts = token_lookup_hosts(host);

    // 1. Enterprise token (non-github.com only)
    if !is_ghcom && let Some(token) = env::MISE_GITHUB_ENTERPRISE_TOKEN.as_deref() {
        return Some((
            token.to_string(),
            TokenSource::EnvVar("MISE_GITHUB_ENTERPRISE_TOKEN"),
        ));
    }

    // 2. Standard env vars (checked individually for correct precedence and source reporting)
    for var_name in GITHUB_TOKEN_ENV_VARS {
        if let Some(token) = std::env::var(var_name)
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        {
            return Some((token, TokenSource::EnvVar(var_name)));
        }
    }

    // 3. credential_command — call once with the canonical host so
    // `github.com` and `api.github.com` (same instance) share a cache
    // entry, while `github.com` vs a GHE host stay separate. Walking
    // `lookup_hosts` here would spawn the helper twice on a single
    // `resolve_token("api.github.com")` whenever the first call returned
    // `None`, which manifests as extra password-manager prompts.
    let credential_command = &settings.github.credential_command;
    if !credential_command.is_empty()
        && let Some(canonical) = lookup_hosts.first()
        && let Some(token) =
            tokens::get_credential_command_token("github", credential_command, canonical)
    {
        return Some((token, TokenSource::CredentialCommand));
    }

    // 4. native GitHub OAuth device-flow token
    if let Some(token) = oauth::resolve_token(host) {
        return Some((token, TokenSource::GithubOauth));
    }

    // 5. github_tokens.toml
    for lookup_host in &lookup_hosts {
        if let Some(token) = MISE_GITHUB_TOKENS.get(*lookup_host) {
            return Some((token.clone(), TokenSource::TokensFile));
        }
    }

    // 6. gh CLI hosts.yml
    if settings.github.gh_cli_tokens {
        for lookup_host in &lookup_hosts {
            if let Some(token) = GH_HOSTS.get(*lookup_host) {
                return Some((token.clone(), TokenSource::GhCli));
            }
        }
    }

    // 7. git credential fill
    if settings.github.use_git_credentials {
        for lookup_host in &lookup_hosts {
            if let Some(token) = tokens::get_git_credential_token("github", lookup_host) {
                return Some((token, TokenSource::GitCredential));
            }
        }
    }

    None
}

/// Resolve the GitHub token from a full API base URL (e.g., "https://api.github.com").
/// Extracts the hostname and delegates to [`resolve_token`].
pub fn resolve_token_for_api_url(api_url: &str) -> Option<String> {
    let parsed = url::Url::parse(api_url).ok();
    let host = parsed
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or("api.github.com");
    resolve_token(host).map(|(t, _)| t)
}

pub fn get_headers<U: IntoUrl>(url: U) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    let url = url
        .into_url()
        .wrap_err("invalid request URL for GitHub auth headers")?;

    if is_github_api_url(&url) {
        let host = url.host_str().unwrap_or("github.com");
        if let Some((token, source)) = resolve_token(host) {
            remember_token_source(host, &token, source);
            headers.insert(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(format!("Bearer {token}").as_str()).unwrap(),
            );
            headers.insert(
                "x-github-api-version",
                HeaderValue::from_static("2022-11-28"),
            );
        } else {
            TOKEN_SOURCES.lock().unwrap().remove(host);
        }
    }

    if is_github_api_url(&url) && url.path().contains("/releases/assets/") {
        headers.insert(
            "accept",
            HeaderValue::from_static("application/octet-stream"),
        );
    } else if is_github_api_url(&url) && url.path().contains("/contents/") {
        // https://docs.github.com/en/rest/repos/contents#custom-media-types-for-repository-contents
        headers.insert(
            "accept",
            HeaderValue::from_static("application/vnd.github.raw"),
        );
    }

    Ok(headers)
}

// ── github_tokens.toml ──────────────────────────────────────────────

/// Tokens from $MISE_CONFIG_DIR/github_tokens.toml.
/// Maps hostname (e.g. "github.com") to token string.
static MISE_GITHUB_TOKENS: Lazy<HashMap<String, String>> =
    Lazy::new(|| read_mise_github_tokens().unwrap_or_default());

#[cfg(test)]
fn parse_github_tokens(contents: &str) -> Option<HashMap<String, String>> {
    tokens::parse_tokens_toml(contents)
}

fn read_mise_github_tokens() -> Option<HashMap<String, String>> {
    tokens::read_tokens_toml("github_tokens.toml", "github_tokens.toml")
}

// ── gh CLI hosts.yml ────────────────────────────────────────────────

/// Tokens read from the gh CLI hosts config (~/.config/gh/hosts.yml).
/// Maps hostname (e.g. "github.com") to oauth_token.
static GH_HOSTS: Lazy<HashMap<String, String>> = Lazy::new(|| read_gh_hosts().unwrap_or_default());

/// Resolve the path to gh CLI's hosts.yml, following go-gh's own `ConfigDir()`:
/// 1. `$GH_CONFIG_DIR/hosts.yml`
/// 2. `$XDG_CONFIG_HOME/gh/hosts.yml` — only when that variable is actually set, which is gh's
///    condition; `env::XDG_CONFIG_HOME` defaults to `~/.config` and so cannot express it
/// 3. `%APPDATA%\GitHub CLI\hosts.yml` on Windows — gh checks `AppData` explicitly rather than
///    going through XDG, so this is the default location there and mise never looked at it.
///    Like gh, this branch is taken only when the variable is actually set.
/// 4. `~/.config/gh/hosts.yml`, gh's own last branch, used as the fallback
///
/// The macOS candidate is kept for compatibility but does not correspond to a gh branch: gh
/// uses `~/.config/gh` on macOS too.
fn gh_hosts_path() -> Option<PathBuf> {
    // Explicit GH_CONFIG_DIR takes priority. Empty means unset, as in go-gh's
    // `os.Getenv(ghConfigDir) != ""`; `var_os` rather than `var` so a non-UTF-8 directory is
    // honoured instead of silently skipped.
    if let Some(dir) = std::env::var_os("GH_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join("hosts.yml"));
    }

    // When XDG_CONFIG_HOME is set it is both gh's next branch and the right thing to name in a
    // trace if nothing is found; otherwise the branch gh would fall to on this platform.
    // `var_path` treats an empty value as unset, matching go-gh's
    // `os.Getenv(xdgConfigHome) != ""`.
    let xdg_path = env::var_path("XDG_CONFIG_HOME").map(|dir| dir.join("gh/hosts.yml"));
    let fallback = xdg_path.clone().unwrap_or_else(gh_default_hosts_path);

    let candidates = xdg_path
        .into_iter()
        .chain(gh_native_hosts_paths())
        .collect();
    Some(tokens::first_existing_file(candidates, fallback))
}

/// `%APPDATA%\GitHub CLI\hosts.yml`, or `None` when `APPDATA` is unset or empty.
///
/// go-gh guards that branch with `os.Getenv(appData) != ""` and otherwise falls through to
/// `~/.config/gh`, so the variable being absent is meaningful — synthesizing `~/AppData/Roaming`
/// here would send mise to a directory gh would never have used.
#[cfg(windows)]
fn gh_appdata_hosts_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(|v| PathBuf::from(v).join("GitHub CLI/hosts.yml"))
}

/// Where gh lands when neither `GH_CONFIG_DIR` nor `XDG_CONFIG_HOME` is set.
#[cfg(windows)]
fn gh_default_hosts_path() -> PathBuf {
    gh_appdata_hosts_path().unwrap_or_else(|| dirs::HOME.join(".config/gh/hosts.yml"))
}

#[cfg(not(windows))]
fn gh_default_hosts_path() -> PathBuf {
    dirs::HOME.join(".config/gh/hosts.yml")
}

/// Platform-native locations gh may have written to, probed after the XDG one.
#[cfg(target_os = "macos")]
fn gh_native_hosts_paths() -> Vec<PathBuf> {
    vec![dirs::HOME.join("Library/Application Support/gh/hosts.yml")]
}

#[cfg(windows)]
fn gh_native_hosts_paths() -> Vec<PathBuf> {
    gh_appdata_hosts_path().into_iter().collect()
}

#[cfg(all(not(target_os = "macos"), not(windows)))]
fn gh_native_hosts_paths() -> Vec<PathBuf> {
    Vec::new()
}

fn read_gh_hosts() -> Option<HashMap<String, String>> {
    let hosts_path = gh_hosts_path()?;
    let contents = match std::fs::read_to_string(&hosts_path) {
        Ok(c) => c,
        Err(e) => {
            trace!("gh hosts.yml not readable at {}: {e}", hosts_path.display());
            return None;
        }
    };
    let hosts: HashMap<String, GhHostEntry> = match serde_yaml::from_str(&contents) {
        Ok(h) => h,
        Err(e) => {
            debug!(
                "failed to parse gh hosts.yml at {}: {e}",
                hosts_path.display()
            );
            return None;
        }
    };
    Some(
        hosts
            .into_iter()
            .filter_map(|(host, entry)| entry.oauth_token.map(|token| (host, token)))
            .collect(),
    )
}

#[derive(Deserialize)]
struct GhHostEntry {
    oauth_token: Option<String>,
}

/// Serializes env-var mutations across every `#[cfg(test)]` module that touches GitHub token
/// environment variables. `github::tests` and `github::sigstore::tests` both mutate the same
/// four tokens (`MISE_GITHUB_TOKEN`, `GITHUB_API_TOKEN`, `GITHUB_TOKEN`,
/// `MISE_GITHUB_ENTERPRISE_TOKEN`); sharing a single lock prevents parallel test runs from
/// racing.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) mod test_support {
    //! Test-only hooks that let sibling modules seed non-env-var token sources without
    //! spinning up global configuration infrastructure. Only consulted from `resolve_token`
    //! under `#[cfg(test)]`; production builds never see these statics.

    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Overrides the `github_tokens.toml` source in [`super::resolve_token`].
    /// Keyed by the same lookup hosts `resolve_token` walks — e.g. `"github.com"`.
    /// Hold [`super::TEST_ENV_LOCK`] while mutating; always clear before returning.
    pub(crate) static TOKENS_FILE_OVERRIDE: RwLock<Option<HashMap<String, String>>> =
        RwLock::new(None);

    pub(crate) fn lookup_tokens_file_override(lookup_hosts: &[&str]) -> Option<String> {
        let guard = TOKENS_FILE_OVERRIDE.read().ok()?;
        let map = guard.as_ref()?;
        for host in lookup_hosts {
            if let Some(token) = map.get(*host) {
                return Some(token.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSET_API_URL: &str = "https://api.github.com/repos/o/r/releases/assets/1";

    #[test]
    fn test_github_content_headers_request_raw_content() {
        let headers = get_headers("https://api.github.com/repos/o/r/contents/bin/tool").unwrap();

        assert_eq!(headers.get("accept").unwrap(), "application/vnd.github.raw");
    }

    #[tokio::test]
    async fn test_pick_reachable_asset_url_keeps_browser_url_when_reachable() {
        // Public repos: the browser URL serves the asset (not HTML), so it is kept and the
        // API endpoint is not used.
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("HEAD", "/asset.tar.gz")
            .with_status(200)
            .with_header("content-type", "application/octet-stream")
            .create_async()
            .await;
        let browser_url = format!("{}/asset.tar.gz", server.url());
        assert_eq!(
            pick_reachable_asset_url(&browser_url, ASSET_API_URL).await,
            browser_url
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_pick_reachable_asset_url_falls_back_on_404() {
        // Private repos: the browser URL 404s even with a valid token, so fall back to the
        // API asset endpoint.
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("HEAD", "/asset.tar.gz")
            .with_status(404)
            .create_async()
            .await;
        let browser_url = format!("{}/asset.tar.gz", server.url());
        assert_eq!(
            pick_reachable_asset_url(&browser_url, ASSET_API_URL).await,
            ASSET_API_URL
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_pick_reachable_asset_url_falls_back_on_html_login_page() {
        // Some private repos return a 200 HTML login page at the browser URL instead of a
        // 404; that is also treated as unreachable and falls back to the API endpoint.
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("HEAD", "/asset.tar.gz")
            .with_status(200)
            .with_header("content-type", "text/html; charset=utf-8")
            .create_async()
            .await;
        let browser_url = format!("{}/asset.tar.gz", server.url());
        assert_eq!(
            pick_reachable_asset_url(&browser_url, ASSET_API_URL).await,
            ASSET_API_URL
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_pick_reachable_asset_url_falls_back_on_uppercase_html_content_type() {
        // HTTP media types are case-insensitive; a `Content-Type` such as `TEXT/HTML` must
        // still be recognized as an auth page and fall back to the API endpoint.
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("HEAD", "/asset.tar.gz")
            .with_status(200)
            .with_header("content-type", "TEXT/HTML; charset=UTF-8")
            .create_async()
            .await;
        let browser_url = format!("{}/asset.tar.gz", server.url());
        assert_eq!(
            pick_reachable_asset_url(&browser_url, ASSET_API_URL).await,
            ASSET_API_URL
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_pick_reachable_asset_url_skips_probe_when_urls_equal() {
        // When both URLs are identical there is nothing to fall back to, so no request is
        // made and the URL is returned as-is.
        assert_eq!(
            pick_reachable_asset_url(ASSET_API_URL, ASSET_API_URL).await,
            ASSET_API_URL
        );
    }

    const GITHUB_TOKEN_VARS: [&str; 4] = [
        "MISE_GITHUB_TOKEN",
        "GITHUB_API_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_ENTERPRISE_TOKEN",
    ];

    /// Holds [`super::TEST_ENV_LOCK`] and puts the token variables back in `Drop`.
    ///
    /// Restoring in `Drop` rather than after the callback is what makes the lock's poison flag
    /// unnecessary: `Drop` runs while unwinding, so a panicking test cannot leave `ghp_test`
    /// behind for whatever runs next.
    struct GithubTokenGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        // `var_os`, not `var`: the latter reports a non-Unicode value as `None`, which `Drop`
        // would then read as "was unset" and delete. Same shape as `crate::test::EnvVarGuard`.
        prev: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl GithubTokenGuard {
        fn new() -> Self {
            let lock = crate::test::lock_ignoring_poison(&super::TEST_ENV_LOCK);
            let prev = GITHUB_TOKEN_VARS
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect();

            env::remove_var("MISE_GITHUB_TOKEN");
            env::remove_var("GITHUB_API_TOKEN");
            env::set_var("GITHUB_TOKEN", "ghp_test");
            env::remove_var("MISE_GITHUB_ENTERPRISE_TOKEN");

            Self { _lock: lock, prev }
        }
    }

    impl Drop for GithubTokenGuard {
        fn drop(&mut self) {
            for (name, prev) in self.prev.drain(..) {
                match prev {
                    Some(v) => env::set_var(name, v),
                    None => env::remove_var(name),
                }
            }
        }
    }

    fn with_github_token<F, R>(test_fn: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = GithubTokenGuard::new();
        test_fn()
    }

    struct TokensFileOverrideGuard;

    impl TokensFileOverrideGuard {
        fn set(host: &str, token: &str) -> Self {
            let mut tokens = HashMap::new();
            tokens.insert(host.to_string(), token.to_string());
            *test_support::TOKENS_FILE_OVERRIDE.write().unwrap() = Some(tokens);
            Self
        }
    }

    impl Drop for TokensFileOverrideGuard {
        fn drop(&mut self) {
            *test_support::TOKENS_FILE_OVERRIDE.write().unwrap() = None;
        }
    }

    #[test]
    fn test_token_source_memo_matches_only_the_supplying_token_and_host() {
        let host = "github-source-test.example.com";
        remember_token_source(host, "ghp_test", TokenSource::EnvVar("GITHUB_TOKEN"));

        assert_eq!(
            token_source_for_token(host, "ghp_test"),
            Some(TokenSource::EnvVar("GITHUB_TOKEN"))
        );
        assert_eq!(token_source_for_token(host, "from-netrc"), None);
        assert_eq!(
            token_source_for_token("another-github.example.com", "ghp_test"),
            None
        );
    }

    #[test]
    fn test_get_headers_remembers_tokens_file_source() {
        let _lock = crate::test::lock_ignoring_poison(&TEST_ENV_LOCK);
        let host = "github-tokens-file-test.example.com";
        let _tokens_file = TokensFileOverrideGuard::set(host, "ghp_from_tokens_file");

        let headers =
            get_headers(format!("https://{host}/api/v3/repos/owner/repo/releases")).unwrap();

        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).unwrap(),
            "Bearer ghp_from_tokens_file"
        );
        assert_eq!(
            token_source_for_token(host, "ghp_from_tokens_file"),
            Some(TokenSource::TokensFile)
        );
    }

    #[test]
    fn test_parse_github_tokens() {
        let toml = r#"
[tokens."github.com"]
token = "ghp_abc123"

[tokens."github.mycompany.com"]
token = "ghp_def456"
"#;
        let result = parse_github_tokens(toml).unwrap();
        assert_eq!(result.get("github.com").unwrap(), "ghp_abc123");
        assert_eq!(result.get("github.mycompany.com").unwrap(), "ghp_def456");
    }

    #[test]
    fn test_parse_github_tokens_empty() {
        assert!(parse_github_tokens("").is_none());
    }

    #[test]
    fn test_parse_github_tokens_empty_tokens() {
        let toml = "[tokens]\n";
        let result = parse_github_tokens(toml).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_github_tokens_missing_token_field() {
        let toml = r#"
[tokens."github.com"]
something_else = "value"
"#;
        let result = parse_github_tokens(toml).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_api_host_token_lookup_hosts() {
        assert_eq!(
            token_lookup_hosts("api.github.com"),
            vec!["github.com", "api.github.com"]
        );
        assert_eq!(
            token_lookup_hosts("api.octocorp.ghe.com"),
            vec!["octocorp.ghe.com", "api.octocorp.ghe.com"]
        );
        assert_eq!(
            token_lookup_hosts("github.example.com"),
            vec!["github.example.com"]
        );
    }

    #[test]
    fn test_only_github_api_urls_use_github_token() {
        with_github_token(|| {
            for url in [
                "https://github.com/api/v3/repos/owner/repo/releases",
                "https://github.com/cuotos/ecs-exec-pf/releases/download/v0.3.0/ecs-exec-pf_0.3.0_Linux_x86_64.tar.gz",
                "https://github.example.com/owner/repo/releases/download/v1.0.0/file.tar.gz",
                "https://raw.githubusercontent.com/owner/repo/main/file.txt",
                "https://objects.githubusercontent.com/github-production-release-asset",
                "https://objects-origin.githubusercontent.com/github-production-release-asset",
                "https://release-assets.githubusercontent.com/github-production-release-asset",
                "https://octocorp.ghe.com/api/v3/repos/owner/repo/releases",
                "https://octocorp.ghe.com/owner/repo/releases/download/v1.0.0/file.tar.gz",
            ] {
                let headers = get_headers(url).unwrap();
                assert!(
                    !headers.contains_key(reqwest::header::AUTHORIZATION),
                    "{url} should not use GitHub auth"
                );
                assert!(
                    !headers.contains_key("x-github-api-version"),
                    "{url} should not use GitHub API version"
                );
            }

            let headers = get_headers("https://api.github.com/repos/owner/repo/releases").unwrap();
            assert!(headers.contains_key(reqwest::header::AUTHORIZATION));
            assert!(headers.contains_key("x-github-api-version"));

            let headers =
                get_headers("https://api.github.com/repos/owner/repo/releases/assets/1").unwrap();
            assert!(headers.contains_key(reqwest::header::AUTHORIZATION));
            assert_eq!(headers.get("accept").unwrap(), "application/octet-stream");

            let headers =
                get_headers("https://github.example.com/api/v3/repos/owner/repo/releases").unwrap();
            assert!(headers.contains_key(reqwest::header::AUTHORIZATION));
            assert!(headers.contains_key("x-github-api-version"));

            let headers =
                get_headers("https://api.octocorp.ghe.com/repos/owner/repo/releases").unwrap();
            assert!(headers.contains_key(reqwest::header::AUTHORIZATION));
            assert!(headers.contains_key("x-github-api-version"));
        });
    }

    #[test]
    fn test_get_headers_rejects_relative_url() {
        let err = get_headers("/repos/jdx/aube/releases").unwrap_err();
        assert!(
            err.to_string()
                .contains("invalid request URL for GitHub auth headers"),
            "unexpected error: {err}"
        );
    }

    fn make_release(tag: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_string(),
            draft: false,
            prerelease: false,
            created_at: String::new(),
            published_at: None,
            assets: vec![],
        }
    }

    #[test]
    fn release_date_prefers_published_at() {
        let mut release = make_release("v1.0.0");
        release.created_at = "2026-06-28T17:38:00Z".into();
        release.published_at = Some("2026-08-06T09:16:56Z".into());

        assert_eq!(release.released_at(), "2026-08-06T09:16:56Z");
    }

    #[test]
    fn release_date_falls_back_to_created_at() {
        let mut release = make_release("v1.0.0");
        release.created_at = "2026-06-28T17:38:00Z".into();

        assert_eq!(release.released_at(), "2026-06-28T17:38:00Z");
    }

    #[test]
    fn release_without_published_at_remains_deserializable() {
        let release: GithubRelease = serde_json::from_value(serde_json::json!({
            "tag_name": "v1.0.0",
            "draft": false,
            "prerelease": false,
            "created_at": "2026-06-28T17:38:00Z",
            "assets": []
        }))
        .unwrap();

        assert_eq!(release.released_at(), "2026-06-28T17:38:00Z");
    }

    #[test]
    fn test_build_revision_selects_highest() {
        let releases = vec![
            make_release("3.3.11"),
            make_release("3.3.11-1"),
            make_release("3.3.11-2"),
            make_release("3.3.10-1"),
        ];
        let best = pick_best_build_revision(releases, "3.3.11").unwrap();
        assert_eq!(best.tag_name, "3.3.11-2");
    }

    #[test]
    fn test_numeric_build_revision_selects_highest_without_base_fallback() {
        let releases = vec![
            make_release("3.3.11"),
            make_release("3.3.11-1"),
            make_release("3.3.11-2"),
            make_release("3.3.10-1"),
        ];
        let best = pick_best_numeric_build_revision(releases, "3.3.11").unwrap();
        assert_eq!(best.tag_name, "3.3.11-2");

        let releases = vec![make_release("3.3.11"), make_release("3.3.10-1")];
        assert!(pick_best_numeric_build_revision(releases, "3.3.11").is_none());
    }

    /// RubyInstaller2 tags releases `RubyInstaller-<version>-<revision>`, so the
    /// caller passes the prefixed tag as the "version" (discussion #5227). The
    /// prefix comparison must also keep 3.4.4 from matching 3.4.10.
    #[test]
    fn test_numeric_build_revision_handles_prefixed_tags() {
        let releases = vec![
            make_release("RubyInstaller-3.4.4-1"),
            make_release("RubyInstaller-3.4.4-2"),
            make_release("RubyInstaller-3.4.10-1"),
        ];
        let best = pick_best_numeric_build_revision(releases, "RubyInstaller-3.4.4").unwrap();
        assert_eq!(best.tag_name, "RubyInstaller-3.4.4-2");
    }

    #[test]
    fn test_build_revision_falls_back_to_base() {
        let releases = vec![make_release("3.3.11"), make_release("3.3.10-1")];
        let best = pick_best_build_revision(releases, "3.3.11").unwrap();
        assert_eq!(best.tag_name, "3.3.11");
    }

    #[test]
    fn test_build_revision_no_match() {
        let releases = vec![make_release("3.3.10"), make_release("3.3.10-1")];
        let best = pick_best_build_revision(releases, "3.3.11");
        assert!(best.is_none());
    }

    #[test]
    fn test_build_revision_ignores_non_numeric_suffix() {
        let releases = vec![
            make_release("3.3.11"),
            make_release("3.3.11-rc1"),
            make_release("3.3.11-1"),
        ];
        let best = pick_best_build_revision(releases, "3.3.11").unwrap();
        assert_eq!(best.tag_name, "3.3.11-1");
    }

    fn make_asset(name: &str) -> GithubAsset {
        GithubAsset {
            name: name.to_string(),
            browser_download_url: format!("https://github.com/owner/repo/releases/download/{name}"),
            url: format!("https://api.github.com/repos/owner/repo/releases/assets/{name}"),
            digest: None,
        }
    }

    #[tokio::test]
    async fn test_empty_release_assets_are_not_cached() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let repo = "owner/empty-assets-cache-test";
        let tag = "v1.0.0";
        let path = format!("/repos/{repo}/releases/tags/{tag}");
        let key = release_cache_key(&server.url(), repo, tag, true);

        let cached_empty_release = make_release(tag);
        {
            let cache_group = get_release_cache(&key).await;
            let cache = cache_group.get(&key).unwrap();
            cache.write(&cached_empty_release).unwrap();
        }

        let empty_mock = server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&cached_empty_release).unwrap())
            .expect(1)
            .create_async()
            .await;

        let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, true)
            .await
            .unwrap();
        assert!(release.assets.is_empty());
        empty_mock.assert_async().await;
        empty_mock.remove_async().await;

        let populated_release = GithubRelease {
            assets: vec![make_asset("tool-v1.0.0-linux-x86_64.tar.gz")],
            ..make_release(tag)
        };
        let mock = server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&populated_release).unwrap())
            .expect(1)
            .create_async()
            .await;

        let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, true)
            .await
            .unwrap();
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "tool-v1.0.0-linux-x86_64.tar.gz");

        let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, true)
            .await
            .unwrap();
        assert_eq!(release.assets.len(), 1);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_versions_host_flag_splits_release_cache() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let repo = "owner/versions-host-cache-split-test";
        let tag = "v1.0.0";
        let path = format!("/repos/{repo}/releases/tags/{tag}");
        let true_key = release_cache_key(&server.url(), repo, tag, true);

        {
            let cache_group = get_release_cache(&true_key).await;
            let cache = cache_group.get(&true_key).unwrap();
            cache
                .write(&GithubRelease {
                    assets: vec![make_asset("cached-from-versions-host.tar.gz")],
                    ..make_release(tag)
                })
                .unwrap();
        }

        let direct_release = GithubRelease {
            assets: vec![make_asset("direct-github-api.tar.gz")],
            ..make_release(tag)
        };
        let mock = server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&direct_release).unwrap())
            .expect(1)
            .create_async()
            .await;

        let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, false)
            .await
            .unwrap();
        assert_eq!(release.assets[0].name, "direct-github-api.tar.gz");
        mock.assert_async().await;
    }

    fn make_prerelease(tag: &str) -> GithubRelease {
        GithubRelease {
            prerelease: true,
            ..make_release(tag)
        }
    }

    // #10343: a first page made up entirely of prereleases must not yield "no
    // versions found" -- the fallback follows the Link header to a later page.
    #[tokio::test]
    async fn test_list_releases_paginates_past_all_prerelease_first_page() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let repo = "owner/all-prerelease-first-page";

        let page1 = vec![
            make_prerelease("v2.0.0-alpha.2"),
            make_prerelease("v2.0.0-alpha.1"),
        ];
        let page2 = vec![make_release("v1.0.0")];

        // The first page requests per_page=100 and is entirely prereleases.
        let page1_mock = server
            .mock("GET", format!("/repos/{repo}/releases").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{base}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&page1).unwrap())
            .expect(1)
            .create_async()
            .await;
        // The fallback follows the Link header to a second page that has a stable release.
        let page2_mock = server
            .mock("GET", "/page2")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&page2).unwrap())
            .expect(1)
            .create_async()
            .await;

        let releases = list_releases_(&base, repo).await.unwrap();
        page1_mock.assert_async().await;
        page2_mock.assert_async().await;
        assert!(
            releases
                .iter()
                .any(|r| r.tag_name == "v1.0.0" && !r.prerelease),
            "stable release from page 2 should be discovered, got {:?}",
            releases.iter().map(|r| &r.tag_name).collect::<Vec<_>>()
        );
    }

    // #10343: once a stable release is seen the fallback stops (no extra API calls).
    #[tokio::test]
    async fn test_list_releases_stops_when_first_page_has_stable() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let repo = "owner/stable-on-first-page";

        let page1 = vec![make_prerelease("v1.1.0-alpha.1"), make_release("v1.0.0")];

        let page1_mock = server
            .mock("GET", format!("/repos/{repo}/releases").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{base}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&page1).unwrap())
            .expect(1)
            .create_async()
            .await;
        // A stable release is already present, so page 2 must NOT be fetched.
        let page2_mock = server
            .mock("GET", "/page2")
            .with_status(200)
            .with_body("[]")
            .expect(0)
            .create_async()
            .await;

        let releases = list_releases_(&base, repo).await.unwrap();
        page1_mock.assert_async().await;
        page2_mock.assert_async().await;
        assert!(releases.iter().any(|r| r.tag_name == "v1.0.0"));
    }

    // #10343: the prerelease fallback is bounded to MAX_RELEASE_FALLBACK_PAGES pages.
    #[tokio::test]
    async fn test_list_releases_fallback_pagination_is_bounded() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let repo = "owner/all-prerelease-many-pages";

        let body = || serde_json::to_string(&vec![make_prerelease("v9.0.0-alpha")]).unwrap();

        // Three all-prerelease pages, each linking to the next.
        let p1 = server
            .mock("GET", format!("/repos/{repo}/releases").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{base}/p2>; rel=\"next\"").as_str())
            .with_body(body())
            .expect(1)
            .create_async()
            .await;
        let p2 = server
            .mock("GET", "/p2")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{base}/p3>; rel=\"next\"").as_str())
            .with_body(body())
            .expect(1)
            .create_async()
            .await;
        let p3 = server
            .mock("GET", "/p3")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{base}/p4>; rel=\"next\"").as_str())
            .with_body(body())
            .expect(1)
            .create_async()
            .await;
        // The 4th page must never be requested (capped at MAX_RELEASE_FALLBACK_PAGES).
        let p4 = server
            .mock("GET", "/p4")
            .with_status(200)
            .with_body("[]")
            .expect(0)
            .create_async()
            .await;

        let releases = list_releases_(&base, repo).await.unwrap();
        p1.assert_async().await;
        p2.assert_async().await;
        p3.assert_async().await;
        p4.assert_async().await;
        assert_eq!(releases.len(), 3);
    }

    const PAGINATE_TEST_TOKEN: &str = "ghp_paginate_test";

    /// A GHES-shaped base URL: `get_headers` only attaches auth to REST API URLs, and for a
    /// host that is not api.github.com that means the path must sit under [`API_PATH`].
    fn ghes_api_url(base: &str) -> String {
        format!("{base}{API_PATH}")
    }

    fn tag_without_commit(name: &str) -> GithubTag {
        GithubTag {
            name: name.to_string(),
            commit: None,
        }
    }

    // Regression for #6318: every paginated request must carry the Authorization header.
    // Before that fix page 2 was sent page 1's *response* headers and went out
    // unauthenticated; nothing pinned the fix until now.
    #[tokio::test]
    async fn test_list_releases_sends_auth_on_every_page() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let api = ghes_api_url(&base);
        let repo = "owner/auth-on-every-page";
        let host = url::Url::parse(&base)
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let _token = TokensFileOverrideGuard::set(&host, PAGINATE_TEST_TOKEN);
        let auth = format!("Bearer {PAGINATE_TEST_TOKEN}");

        let page1 = server
            .mock("GET", format!("{API_PATH}/repos/{repo}/releases").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{api}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&vec![make_prerelease("v2.0.0-alpha.1")]).unwrap())
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", format!("{API_PATH}/page2").as_str())
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![make_release("v1.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;

        let releases = list_releases_(&api, repo).await.unwrap();
        page1.assert_async().await;
        page2.assert_async().await;
        assert_eq!(releases.len(), 2);
    }

    // Same regression for the tags loop -- see the release test above.
    #[tokio::test]
    async fn test_list_tags_sends_auth_on_every_page() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let api = ghes_api_url(&base);
        let repo = "owner/auth-on-every-page";
        let host = url::Url::parse(&base)
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let _token = TokensFileOverrideGuard::set(&host, PAGINATE_TEST_TOKEN);
        let auth = format!("Bearer {PAGINATE_TEST_TOKEN}");

        let page1 = server
            .mock("GET", format!("{API_PATH}/repos/{repo}/tags").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{api}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&vec![tag_without_commit("v2.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", format!("{API_PATH}/page2").as_str())
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![tag_without_commit("v1.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;

        let tags = list_tags_(&api, repo, true).await.unwrap();
        page1.assert_async().await;
        page2.assert_async().await;
        assert_eq!(tags, ["v2.0.0", "v1.0.0"]);
    }

    // `list_tags_with_dates_` paginates unconditionally, so it needs the same guarantee.
    // Tags carry no `commit`, which keeps this to the two paginated requests.
    #[tokio::test]
    async fn test_list_tags_with_dates_sends_auth_on_every_page() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let api = ghes_api_url(&base);
        let repo = "owner/auth-on-every-page-dates";
        let host = url::Url::parse(&base)
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let _token = TokensFileOverrideGuard::set(&host, PAGINATE_TEST_TOKEN);
        let auth = format!("Bearer {PAGINATE_TEST_TOKEN}");

        let page1 = server
            .mock("GET", format!("{API_PATH}/repos/{repo}/tags").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{api}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&vec![tag_without_commit("v2.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", format!("{API_PATH}/page2").as_str())
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![tag_without_commit("v1.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;

        let tags = list_tags_with_dates_(&api, repo).await.unwrap();
        page1.assert_async().await;
        page2.assert_async().await;
        assert_eq!(
            tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["v2.0.0", "v1.0.0"]
        );
        assert!(tags.iter().all(|t| t.date.is_none()));
    }
}
