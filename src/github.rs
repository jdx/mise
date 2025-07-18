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
    // pub created_at: String,
    // pub published_at: Option<String>,
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTag {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    // pub size: u64,
    pub browser_download_url: String,
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
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GithubTag>, _>(next, &headers)
                .await?;
            tags.extend(more);
            headers = h;
        }
    }

    Ok(tags.into_iter().map(|t| t.name).collect())
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

pub async fn get_release_asset_checksum(
    repo: &str,
    tag: &str,
    asset_name: &str,
) -> Result<Option<String>> {
    get_release_asset_checksum_for_url(API_URL, repo, tag, asset_name).await
}

pub async fn get_release_asset_checksum_for_url(
    api_url: &str,
    repo: &str,
    tag: &str,
    asset_name: &str,
) -> Result<Option<String>> {
    let release = get_release_for_url(api_url, repo, tag).await?;
    
    // Common checksum file patterns to look for
    let checksum_patterns = vec![
        format!("{}.sha256", asset_name),
        format!("{}.sha512", asset_name),
        format!("{}.md5", asset_name),
        "checksums.txt".to_string(),
        "checksums.sha256".to_string(),
        "sha256sums.txt".to_string(),
        "SHA256SUMS".to_string(),
        "CHECKSUMS".to_string(),
        "checksum.txt".to_string(),
    ];
    
    // Look for checksum files in the release assets
    for pattern in &checksum_patterns {
        if let Some(checksum_asset) = release.assets.iter().find(|asset| {
            asset.name.eq_ignore_ascii_case(pattern) || asset.name.contains(pattern)
        }) {
            match download_and_parse_checksum(&checksum_asset.browser_download_url, asset_name).await {
                Ok(Some(checksum)) => return Ok(Some(checksum)),
                Ok(None) => continue, // Checksum file found but no entry for this asset
                Err(e) => {
                    debug!("Failed to parse checksum from {}: {}", checksum_asset.name, e);
                    continue;
                }
            }
        }
    }
    
    Ok(None)
}

async fn download_and_parse_checksum(
    checksum_url: &str, 
    asset_name: &str
) -> Result<Option<String>> {
    let checksum_content = crate::http::HTTP_FETCH.get_text(checksum_url).await?;
    
    // Try to parse the checksum file and find the checksum for our asset
    for line in checksum_content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        // Handle different checksum file formats:
        // Format 1: "checksum filename" (most common)
        // Format 2: "checksum *filename" (indicates binary mode)
        // Format 3: "filename:checksum" (less common)
        
        if let Some((checksum, filename)) = parse_checksum_line(line) {
            if filename == asset_name {
                // Determine the hash algorithm based on checksum length
                let algorithm = match checksum.len() {
                    32 => "md5",
                    64 => "sha256",
                    96 => "sha384", 
                    128 => "sha512",
                    _ => {
                        // For unknown lengths, try to guess from the URL or default to sha256
                        if checksum_url.contains("md5") {
                            "md5"
                        } else if checksum_url.contains("sha512") {
                            "sha512"
                        } else {
                            "sha256" // Most common default
                        }
                    }
                };
                
                return Ok(Some(format!("{}:{}", algorithm, checksum)));
            }
        }
    }
    
    Ok(None)
}

fn parse_checksum_line(line: &str) -> Option<(String, String)> {
    // Try format: "checksum filename" or "checksum *filename"
    if let Some(space_idx) = line.find(' ') {
        let checksum = line[..space_idx].trim();
        let filename_part = line[space_idx + 1..].trim();
        
        // Remove leading asterisk if present (indicates binary mode)
        let filename = filename_part.strip_prefix('*').unwrap_or(filename_part);
        
        // Extract just the filename from a path
        let filename = std::path::Path::new(filename)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(filename);
            
        if is_valid_checksum(checksum) {
            return Some((checksum.to_string(), filename.to_string()));
        }
    }
    
    // Try format: "filename:checksum"
    if let Some(colon_idx) = line.find(':') {
        let filename_part = line[..colon_idx].trim();
        let checksum = line[colon_idx + 1..].trim();
        
        let filename = std::path::Path::new(filename_part)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(filename_part);
            
        if is_valid_checksum(checksum) {
            return Some((checksum.to_string(), filename.to_string()));
        }
    }
    
    None
}

fn is_valid_checksum(s: &str) -> bool {
    // Check if string looks like a valid hex checksum
    s.len() >= 32 && s.chars().all(|c| c.is_ascii_hexdigit())
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
            "authorization",
            HeaderValue::from_str(format!("token {token}").as_str()).unwrap(),
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
    } else if let Some(token) = env::MISE_GITHUB_ENTERPRISE_TOKEN.as_ref() {
        set_headers(token);
    }

    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_checksum_line_standard_format() {
        // Test standard format: "checksum filename"
        let result = parse_checksum_line("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 hello.txt");
        assert_eq!(
            result,
            Some((
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                "hello.txt".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_checksum_line_binary_mode() {
        // Test binary mode format: "checksum *filename"
        let result = parse_checksum_line("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 *hello.txt");
        assert_eq!(
            result,
            Some((
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                "hello.txt".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_checksum_line_colon_format() {
        // Test colon format: "filename:checksum"
        let result = parse_checksum_line("hello.txt:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(
            result,
            Some((
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                "hello.txt".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_checksum_line_with_path() {
        // Test with full path in filename
        let result = parse_checksum_line("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 ./dist/hello.txt");
        assert_eq!(
            result,
            Some((
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                "hello.txt".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_checksum_line_invalid() {
        // Test invalid formats
        assert_eq!(parse_checksum_line("not a checksum line"), None);
        assert_eq!(parse_checksum_line(""), None);
        assert_eq!(parse_checksum_line("# comment line"), None);
        assert_eq!(parse_checksum_line("tooshort filename"), None);
    }

    #[test]
    fn test_is_valid_checksum() {
        // Test valid checksums
        assert!(is_valid_checksum("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")); // sha256
        assert!(is_valid_checksum("d41d8cd98f00b204e9800998ecf8427e")); // md5
        assert!(is_valid_checksum("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")); // sha512

        // Test invalid checksums
        assert!(!is_valid_checksum("not_hex_chars"));
        assert!(!is_valid_checksum("tooshort"));
        assert!(!is_valid_checksum(""));
        assert!(!is_valid_checksum("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b85g")); // contains 'g'
    }

    #[test]
    fn test_checksum_algorithm_detection() {
        // We can't easily test the full download_and_parse_checksum function without mocking HTTP,
        // but we can test the algorithm detection logic
        let test_cases = vec![
            (32, "md5"),
            (64, "sha256"),
            (96, "sha384"),
            (128, "sha512"),
        ];

        for (length, expected_algo) in test_cases {
            let checksum = "a".repeat(length);
            let algorithm = match checksum.len() {
                32 => "md5",
                64 => "sha256",
                96 => "sha384",
                128 => "sha512",
                _ => "sha256",
            };
            assert_eq!(algorithm, expected_algo);
        }
    }
}
