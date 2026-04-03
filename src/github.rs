use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::config::Settings;
use crate::file::path_env_without_shims;
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

async fn get_releases_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<Vec<GithubRelease>>> {
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

async fn get_release_cache<'a>(key: &str) -> RwLockReadGuard<'a, CacheGroup<GithubRelease>> {
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

pub async fn list_releases(repo: &str) -> Result<Vec<GithubRelease>> {
    let key = repo.to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || list_releases_(API_URL, repo).await)
        .await?
        .to_vec())
}

pub async fn list_releases_from_url(api_url: &str, repo: &str) -> Result<Vec<GithubRelease>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || list_releases_(api_url, repo).await)
        .await?
        .to_vec())
}

async fn list_releases_(api_url: &str, repo: &str) -> Result<Vec<GithubRelease>> {
    let url = format!("{api_url}/repos/{repo}/releases");
    let headers = get_headers(&url);
    let (mut releases, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubRelease>, _>(url, &headers)
        .await?;

    if *env::MISE_LIST_ALL_VERSIONS {
        while let Some(next) = next_page(&headers) {
            headers = get_headers(&next);
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GithubRelease>, _>(next, &headers)
                .await?;
            releases.extend(more);
            headers = h;
        }
    }
    releases.retain(|r| !r.draft && !r.prerelease);

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
    let url = format!("{api_url}/repos/{repo}/tags");
    let headers = get_headers(&url);
    let (mut tags, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubTag>, _>(url, &headers)
        .await?;

    if *env::MISE_LIST_ALL_VERSIONS {
        while let Some(next) = next_page(&headers) {
            headers = get_headers(&next);
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GithubTag>, _>(next, &headers)
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
    let url = format!("{api_url}/repos/{repo}/tags");
    let headers = get_headers(&url);
    let (mut tags, mut response_headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubTag>, _>(url, &headers)
        .await?;

    // Fetch all pages when MISE_LIST_ALL_VERSIONS is set
    while let Some(next) = next_page(&response_headers) {
        response_headers = get_headers(&next);
        let (more, h) = crate::http::HTTP_FETCH
            .json_headers_with_headers::<Vec<GithubTag>, _>(next, &response_headers)
            .await?;
        tags.extend(more);
        response_headers = h;
    }

    // Fetch commit dates in parallel using the parallel utility
    let results = crate::parallel::parallel(tags, |tag| async move {
        let date = if let Some(commit) = tag.commit {
            let headers = get_headers(&commit.url);
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
    let key = format!("{repo}-{tag}").to_kebab_case();
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || get_release_(API_URL, repo, tag).await)
        .await?
        .clone())
}

pub async fn get_release_for_url(api_url: &str, repo: &str, tag: &str) -> Result<GithubRelease> {
    let key = format!("{api_url}-{repo}-{tag}").to_kebab_case();
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || get_release_(api_url, repo, tag).await)
        .await?
        .clone())
}

async fn get_release_(api_url: &str, repo: &str, tag: &str) -> Result<GithubRelease> {
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
    dirs::CACHE.join("github")
}

/// The source from which a GitHub token was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    EnvVar(&'static str),
    TokensFile,
    GhCli,
    CredentialCommand,
    GitCredential,
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenSource::EnvVar(name) => write!(f, "{name}"),
            TokenSource::TokensFile => write!(f, "github_tokens.toml"),
            TokenSource::GhCli => write!(f, "gh CLI (hosts.yml)"),
            TokenSource::CredentialCommand => write!(f, "credential_command"),
            TokenSource::GitCredential => write!(f, "git credential fill"),
        }
    }
}

/// Normalize a URL hostname to the canonical host used for token lookups.
/// Maps "api.github.com" and "*.githubusercontent.com" to "github.com".
fn canonical_host(host: Option<&str>) -> Option<&str> {
    match host {
        Some("api.github.com") => Some("github.com"),
        Some(h) if h.ends_with(".githubusercontent.com") => Some("github.com"),
        other => other,
    }
}

/// Resolve the GitHub token for the given hostname, returning the token and its source.
///
/// Priority:
/// 1. `MISE_GITHUB_ENTERPRISE_TOKEN` env var (non-github.com only)
/// 2. `MISE_GITHUB_TOKEN` / `GITHUB_API_TOKEN` / `GITHUB_TOKEN` env vars
/// 3. `credential_command` (if set)
/// 4. `github_tokens.toml` (per-host)
/// 5. gh CLI token (from `hosts.yml`)
/// 6. `git credential fill` (if enabled)
pub fn resolve_token(host: &str) -> Option<(String, TokenSource)> {
    let settings = Settings::get();

    let is_ghcom = host == "github.com"
        || host == "api.github.com"
        || host.ends_with(".githubusercontent.com");
    let lookup_host = if host == "api.github.com" || host.ends_with(".githubusercontent.com") {
        "github.com"
    } else {
        host
    };

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

    // 3. credential_command
    let credential_command = &settings.github.credential_command;
    if !credential_command.is_empty()
        && let Some(token) = get_credential_command_token(credential_command, lookup_host)
    {
        return Some((token, TokenSource::CredentialCommand));
    }

    // 4. github_tokens.toml
    if let Some(token) = MISE_GITHUB_TOKENS.get(lookup_host) {
        return Some((token.clone(), TokenSource::TokensFile));
    }

    // 5. gh CLI hosts.yml
    if settings.github.gh_cli_tokens
        && let Some(token) = GH_HOSTS.get(lookup_host)
    {
        return Some((token.clone(), TokenSource::GhCli));
    }

    // 6. git credential fill
    if settings.github.use_git_credentials
        && let Some(token) = get_git_credential_token(lookup_host)
    {
        return Some((token, TokenSource::GitCredential));
    }

    None
}

pub fn get_headers<U: IntoUrl>(url: U) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let url = url.into_url().unwrap();

    let host = url.host_str();
    let lookup_host = canonical_host(host).unwrap_or("github.com");

    if let Some((token, _source)) = resolve_token(lookup_host) {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(format!("Bearer {token}").as_str()).unwrap(),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );
    }

    if url.path().contains("/releases/assets/") {
        headers.insert(
            "accept",
            HeaderValue::from_static("application/octet-stream"),
        );
    }

    headers
}

