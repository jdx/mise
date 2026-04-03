use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::config::Settings;
use crate::tokens;
use crate::{dirs, duration, env};
use eyre::Result;
use heck::ToKebabCase;
use reqwest::IntoUrl;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_derive::{Deserialize, Serialize};
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
                .with_fresh_duration(Some(duration::DAILY))
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
                .with_fresh_duration(Some(duration::DAILY))
                .build()
        });
    RELEASE_CACHE.read().await
}

pub async fn list_releases_from_url(api_url: &str, repo: &str) -> Result<Vec<ForgejoRelease>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || list_releases_(api_url, repo).await)
        .await?
        .to_vec())
}

async fn list_releases_(api_url: &str, repo: &str) -> Result<Vec<ForgejoRelease>> {
    let url = format!("{api_url}/repos/{repo}/releases");
    let headers = get_headers(&url);
    let (mut releases, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<ForgejoRelease>, _>(url, &headers)
        .await?;

    if *env::MISE_LIST_ALL_VERSIONS {
        while let Some(next) = next_page(&headers) {
            headers = get_headers(&next);
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<ForgejoRelease>, _>(next, &headers)
                .await?;
            releases.extend(more);
            headers = h;
        }
    }
    releases.retain(|r| !r.draft && !r.prerelease);

    Ok(releases)
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

    let lookup_host = canonical_host(url.host_str()).unwrap_or("codeberg.org");
    if let Some((token, _source)) = resolve_token(lookup_host) {
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

fn canonical_host(host: Option<&str>) -> Option<&str> {
    host
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
    if let Some(token) = std::env::var("MISE_FORGEJO_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        return Some((token, TokenSource::EnvVar("MISE_FORGEJO_TOKEN")));
    }
    if let Some(token) = env::FORGEJO_TOKEN
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        return Some((token.to_string(), TokenSource::EnvVar("FORGEJO_TOKEN")));
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
        let macos_path = dirs::HOME.join("Library/Application Support/forgejo-cli/keys.json");
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
}
