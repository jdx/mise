use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::config::Settings;
use crate::tokens;
use crate::{dirs, env};
use eyre::Result;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoRelease {
    pub id: u64,
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    pub assets: Vec<ForgejoAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoAsset {
    pub id: u64,
    pub name: String,
    // pub size: u64,
    pub uuid: String,
    pub browser_download_url: String,
}

type CacheGroup<T> = HashMap<String, CacheManager<T>>;

static RELEASES_CACHE: Lazy<RwLock<CacheGroup<Vec<ForgejoRelease>>>> = Lazy::new(Default::default);

static RELEASE_CACHE: Lazy<RwLock<CacheGroup<ForgejoRelease>>> = Lazy::new(Default::default);

async fn get_releases_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<Vec<ForgejoRelease>>> {
    RELEASES_CACHE
        .write()
        .await
        .entry(key.to_string())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}-releases.msgpack.z")))
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build()
        });
    RELEASES_CACHE.read().await
}

async fn get_release_cache<'a>(key: &str) -> RwLockReadGuard<'a, CacheGroup<ForgejoRelease>> {
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

/// Lists releases, including releases flagged `prerelease: true`. Drafts are
/// always filtered out. The cache stores this non-draft superset so callers can
/// apply the current `prerelease` option at read time without invalidating
/// cached release metadata.
pub async fn list_releases_including_prereleases_from_url(
    api_url: &str,
    repo: &str,
) -> Result<Vec<ForgejoRelease>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || list_releases_(api_url, repo).await)
        .await?
        .to_vec())
}

/// See the constant of the same name in [`crate::github`]: bound the prerelease
/// fallback pagination so a repo full of nightly prereleases still surfaces a stable
/// release without unbounded API calls. (#10343)
const MAX_RELEASE_FALLBACK_PAGES: usize = 3;

async fn list_releases_(api_url: &str, repo: &str) -> Result<Vec<ForgejoRelease>> {
    let url = format!("{api_url}/repos/{repo}/releases?limit=100");
    let headers = get_headers(&url);
    let (mut releases, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<ForgejoRelease>, _>(url, &headers)
        .await?;

    // Fetch additional pages when MISE_LIST_ALL_VERSIONS is set, or (bounded) while
    // every release seen so far is a prerelease/draft, mirroring src/github.rs. (#10343)
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
        headers = get_headers(&next);
        let (more, h) = crate::http::HTTP_FETCH
            .json_headers_with_headers::<Vec<ForgejoRelease>, _>(next, &headers)
            .await?;
        releases.extend(more);
        headers = h;
        pages_fetched += 1;
    }
    releases.retain(is_published_release);

    Ok(releases)
}

fn is_published_release(release: &ForgejoRelease) -> bool {
    !release.draft
}

pub async fn get_release_for_url(api_url: &str, repo: &str, tag: &str) -> Result<ForgejoRelease> {
    let key = format!("{api_url}-{repo}-{tag}").to_kebab_case();
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || get_release_(api_url, repo, tag).await)
        .await?
        .clone())
}

async fn get_release_(api_url: &str, repo: &str, tag: &str) -> Result<ForgejoRelease> {
    let url = if tag == "latest" {
        format!("{api_url}/repos/{repo}/releases/latest")
    } else {
        format!("{api_url}/repos/{repo}/releases/tags/{tag}")
    };
    let headers = get_headers(&url);
    crate::http::HTTP_FETCH
        .json_with_headers(url, &headers)
        .await
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
    dirs::CACHE.join("forgejo")
}

pub fn get_headers<U: IntoUrl>(url: U) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let url = url.into_url().unwrap();

    if let Some((token, _source)) = resolve_token(url.host_str().unwrap_or("codeberg.org")) {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(format!("Bearer {token}").as_str()).unwrap(),
        );
    }

    headers
}

/// The source from which a Forgejo token was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    EnvVar(&'static str),
    TokensFile,
    FjCli,
    CredentialCommand,
    GitCredential,
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenSource::EnvVar(name) => write!(f, "{name}"),
            TokenSource::TokensFile => write!(f, "forgejo_tokens.toml"),
            TokenSource::FjCli => write!(f, "fj CLI (keys.json)"),
            TokenSource::CredentialCommand => write!(f, "credential_command"),
            TokenSource::GitCredential => write!(f, "git credential fill"),
        }
    }
}