/// Returns true if the given hostname has a token available from a non-env-var source.
/// Used by http.rs to decide whether to attach GitHub auth headers to requests.
pub fn is_gh_host(host: &str) -> bool {
    MISE_GITHUB_TOKENS.contains_key(host)
        || (Settings::get().github.gh_cli_tokens && GH_HOSTS.contains_key(host))
}

// ── github_tokens.toml ──────────────────────────────────────────────

/// Tokens from $MISE_CONFIG_DIR/github_tokens.toml.
/// Maps hostname (e.g. "github.com") to token string.
static MISE_GITHUB_TOKENS: Lazy<HashMap<String, String>> =
    Lazy::new(|| read_mise_github_tokens().unwrap_or_default());

#[derive(Deserialize)]
struct MiseGithubTokensFile {
    tokens: Option<HashMap<String, MiseGithubTokenEntry>>,
}

#[derive(Deserialize)]
struct MiseGithubTokenEntry {
    token: Option<String>,
}

fn parse_github_tokens(contents: &str) -> Option<HashMap<String, String>> {
    let file: MiseGithubTokensFile = toml::from_str(contents).ok()?;
    Some(
        file.tokens?
            .into_iter()
            .filter_map(|(host, entry)| entry.token.map(|t| (host, t)))
            .collect(),
    )
}

fn read_mise_github_tokens() -> Option<HashMap<String, String>> {
    let path = env::MISE_CONFIG_DIR.join("github_tokens.toml");
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            trace!("github_tokens.toml not readable at {}: {e}", path.display());
            return None;
        }
    };
    match parse_github_tokens(&contents) {
        Some(tokens) => Some(tokens),
        None => {
            debug!("failed to parse github_tokens.toml at {}", path.display());
            None
        }
    }
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

// ── credential_command ──────────────────────────────────────────────

/// Cache for tokens obtained from `credential_command`.
/// Maps hostname to the token (or None if the command failed).
static CREDENTIAL_COMMAND_CACHE: Lazy<std::sync::Mutex<HashMap<String, Option<String>>>> =
    Lazy::new(Default::default);

/// Get a GitHub token by running the user's `credential_command` setting.
/// The host is passed as `$1` to the command. Results are cached per host.
fn get_credential_command_token(cmd: &str, host: &str) -> Option<String> {
    let mut cache = CREDENTIAL_COMMAND_CACHE
        .lock()
        .expect("CREDENTIAL_COMMAND_CACHE mutex poisoned");
    if let Some(token) = cache.get(host) {
        return token.clone();
    }
    let path_without_shims = path_env_without_shims();
    let result = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .arg("mise-credential-helper") // $0
        .arg(host) // $1
        .env("PATH", &path_without_shims)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()
        .and_then(|output| {
            if !output.status.success() {
                if let Ok(err) = String::from_utf8(output.stderr)
                    && !err.trim().is_empty()
                {
                    debug!("credential_command stderr: {}", err.trim());
                }
                return None;
            }
            String::from_utf8(output.stdout)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    trace!(
        "credential_command for {host}: {}",
        if result.is_some() {
            "found"
        } else {
            "not found"
        }
    );
    cache.insert(host.to_string(), result.clone());
    result
}

// ── git credential fill ─────────────────────────────────────────────

/// Cache for tokens obtained from `git credential fill`.
/// Maps hostname to the token (or None if the command failed / git is not installed).
static GIT_CREDENTIAL_CACHE: Lazy<std::sync::Mutex<HashMap<String, Option<String>>>> =
    Lazy::new(Default::default);

/// Get a GitHub token for `host` by running `git credential fill`.
/// Results are cached per hostname so the subprocess is only spawned once.
// TODO: make async and use tokio::sync::Mutex to avoid blocking the runtime
// thread during subprocess I/O. Requires making resolve_token and get_headers async.
fn get_git_credential_token(host: &str) -> Option<String> {
    let mut cache = GIT_CREDENTIAL_CACHE
        .lock()
        .expect("GIT_CREDENTIAL_CACHE mutex poisoned");
    if let Some(token) = cache.get(host) {
        return token.clone();
    }
    let path_without_shims = path_env_without_shims();
    let input = format!("protocol=https\nhost={host}\n\n");
    let result = std::process::Command::new("git")
        .args(["credential", "fill"])
        .env("PATH", &path_without_shims)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take()?.write_all(input.as_bytes()).ok()?;
            let output = child.wait_with_output().ok()?;
            if !output.status.success() {
                return None;
            }
            String::from_utf8(output.stdout)
                .ok()?
                .lines()
                .find_map(|line| line.strip_prefix("password="))
                .map(|p| p.to_string())
                .filter(|s| !s.is_empty())
        });
    trace!(
        "git credential fill for {host}: {}",
        if result.is_some() {
            "found"
        } else {
            "not found"
        }
    );
    cache.insert(host.to_string(), result.clone());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
