use crate::config::Settings;
use crate::tokens;
use eyre::Result;
use heck::ToKebabCase;
use reqwest::IntoUrl;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use tokio::sync::{RwLock, RwLockReadGuard};
use xx::regex;

use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::{dirs, env};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitlabRelease {
    pub tag_name: String,
    pub description: Option<String>,
    pub released_at: Option<String>,
    pub assets: GitlabAssets,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitlabTag {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitlabAssets {
    // pub count: i64,
    pub sources: Vec<GitlabAssetSource>,
    pub links: Vec<GitlabAssetLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitlabAssetSource {
    pub format: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitlabAssetLink {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub direct_asset_url: String,
    pub link_type: String,
}

type CacheGroup<T> = HashMap<String, CacheManager<T>>;

static RELEASES_CACHE: Lazy<RwLock<CacheGroup<Vec<GitlabRelease>>>> = Lazy::new(Default::default);

static RELEASE_CACHE: Lazy<RwLock<CacheGroup<GitlabRelease>>> = Lazy::new(Default::default);

static TAGS_CACHE: Lazy<RwLock<CacheGroup<Vec<String>>>> = Lazy::new(Default::default);

pub static API_URL: &str = "https://gitlab.com/api/v4";

pub static API_PATH: &str = "/api/v4";

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

async fn get_releases_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<Vec<GitlabRelease>>> {
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

async fn get_release_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<GitlabRelease>> {
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

#[allow(dead_code)]
pub async fn list_releases(repo: &str) -> Result<Vec<GitlabRelease>> {
    let key = repo.to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || {
            list_releases_(API_URL, repo, *env::MISE_LIST_ALL_VERSIONS).await
        })
        .await?
        .to_vec())
}

pub async fn list_releases_from_url(api_url: &str, repo: &str) -> Result<Vec<GitlabRelease>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || {
            list_releases_(api_url, repo, *env::MISE_LIST_ALL_VERSIONS).await
        })
        .await?
        .to_vec())
}

/// `list_all` is `MISE_LIST_ALL_VERSIONS`, taken as an argument rather than read here so
/// tests can exercise the pagination loop: the env var is a process-wide `Lazy` that other
/// modules' tests have already forced by the time this one runs.
async fn list_releases_(api_url: &str, repo: &str, list_all: bool) -> Result<Vec<GitlabRelease>> {
    let mut url = format!(
        "{}/projects/{}/releases?per_page=100",
        api_url,
        urlencoding::encode(repo)
    );

    let headers = get_headers(&url, api_url);
    let (mut releases, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GitlabRelease>, _>(&url, &headers)
        .await?;

    if list_all {
        while let Some(next) = next_page(&headers) {
            url = crate::http::resolve_pagination_url(&url, &next)?;
            // Re-derive auth for every page. `headers` holds the *response* headers of the
            // previous page at this point (that is how `next_page` reads `Link`), and
            // `json_headers_with_headers` bypasses the automatic host auth, so reusing it
            // would send page 2 onward unauthenticated. Same defect github had (#6318).
            headers = get_headers(&url, api_url);
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GitlabRelease>, _>(&url, &headers)
                .await?;
            releases.extend(more);
            headers = h;
        }
    }

    Ok(releases)
}

#[allow(dead_code)]
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

/// See [`list_releases_`] for why `list_all` is a parameter.
async fn list_tags_(api_url: &str, repo: &str, list_all: bool) -> Result<Vec<String>> {
    let mut url = format!(
        "{}/projects/{}/repository/tags?per_page=100",
        api_url,
        urlencoding::encode(repo)
    );
    let headers = get_headers(&url, api_url);
    let (mut tags, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GitlabTag>, _>(&url, &headers)
        .await?;

    if list_all {
        while let Some(next) = next_page(&headers) {
            url = crate::http::resolve_pagination_url(&url, &next)?;
            // Re-derive auth for every page — see the comment in `list_releases_`.
            headers = get_headers(&url, api_url);
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GitlabTag>, _>(&url, &headers)
                .await?;
            tags.extend(more);
            headers = h;
        }
    }

    Ok(tags.into_iter().map(|t| t.name).collect())
}

#[allow(dead_code)]
pub async fn get_release(repo: &str, tag: &str) -> Result<GitlabRelease> {
    let key = format!("{repo}-{tag}").to_kebab_case();
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || get_release_(API_URL, repo, tag).await)
        .await?
        .clone())
}

pub async fn get_release_for_url(api_url: &str, repo: &str, tag: &str) -> Result<GitlabRelease> {
    let key = format!("{api_url}-{repo}-{tag}").to_kebab_case();
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || get_release_(api_url, repo, tag).await)
        .await?
        .clone())
}