/// Resolve the Forgejo token for the given hostname.
///
/// Priority:
/// 1. `MISE_FORGEJO_ENTERPRISE_TOKEN` env var (non-codeberg.org only)
/// 2. `MISE_FORGEJO_TOKEN` / `FORGEJO_TOKEN` env vars
/// 3. `credential_command` (if set)
/// 4. `forgejo_tokens.toml` (per-host)
/// 5. fj CLI token (from `keys.json`)
/// 6. `git credential fill` (if enabled)
pub fn resolve_token(host: &str) -> Option<(String, TokenSource)> {
    let settings = Settings::get();
    let is_codeberg = host == "codeberg.org";

    // 1. Enterprise token (non-codeberg.org only)
    if !is_codeberg && let Some(token) = env::MISE_FORGEJO_ENTERPRISE_TOKEN.as_deref() {
        return Some((
            token.to_string(),
            TokenSource::EnvVar("MISE_FORGEJO_ENTERPRISE_TOKEN"),
        ));
    }

    // 2. Standard env vars
    for var_name in &["MISE_FORGEJO_TOKEN", "FORGEJO_TOKEN"] {
        if let Some(tok) = std::env::var(var_name)
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        {
            return Some((tok, TokenSource::EnvVar(var_name)));
        }
    }

    // 3. credential_command
    let credential_command = &settings.forgejo.credential_command;
    if !credential_command.is_empty()
        && let Some(token) =
            tokens::get_credential_command_token("forgejo", credential_command, host)
    {
        return Some((token, TokenSource::CredentialCommand));
    }

    // 4. forgejo_tokens.toml
    if let Some(token) = MISE_FORGEJO_TOKENS.get(host) {
        return Some((token.clone(), TokenSource::TokensFile));
    }

    // 5. fj CLI keys.json
    if settings.forgejo.fj_cli_tokens
        && let Some(token) = FJ_HOSTS.get(host)
    {
        return Some((token.clone(), TokenSource::FjCli));
    }

    // 6. git credential fill
    if settings.forgejo.use_git_credentials
        && let Some(token) = tokens::get_git_credential_token("forgejo", host)
    {
        return Some((token, TokenSource::GitCredential));
    }

    None
}

/// Returns true if the given hostname has a token available from a non-env-var source.
pub fn is_forgejo_host(host: &str) -> bool {
    MISE_FORGEJO_TOKENS.contains_key(host)
        || (Settings::get().forgejo.fj_cli_tokens && FJ_HOSTS.contains_key(host))
}

// ── forgejo_tokens.toml ────────────────────────────────────────────

static MISE_FORGEJO_TOKENS: Lazy<HashMap<String, String>> = Lazy::new(|| {
    tokens::read_tokens_toml("forgejo_tokens.toml", "forgejo_tokens.toml").unwrap_or_default()
});

// ── fj CLI keys.json ──────────────────────────────────────────────

static FJ_HOSTS: Lazy<HashMap<String, String>> = Lazy::new(|| read_fj_hosts().unwrap_or_default());

fn fj_keys_path() -> Option<PathBuf> {
    // Linux/XDG: $XDG_DATA_HOME/forgejo-cli/keys.json
    let xdg_path = env::XDG_DATA_HOME.join("forgejo-cli/keys.json");
    if xdg_path.exists() {
        return Some(xdg_path);
    }

    #[cfg(target_os = "macos")]
    {
        let macos_path =
            dirs::HOME.join("Library/Application Support/Cyborus.forgejo-cli/keys.json");
        if macos_path.exists() {
            return Some(macos_path);
        }
    }

    Some(xdg_path)
}

fn read_fj_hosts() -> Option<HashMap<String, String>> {
    let path = fj_keys_path()?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            trace!("fj keys.json not readable at {}: {e}", path.display());
            return None;
        }
    };
    match parse_fj_keys(&contents) {
        Some(tokens) => Some(tokens),
        None => {
            debug!("failed to parse fj keys.json at {}", path.display());
            None
        }
    }
}

