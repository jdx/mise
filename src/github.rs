use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub prerelease: bool,
    pub created_at: String,
    pub published_at: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubTag {
    pub name: String,
    pub zipball_url: String,
    pub tarball_url: String,
    pub commit: GithubCommit,
    pub node_id: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubCommit {
    pub sha: String,
    pub url: String,
}

pub fn list_releases(repo: &str) -> eyre::Result<Vec<GithubRelease>> {
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    crate::http::HTTP_FETCH.json(url)
}

pub fn list_tags(repo: &str) -> eyre::Result<Vec<GithubTag>> {
    let url = format!("https://api.github.com/repos/{}/tags", repo);
    crate::http::HTTP_FETCH.json(url)
}
