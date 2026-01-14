use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::{dirs, duration, env};
use eyre::Result;
use heck::ToKebabCase;
use reqwest::IntoUrl;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
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
    let url = format!("{api_url}/repos/{repo}/releases/tags/{tag}");
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

pub fn get_headers<U: IntoUrl>(url: U) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let url = url.into_url().unwrap();
    let mut set_headers = |token: &str| {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(format!("Bearer {token}").as_str()).unwrap(),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );
    };

    if url.host_str() == Some("api.github.com") {
        if let Some(token) = env::GITHUB_TOKEN.as_ref() {
            set_headers(token);
        }
    } else if let Some(token) = env::MISE_GITHUB_ENTERPRISE_TOKEN
        .as_ref()
        .or(env::GITHUB_TOKEN.as_ref())
    {
        set_headers(token);
    }

    if url.path().contains("/releases/assets/") {
        headers.insert(
            "accept",
            HeaderValue::from_static("application/octet-stream"),
        );
    }

    headers
}