/// Parse `fj` CLI `keys.json` into a host→token map.
///
/// The file schema is:
/// ```json
/// {
///   "hosts": {
///     "codeberg.org": { "type": "Application", "name": "user", "token": "abc" },
///     "codeberg.org": { "type": "OAuth", "name": "user", "token": "abc", ... }
///   }
/// }
/// ```
fn parse_fj_keys(contents: &str) -> Option<HashMap<String, String>> {
    #[derive(serde::Deserialize)]
    struct FjKeys {
        hosts: Option<HashMap<String, FjLogin>>,
    }
    #[derive(serde::Deserialize)]
    struct FjLogin {
        token: Option<String>,
    }
    let keys: FjKeys = serde_json::from_str(contents).ok()?;
    let hosts = keys.hosts?;
    let map: HashMap<String, String> = hosts
        .into_iter()
        .filter_map(|(host, login)| login.token.map(|t| (host, t)))
        .collect();
    if map.is_empty() { None } else { Some(map) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag_name: &str, draft: bool, prerelease: bool) -> ForgejoRelease {
        ForgejoRelease {
            id: 1,
            tag_name: tag_name.to_string(),
            draft,
            prerelease,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            assets: vec![],
        }
    }

    #[test]
    fn test_is_published_release_keeps_prereleases() {
        assert!(is_published_release(&release("1.0.0", false, false)));
        assert!(is_published_release(&release("1.1.0-rc1", false, true)));
        assert!(!is_published_release(&release("1.2.0", true, false)));
        assert!(!is_published_release(&release("1.2.0-rc1", true, true)));
    }

    #[test]
    fn test_parse_forgejo_tokens() {
        let toml = r#"
[tokens."codeberg.org"]
token = "abc123"

[tokens."forgejo.mycompany.com"]
token = "def456"
"#;
        let result = tokens::parse_tokens_toml(toml).unwrap();
        assert_eq!(result.get("codeberg.org").unwrap(), "abc123");
        assert_eq!(result.get("forgejo.mycompany.com").unwrap(), "def456");
    }

    #[test]
    fn test_parse_forgejo_tokens_empty() {
        assert!(tokens::parse_tokens_toml("").is_none());
    }

    #[test]
    fn test_parse_forgejo_tokens_empty_tokens() {
        let toml = "[tokens]\n";
        let result = tokens::parse_tokens_toml(toml).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_forgejo_tokens_missing_token_field() {
        let toml = r#"
[tokens."codeberg.org"]
something_else = "value"
"#;
        let result = tokens::parse_tokens_toml(toml).unwrap();
        assert!(result.is_empty());
    }

    // #10343: a first page made up entirely of prereleases must not yield "no
    // versions found" -- the fallback (bounded) follows the Link header to a later
    // page that has a stable release. Forgejo paginates with limit=100.
    #[tokio::test]
    async fn test_list_releases_paginates_past_all_prerelease_first_page() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let repo = "owner/all-prerelease-first-page";

        let page1 = vec![
            release("v2.0.0-alpha.2", false, true),
            release("v2.0.0-alpha.1", false, true),
        ];
        let page2 = vec![release("v1.0.0", false, false)];

        let page1_mock = server
            .mock("GET", format!("/repos/{repo}/releases").as_str())
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "100".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{base}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&page1).unwrap())
            .expect(1)
            .create_async()
            .await;
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
                .any(|r| r.tag_name == "v1.0.0" && !r.prerelease)
        );
    }

    // #10343: once a stable release is seen the fallback stops (no extra requests).
    #[tokio::test]
    async fn test_list_releases_stops_when_first_page_has_stable() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let repo = "owner/stable-on-first-page";

        let page1 = vec![
            release("v1.1.0-alpha.1", false, true),
            release("v1.0.0", false, false),
        ];

        let page1_mock = server
            .mock("GET", format!("/repos/{repo}/releases").as_str())
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "100".into()))
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

        let body = || serde_json::to_string(&vec![release("v9.0.0-alpha", false, true)]).unwrap();

        // Three all-prerelease pages, each linking to the next.
        let p1 = server
            .mock("GET", format!("/repos/{repo}/releases").as_str())
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "100".into()))
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
