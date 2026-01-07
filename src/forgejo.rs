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
    dirs::CACHE.join("forgejo")
}

pub fn get_headers<U: IntoUrl>(url: U) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let url = url.into_url().unwrap();
    let mut set_headers = |token: &str| {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_str(format!("Bearer {token}").as_str()).unwrap(),
        );
    };

    if url.host_str() == Some("codeberg.org") {
        if let Some(token) = env::FORGEJO_TOKEN.as_ref() {
            set_headers(token);
        }
    } else if let Some(token) = env::MISE_FORGEJO_ENTERPRISE_TOKEN.as_ref() {
        set_headers(token);
    }
    headers
}