async fn get_release_(api_url: &str, repo: &str, tag: &str) -> Result<GitlabRelease> {
    let url = format!(
        "{}/projects/{}/releases/{}",
        api_url,
        urlencoding::encode(repo),
        tag
    );
    let headers = get_headers(&url, api_url);
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
    dirs::CACHE.join("gitlab")
}

pub fn get_headers<U: IntoUrl>(url: U, api_url: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    // An invalid URL just means no auth headers; the real error surfaces when the
    // request is made. Avoid panicking here. See #3547.
    let Ok(url) = url.into_url() else {
        return headers;
    };
    let Ok(api_url) = reqwest::Url::parse(api_url) else {
        return headers;
    };
    if url.origin() != api_url.origin() {
        return headers;
    }
    let lookup_host = url.host_str().unwrap_or("gitlab.com");

    if let Some((token, _source)) = resolve_token(lookup_host) {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(format!("Bearer {token}").as_str()).unwrap(),
        );
    }

    headers
}

/// The source from which a GitLab token was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    EnvVar(&'static str),
    TokensFile,
    GlabCli,
    CredentialCommand,
    GitCredential,
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenSource::EnvVar(name) => write!(f, "{name}"),
            TokenSource::TokensFile => write!(f, "gitlab_tokens.toml"),
            TokenSource::GlabCli => write!(f, "glab CLI (config.yml)"),
            TokenSource::CredentialCommand => write!(f, "credential_command"),
            TokenSource::GitCredential => write!(f, "git credential fill"),
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Test-only hook that lets tests seed a token without mutating the environment.
    //! Only consulted from [`super::resolve_token`] under `#[cfg(test)]`; production builds
    //! never see it. Deliberately checked *before* the env-var branch so a developer's
    //! ambient `GITLAB_TOKEN` / `MISE_GITLAB_ENTERPRISE_TOKEN` cannot change the outcome.
    //! Mirrors the equivalent in [`crate::github`].

    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Overrides the `gitlab_tokens.toml` source in [`super::resolve_token`], keyed by host.
    pub(crate) static TOKENS_FILE_OVERRIDE: RwLock<Option<HashMap<String, String>>> =
        RwLock::new(None);

    pub(crate) fn lookup_tokens_file_override(host: &str) -> Option<String> {
        let guard = TOKENS_FILE_OVERRIDE.read().ok()?;
        guard.as_ref()?.get(host).cloned()
    }
}

/// Resolve the GitLab token for the given hostname.
///
/// Priority:
/// 1. `MISE_GITLAB_ENTERPRISE_TOKEN` env var (non-gitlab.com only)
/// 2. `MISE_GITLAB_TOKEN` / `GITLAB_TOKEN` env vars
/// 3. `credential_command` (if set)
/// 4. `gitlab_tokens.toml` (per-host)
/// 5. glab CLI token (from `config.yml`)
/// 6. `git credential fill` (if enabled)
pub fn resolve_token(host: &str) -> Option<(String, TokenSource)> {
    #[cfg(test)]
    if let Some(token) = test_support::lookup_tokens_file_override(host) {
        return Some((token, TokenSource::TokensFile));
    }

    let settings = Settings::get();
    let is_gitlab_com = host == "gitlab.com";

    // 1. Enterprise token (non-gitlab.com only)
    if !is_gitlab_com && let Some(token) = env::MISE_GITLAB_ENTERPRISE_TOKEN.as_deref() {
        return Some((
            token.to_string(),
            TokenSource::EnvVar("MISE_GITLAB_ENTERPRISE_TOKEN"),
        ));
    }

    // 2. Standard env vars
    for var_name in &["MISE_GITLAB_TOKEN", "GITLAB_TOKEN"] {
        if let Some(token) = std::env::var(var_name)
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        {
            return Some((token, TokenSource::EnvVar(var_name)));
        }
    }

    // 3. credential_command
    let credential_command = &settings.gitlab.credential_command;
    if !credential_command.is_empty()
        && let Some(token) =
            tokens::get_credential_command_token("gitlab", credential_command, host)
    {
        return Some((token, TokenSource::CredentialCommand));
    }

    // 4. gitlab_tokens.toml
    if let Some(token) = MISE_GITLAB_TOKENS.get(host) {
        return Some((token.clone(), TokenSource::TokensFile));
    }

    // 5. glab CLI config.yml
    if settings.gitlab.glab_cli_tokens
        && let Some(token) = GLAB_HOSTS.get(host)
    {
        return Some((token.clone(), TokenSource::GlabCli));
    }

    // 6. git credential fill
    if settings.gitlab.use_git_credentials
        && let Some(token) = tokens::get_git_credential_token("gitlab", host)
    {
        return Some((token, TokenSource::GitCredential));
    }

    None
}

