use crate::config::Settings;
use crate::tokens;
use eyre::Result;
use heck::ToKebabCase;
use reqwest::IntoUrl;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use tokio::sync::{RwLock, RwLockReadGuard};
use xx::regex;

use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::{dirs, duration, env};

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

async fn get_tags_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<Vec<String>>> {
    TAGS_CACHE
        .write()
        .await
        .entry(key.to_string())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}-tags.msgpack.z")))
                .with_fresh_duration(Some(duration::DAILY))
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
                .with_fresh_duration(Some(duration::DAILY))
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
                .with_fresh_duration(Some(duration::DAILY))
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
        .get_or_try_init_async(async || list_releases_(API_URL, repo).await)
        .await?
        .to_vec())
}

pub async fn list_releases_from_url(api_url: &str, repo: &str) -> Result<Vec<GitlabRelease>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || list_releases_(api_url, repo).await)
        .await?
        .to_vec())
}

async fn list_releases_(api_url: &str, repo: &str) -> Result<Vec<GitlabRelease>> {
    let url = format!(
        "{}/projects/{}/releases",
        api_url,
        urlencoding::encode(repo)
    );

    let headers = get_headers(&url);
    let (mut releases, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GitlabRelease>, _>(url, &headers)
        .await?;

    if *env::MISE_LIST_ALL_VERSIONS {
        while let Some(next) = next_page(&headers) {
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GitlabRelease>, _>(next, &headers)
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
    let url = format!(
        "{}/projects/{}/repository/tags",
        api_url,
        urlencoding::encode(repo)
    );
    let headers = get_headers(&url);
    let (mut tags, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GitlabTag>, _>(url, &headers)
        .await?;

    if *env::MISE_LIST_ALL_VERSIONS {
        while let Some(next) = next_page(&headers) {
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GitlabTag>, _>(next, &headers)
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
    dirs::CACHE.join("gitlab")
}

pub fn get_headers<U: IntoUrl>(url: U) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let url = url.into_url().unwrap();
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

/// Returns true if the given hostname has a token available from a non-env-var source.
pub fn is_gitlab_host(host: &str) -> bool {
    MISE_GITLAB_TOKENS.contains_key(host)
        || (Settings::get().gitlab.glab_cli_tokens && GLAB_HOSTS.contains_key(host))
}

// ── gitlab_tokens.toml ─────────────────────────────────────────────

static MISE_GITLAB_TOKENS: Lazy<HashMap<String, String>> = Lazy::new(|| {
    tokens::read_tokens_toml("gitlab_tokens.toml", "gitlab_tokens.toml").unwrap_or_default()
});

// ── glab CLI config.yml ────────────────────────────────────────────

static GLAB_HOSTS: Lazy<HashMap<String, String>> =
    Lazy::new(|| read_glab_hosts().unwrap_or_default());

fn glab_config_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("GLAB_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("config.yml"));
    }

    let xdg_path = env::XDG_CONFIG_HOME.join("glab-cli/config.yml");
    if xdg_path.exists() {
        return Some(xdg_path);
    }

    #[cfg(target_os = "macos")]
    {
        let macos_path = dirs::HOME.join("Library/Application Support/glab-cli/config.yml");
        if macos_path.exists() {
            return Some(macos_path);
        }
    }

    Some(xdg_path)
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
    match tokens::yaml_hosts_to_tokens(&contents) {
        Some(tokens) => Some(tokens),
        None => {
            debug!("failed to parse glab config.yml at {}", path.display());
            None
        }
    }
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
}
