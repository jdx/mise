use crate::env;
use reqwest::header::HeaderMap;
use serde_derive::Deserialize;
use xx::regex;

#[derive(Debug, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    // pub name: Option<String>,
    // pub body: Option<String>,
    // pub prerelease: bool,
    // pub created_at: String,
    // pub published_at: Option<String>,
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    // pub size: u64,
    pub browser_download_url: String,
}

pub fn list_releases(repo: &str) -> eyre::Result<Vec<GithubRelease>> {
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    let (mut releases, mut headers) =
        crate::http::HTTP_FETCH.json_headers::<Vec<GithubRelease>, _>(url)?;

    if *env::MISE_LIST_ALL_VERSIONS {
        while let Some(next) = next_page(&headers) {
            let (more, h) = crate::http::HTTP_FETCH.json_headers::<Vec<GithubRelease>, _>(next)?;
            releases.extend(more);
            headers = h;
        }
    }

    Ok(releases)
}

pub fn get_release(repo: &str, tag: &str) -> eyre::Result<GithubRelease> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/tags/{}",
        repo, tag
    );
    crate::http::HTTP_FETCH.json(url)
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