// ── gitlab_tokens.toml ─────────────────────────────────────────────

static MISE_GITLAB_TOKENS: Lazy<HashMap<String, String>> = Lazy::new(|| {
    tokens::read_tokens_toml("gitlab_tokens.toml", "gitlab_tokens.toml").unwrap_or_default()
});

// ── glab CLI config.yml ────────────────────────────────────────────

static GLAB_HOSTS: Lazy<HashMap<String, String>> =
    Lazy::new(|| read_glab_hosts().unwrap_or_default());

/// Resolve the path to glab's config.yml, following glab's own `ConfigDir()`:
/// 1. `$GLAB_CONFIG_DIR/config.yml`
/// 2. `$HOME/.config/glab-cli/config.yml` — glab's *legacy* location, which it still prefers when
///    the file is there. glab's `legacyConfigDir()` is `os.UserHomeDir()` + `.config/glab-cli` on
///    every platform, so it is deliberately derived from `HOME` and **not** from
///    `XDG_CONFIG_HOME`: those differ as soon as the user sets that variable.
/// 3. `xdg.ConfigHome/glab-cli/config.yml`, which on Windows is under `%LOCALAPPDATA%`: glab uses
///    the `adrg/xdg` package, and that maps `XDG_CONFIG_HOME` to `%LOCALAPPDATA%` there, not to
///    `%APPDATA%` the way `gh` does
fn glab_config_path() -> Option<PathBuf> {
    // Empty means unset, as in glab's `if glabDir != ""`; `var_os` rather than `var` so a
    // non-UTF-8 directory is honoured instead of silently skipped.
    if let Some(dir) = std::env::var_os("GLAB_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join("config.yml"));
    }

    let xdg_path = env::XDG_CONFIG_HOME.join("glab-cli/config.yml");
    let candidates: Vec<PathBuf> = [
        dirs::HOME.join(".config/glab-cli/config.yml"),
        xdg_path.clone(),
    ]
    .into_iter()
    .chain(glab_native_config_paths())
    .collect();
    // Nothing found: name the location glab itself would settle on, which is the last candidate
    // (its platform-native XDG config dir).
    let fallback = candidates.last().cloned().unwrap_or(xdg_path);
    Some(tokens::first_existing_file(candidates, fallback))
}

/// Platform-native locations glab may have written to, probed after the legacy/XDG one.
#[cfg(target_os = "macos")]
fn glab_native_config_paths() -> Vec<PathBuf> {
    vec![dirs::HOME.join("Library/Application Support/glab-cli/config.yml")]
}

#[cfg(windows)]
fn glab_native_config_paths() -> Vec<PathBuf> {
    vec![env::LOCAL_APPDATA.join("glab-cli/config.yml")]
}

#[cfg(all(not(target_os = "macos"), not(windows)))]
fn glab_native_config_paths() -> Vec<PathBuf> {
    Vec::new()
}

fn read_glab_hosts() -> Option<HashMap<String, String>> {
    let path = glab_config_path()?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            trace!("glab config.yml not readable at {}: {e}", path.display());
            return None;
        }
    };
    warn_glab_expired_tokens(&contents);
    match tokens::yaml_hosts_to_tokens(&contents) {
        Some(tokens) => Some(tokens),
        None => {
            debug!("failed to parse glab config.yml at {}", path.display());
            None
        }
    }
}

/// Warn if any glab OAuth2 tokens are expired.
///
/// glab stores `oauth2_expiry_date` alongside `oauth2_refresh_token`. Current glab
/// versions write RFC3339; older versions used RFC822. We only check RFC3339 since
/// that is the correct format going forward--old tokens will simply not trigger the
/// warning. mise cannot refresh OAuth2 tokens itself, so we warn the user to run a
/// glab command (e.g. `glab api user`) which will trigger a silent token refresh.
fn warn_glab_expired_tokens(contents: &str) {
    for (host, expiry_str) in find_expired_glab_tokens(contents) {
        warn!(
            "glab OAuth2 token for {host} expired at {expiry_str}. Run a glab command (e.g. `glab api user`) to refresh it."
        );
    }
}

