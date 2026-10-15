use super::*;

#[tokio::test]
async fn test_pick_reachable_asset_url_keeps_browser_url_when_reachable() {
    // Public repos: the browser URL serves the asset (not HTML), so it is kept and the
    // API endpoint is not used.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("HEAD", "/asset.tar.gz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .create_async()
        .await;
    let browser_url = format!("{}/asset.tar.gz", server.url());
    assert_eq!(
        pick_reachable_asset_url(&browser_url, ASSET_API_URL).await,
        browser_url
    );
    mock.assert_async().await;
}
#[tokio::test]
async fn test_pick_reachable_asset_url_falls_back_on_404() {
    // Private repos: the browser URL 404s even with a valid token, so fall back to the
    // API asset endpoint.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("HEAD", "/asset.tar.gz")
        .with_status(404)
        .create_async()
        .await;
    let browser_url = format!("{}/asset.tar.gz", server.url());
    assert_eq!(
        pick_reachable_asset_url(&browser_url, ASSET_API_URL).await,
        ASSET_API_URL
    );
    mock.assert_async().await;
}
#[tokio::test]
async fn test_pick_reachable_asset_url_falls_back_on_html_login_page() {
    // Some private repos return a 200 HTML login page at the browser URL instead of a
    // 404; that is also treated as unreachable and falls back to the API endpoint.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("HEAD", "/asset.tar.gz")
        .with_status(200)
        .with_header("content-type", "text/html; charset=utf-8")
        .create_async()
        .await;
    let browser_url = format!("{}/asset.tar.gz", server.url());
    assert_eq!(
        pick_reachable_asset_url(&browser_url, ASSET_API_URL).await,
        ASSET_API_URL
    );
    mock.assert_async().await;
}
#[tokio::test]
async fn test_pick_reachable_asset_url_falls_back_on_uppercase_html_content_type() {
    // HTTP media types are case-insensitive; a `Content-Type` such as `TEXT/HTML` must
    // still be recognized as an auth page and fall back to the API endpoint.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("HEAD", "/asset.tar.gz")
        .with_status(200)
        .with_header("content-type", "TEXT/HTML; charset=UTF-8")
        .create_async()
        .await;
    let browser_url = format!("{}/asset.tar.gz", server.url());
    assert_eq!(
        pick_reachable_asset_url(&browser_url, ASSET_API_URL).await,
        ASSET_API_URL
    );
    mock.assert_async().await;
}
#[tokio::test]
async fn test_download_uses_scoped_install_env() {
    let _token = GithubTokenGuard::new();
    let mut server = mockito::Server::new_async().await;
    let api_url = ghes_api_url(&server.url());
    let install_env = [(
        "GITHUB_TOKEN".to_string(),
        crate::env_value::EnvValue::from(false),
    )]
    .into_iter()
    .collect();
    let mock = server
        .mock("GET", format!("{API_PATH}/download").as_str())
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_body("download")
        .expect(1)
        .create_async()
        .await;
    let tempdir = tempfile::tempdir().unwrap();

    crate::env::with_install_env(install_env, async {
        crate::http::HTTP
            .download_file(
                format!("{api_url}/download"),
                &tempdir.path().join("file"),
                None,
            )
            .await
    })
    .await
    .unwrap();
    mock.assert_async().await;
}
#[tokio::test]
async fn test_release_lookup_encodes_the_tag_as_one_path_segment() {
    // A tag may hold `#` or `/`. Interpolated raw, the first would start a
    // fragment and the second would reach a different path, so neither
    // release could be looked up.
    let mut server = mockito::Server::new_async().await;
    let repo = "owner/tag-encoding-test";
    let tag = "release/2026#1";
    let mock = server
        .mock(
            "GET",
            format!("/repos/{repo}/releases/tags/release%2F2026%231").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&make_release(tag)).unwrap())
        .expect(1)
        .create_async()
        .await;

    let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, false)
        .await
        .unwrap();
    assert_eq!(release.tag_name, tag);
    mock.assert_async().await;
}
