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
use std::sync::LazyLock as Lazy;
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
    // pub published_at: Option<String>,
    pub assets: Vec<GithubAsset>,
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
        url = resolve_pagination_url(&url, &next)?;
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
        .get_or_try_init_async(async || list_tags_(API_URL, repo).await)
        .await?
        .to_vec())
}

pub async fn list_tags_from_url(api_url: &str, repo: &str) -> Result<Vec<String>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_tags_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || list_tags_(api_url, repo).await)
        .await?
        .to_vec())
}

async fn list_tags_(api_url: &str, repo: &str) -> Result<Vec<String>> {
    let mut url = format!("{api_url}/repos/{repo}/tags?per_page=100");
    let headers = get_headers(&url)?;
    let (mut tags, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &headers)
        .await?;

    if *env::MISE_LIST_ALL_VERSIONS {
        while let Some(next) = next_page(&headers) {
            url = resolve_pagination_url(&url, &next)?;
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
        url = resolve_pagination_url(&url, &next)?;
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
    let key = release_cache_key(API_URL, repo, tag, true);
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    cache
        .get_or_try_init_async_if(
            async || get_release_with_options(API_URL, repo, tag, true).await,
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
) -> Result<(GithubRelease, bool)> {
    let releases = list_releases(repo).await?;
    match pick_best_numeric_build_revision(releases.clone(), version) {
        Some(release) => Ok((release, true)),
        None => match pick_best_build_revision(releases, version) {
            Some(release) => Ok((release, false)),
            None => Ok((get_release(repo, version).await?, false)),
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

fn resolve_pagination_url(current: &str, next: &str) -> Result<String> {
    if next.starts_with("http://") || next.starts_with("https://") {
        return Ok(next.to_string());
    }
    let base = url::Url::parse(current)
        .wrap_err_with(|| format!("invalid pagination base URL: {current}"))?;
    if next.starts_with('/') {
        return Ok(format!("{}{next}", base.origin().ascii_serialization()));
    }
    base.join(next)
        .map(|u| u.to_string())
        .wrap_err_with(|| format!("invalid pagination URL: {next}"))
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
    for var_name in &["MISE_GITHUB_TOKEN", "GITHUB_API_TOKEN", "GITHUB_TOKEN"] {
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
    #[cfg(test)]
    if let Some((token, source)) = test_support::lookup_tokens_file_override(&lookup_hosts)
        .map(|t| (t, TokenSource::TokensFile))
    {
        return Some((token, source));
    }
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

    if is_github_api_url(&url)
        && let Some((token, _source)) = resolve_token(url.host_str().unwrap_or("github.com"))
    {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(format!("Bearer {token}").as_str()).unwrap(),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );
    }

    if is_github_api_url(&url) && url.path().contains("/releases/assets/") {
        headers.insert(
            "accept",
            HeaderValue::from_static("application/octet-stream"),
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

/// Resolve the path to gh CLI's hosts.yml, matching gh's own config resolution:
/// 1. $GH_CONFIG_DIR/hosts.yml
/// 2. $XDG_CONFIG_HOME/gh/hosts.yml (env::XDG_CONFIG_HOME handles the fallback to ~/.config)
/// 3. ~/Library/Application Support/gh/hosts.yml (macOS native path from Go's os.UserConfigDir)
fn gh_hosts_path() -> Option<PathBuf> {
    // Explicit GH_CONFIG_DIR takes priority
    if let Ok(dir) = std::env::var("GH_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("hosts.yml"));
    }
    // Try XDG path (env::XDG_CONFIG_HOME falls back to ~/.config)
    let xdg_path = env::XDG_CONFIG_HOME.join("gh/hosts.yml");
    if xdg_path.exists() {
        return Some(xdg_path);
    }
    // Try macOS native config dir
    #[cfg(target_os = "macos")]
    {
        let macos_path = dirs::HOME.join("Library/Application Support/gh/hosts.yml");
        if macos_path.exists() {
            return Some(macos_path);
        }
    }
    // Fall back to XDG path even if it doesn't exist (will produce a trace log)
    Some(xdg_path)
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

    /// Overrides the `github_tokens.toml` path (source #4 in [`super::resolve_token`]).
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

    fn with_github_token<F, R>(test_fn: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = super::TEST_ENV_LOCK.lock().unwrap();
        let orig_mise = std::env::var("MISE_GITHUB_TOKEN").ok();
        let orig_api = std::env::var("GITHUB_API_TOKEN").ok();
        let orig_gh = std::env::var("GITHUB_TOKEN").ok();

        env::remove_var("MISE_GITHUB_TOKEN");
        env::remove_var("GITHUB_API_TOKEN");
        env::set_var("GITHUB_TOKEN", "ghp_test");

        let result = test_fn();

        match orig_mise {
            Some(v) => env::set_var("MISE_GITHUB_TOKEN", v),
            None => env::remove_var("MISE_GITHUB_TOKEN"),
        }
        match orig_api {
            Some(v) => env::set_var("GITHUB_API_TOKEN", v),
            None => env::remove_var("GITHUB_API_TOKEN"),
        }
        match orig_gh {
            Some(v) => env::set_var("GITHUB_TOKEN", v),
            None => env::remove_var("GITHUB_TOKEN"),
        }

        result
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

    #[test]
    fn test_resolve_pagination_url() {
        let base = "https://api.github.com/repos/jdx/aube/releases?per_page=100";
        assert_eq!(
            resolve_pagination_url(base, "/repos/jdx/aube/releases?page=2").unwrap(),
            "https://api.github.com/repos/jdx/aube/releases?page=2"
        );
        assert_eq!(
            resolve_pagination_url(
                base,
                "https://api.github.com/repos/jdx/aube/releases?page=2"
            )
            .unwrap(),
            "https://api.github.com/repos/jdx/aube/releases?page=2"
        );
    }

    fn make_release(tag: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_string(),
            draft: false,
            prerelease: false,
            created_at: String::new(),
            assets: vec![],
        }
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
}