/// Returns `(host, expiry_str)` pairs for every glab host whose OAuth2 token is expired.
fn find_expired_glab_tokens(contents: &str) -> Vec<(String, String)> {
    let Ok(yaml) = serde_yaml::from_str::<Value>(contents) else {
        return vec![];
    };
    let Some(hosts) = yaml.get("hosts").and_then(Value::as_mapping) else {
        return vec![];
    };

    let mut expired = vec![];
    let now = chrono::Utc::now();
    for (k, entry) in hosts {
        let Some(host) = k.as_str() else { continue };
        if entry.get("oauth2_refresh_token").is_none() {
            continue;
        }
        let Some(expiry_str) = entry.get("oauth2_expiry_date").and_then(Value::as_str) else {
            continue;
        };
        let Ok(expiry_date) = chrono::DateTime::parse_from_rfc3339(expiry_str) else {
            continue;
        };
        if expiry_date < now {
            expired.push((host.to_string(), expiry_str.to_string()));
        }
    }
    expired
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gitlab_tokens() {
        let toml = r#"
[tokens."gitlab.com"]
token = "glpat_abc123"

[tokens."gitlab.mycompany.com"]
token = "glpat_def456"
"#;
        let result = tokens::parse_tokens_toml(toml).unwrap();
        assert_eq!(result.get("gitlab.com").unwrap(), "glpat_abc123");
        assert_eq!(result.get("gitlab.mycompany.com").unwrap(), "glpat_def456");
    }

    #[test]
    fn test_parse_gitlab_tokens_empty() {
        assert!(tokens::parse_tokens_toml("").is_none());
    }

    #[test]
    fn test_parse_gitlab_tokens_empty_tokens() {
        let toml = "[tokens]\n";
        let result = tokens::parse_tokens_toml(toml).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_gitlab_tokens_missing_token_field() {
        let toml = r#"
[tokens."gitlab.com"]
something_else = "value"
"#;
        let result = tokens::parse_tokens_toml(toml).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_expired_glab_tokens_expired() {
        let yaml = r#"
hosts:
  gitlab.com:
    oauth_token: gloas-abc123
    oauth2_refresh_token: refresh_token
    oauth2_expiry_date: "2023-03-13T15:47:00Z"
"#;
        let expired = find_expired_glab_tokens(yaml);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, "gitlab.com");
        assert_eq!(expired[0].1, "2023-03-13T15:47:00Z");
    }

    #[test]
    fn test_find_expired_glab_tokens_not_expired() {
        let yaml = r#"
hosts:
  gitlab.com:
    oauth_token: gloas-abc123
    oauth2_refresh_token: refresh_token
    oauth2_expiry_date: "2050-01-01T00:00:00Z"
"#;
        let expired = find_expired_glab_tokens(yaml);
        assert!(expired.is_empty());
    }

    #[test]
    fn test_find_expired_glab_tokens_no_expiry_field() {
        // PATs have no expiry date--should not be flagged
        let yaml = r#"
hosts:
  gitlab.com:
    token: glpat-abc123
"#;
        let expired = find_expired_glab_tokens(yaml);
        assert!(expired.is_empty());
    }

    #[test]
    fn test_find_expired_glab_tokens_multiple_hosts() {
        let yaml = r#"
hosts:
  gitlab.com:
    oauth_token: gloas-abc123
    oauth2_refresh_token: refresh1
    oauth2_expiry_date: "2023-03-13T15:47:00Z"
  gitlab.mycompany.com:
    oauth_token: gloas-def456
    oauth2_refresh_token: refresh2
    oauth2_expiry_date: "2050-01-01T00:00:00Z"
"#;
        let expired = find_expired_glab_tokens(yaml);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, "gitlab.com");
    }

    #[test]
    fn test_find_expired_glab_tokens_old_format_skipped() {
        // Old RFC822 format is not parsed--no false positives
        let yaml = r#"
hosts:
  gitlab.com:
    oauth_token: gloas-abc123
    oauth2_expiry_date: "13 Mar 23 15:47 GMT"
"#;
        let expired = find_expired_glab_tokens(yaml);
        assert!(expired.is_empty());
    }

    #[test]
    fn test_find_expired_glab_tokens_invalid_date() {
        let yaml = r#"
hosts:
  gitlab.com:
    oauth_token: gloas-abc123
    oauth2_expiry_date: "not-a-date"
"#;
        let expired = find_expired_glab_tokens(yaml);
        assert!(expired.is_empty());
    }

    #[test]
    fn test_find_expired_glab_tokens_no_refresh_token_skipped() {
        // No oauth2_refresh_token means reauthentication is needed, not a refresh—don't warn.
        let yaml = r#"
hosts:
  gitlab.com:
    oauth_token: gloas-abc123
    oauth2_expiry_date: "2023-03-13T15:47:00Z"
"#;
        let expired = find_expired_glab_tokens(yaml);
        assert!(expired.is_empty());
    }

    #[test]
    fn test_find_expired_glab_tokens_empty() {
        assert!(find_expired_glab_tokens("").is_empty());
        assert!(find_expired_glab_tokens("hosts: {}").is_empty());
    }

    const TEST_TOKEN: &str = "glpat_paginate_test";

    /// Seeds a token for `host` for the lifetime of the guard.
    struct TokensFileOverrideGuard;

    impl TokensFileOverrideGuard {
        fn set(host: &str) -> Self {
            let mut tokens = HashMap::new();
            tokens.insert(host.to_string(), TEST_TOKEN.to_string());
            *test_support::TOKENS_FILE_OVERRIDE.write().unwrap() = Some(tokens);
            Self
        }
    }

    impl Drop for TokensFileOverrideGuard {
        fn drop(&mut self) {
            *test_support::TOKENS_FILE_OVERRIDE.write().unwrap() = None;
        }
    }

    fn host_of(url: &str) -> String {
        url::Url::parse(url)
            .unwrap()
            .host_str()
            .unwrap()
            .to_string()
    }

    fn tag_json(name: &str) -> serde_json::Value {
        serde_json::json!({ "name": name })
    }

    fn release_json(tag: &str) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "description": null,
            "released_at": null,
            "assets": { "sources": [], "links": [] },
        })
    }

    #[test]
    fn test_get_headers_only_authenticates_api_origin() {
        let api_url = "https://gitlab.example.com/api/v4";
        let _token = TokensFileOverrideGuard::set("gitlab.example.com");

        let headers = get_headers(
            "https://gitlab.example.com/releases/download/tool.tar.gz",
            api_url,
        );
        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).unwrap(),
            format!("Bearer {TEST_TOKEN}").as_str()
        );

        let headers = get_headers("https://downloads.example.com/tool.tar.gz", api_url);
        assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));

        let headers = get_headers("http://gitlab.example.com/api/v4/page2", api_url);
        assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
    }

    // Regression: every paginated request must carry the Authorization header. Before the
    // fix, page 2 was sent page 1's *response* headers, so it went out unauthenticated and
    // hit the anonymous rate limit on private/rate-limited projects. github had the same
    // defect until #6318; gitlab never got that fix.
    #[tokio::test]
    async fn test_list_releases_sends_auth_on_every_page() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let _token = TokensFileOverrideGuard::set(&host_of(&base));
        let auth = format!("Bearer {TEST_TOKEN}");

        // Regex rather than an exact path: the project is percent-encoded into the path
        // (`owner%2Frepo`) and how that is normalized before matching is not this test's
        // subject -- the Authorization header on page 2 is.
        let page1 = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/projects/.+/releases$".to_string()),
            )
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{base}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::json!([release_json("v2.0.0")]).to_string())
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", "/page2")
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!([release_json("v1.0.0")]).to_string())
            .expect(1)
            .create_async()
            .await;

        let releases = list_releases_(&base, "owner/repo", true).await.unwrap();
        page1.assert_async().await;
        page2.assert_async().await;
        assert_eq!(
            releases
                .iter()
                .map(|r| r.tag_name.as_str())
                .collect::<Vec<_>>(),
            ["v2.0.0", "v1.0.0"]
        );
    }

    // Same regression for the tags loop -- see `test_list_releases_sends_auth_on_every_page`.
    #[tokio::test]
    async fn test_list_tags_sends_auth_on_every_page() {
        let _config = crate::config::Config::get().await.unwrap();
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let _token = TokensFileOverrideGuard::set(&host_of(&base));
        let auth = format!("Bearer {TEST_TOKEN}");

        let page1 = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/projects/.+/repository/tags$".to_string()),
            )
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{base}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::json!([tag_json("v2.0.0")]).to_string())
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", "/page2")
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!([tag_json("v1.0.0")]).to_string())
            .expect(1)
            .create_async()
            .await;

        let tags = list_tags_(&base, "owner/repo", true).await.unwrap();
        page1.assert_async().await;
        page2.assert_async().await;
        assert_eq!(tags, ["v2.0.0", "v1.0.0"]);
    }
}
