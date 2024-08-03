use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    // pub name: Option<String>,
    // pub body: Option<String>,
    // pub prerelease: bool,
    // pub created_at: String,
    // pub published_at: Option<String>,
}

pub fn list_releases(repo: &str) -> eyre::Result<Vec<GithubRelease>> {
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    crate::http::HTTP_FETCH.json(url)
}
