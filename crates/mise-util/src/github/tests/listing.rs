use super::*;

#[tokio::test]
async fn test_empty_release_assets_are_not_cached() {
    let mut server = mockito::Server::new_async().await;
    let repo = "owner/empty-assets-cache-test";
    let tag = "v1.0.0";
    let path = format!("/repos/{repo}/releases/tags/{tag}");
    let key = release_cache_key(&server.url(), repo, tag, true);

    let cached_empty_release = make_release(tag);
    {
        let cache_group = get_release_cache(&key).await;
        let cache = cache_group.get(&key).unwrap();
        cache.write(&cached_empty_release).unwrap();
    }

    let empty_mock = server
        .mock("GET", path.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&cached_empty_release).unwrap())
        .expect(1)
        .create_async()
        .await;

    let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, true)
        .await
        .unwrap();
    assert!(release.assets.is_empty());
    empty_mock.assert_async().await;
    empty_mock.remove_async().await;

    let populated_release = GithubRelease {
        assets: vec![make_asset("tool-v1.0.0-linux-x86_64.tar.gz")],
        ..make_release(tag)
    };
    let mock = server
        .mock("GET", path.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&populated_release).unwrap())
        .expect(1)
        .create_async()
        .await;

    let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, true)
        .await
        .unwrap();
    assert_eq!(release.assets.len(), 1);
    assert_eq!(release.assets[0].name, "tool-v1.0.0-linux-x86_64.tar.gz");

    let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, true)
        .await
        .unwrap();
    assert_eq!(release.assets.len(), 1);
    mock.assert_async().await;
}
#[tokio::test]
async fn test_versions_host_flag_splits_release_cache() {
    let mut server = mockito::Server::new_async().await;
    let repo = "owner/versions-host-cache-split-test";
    let tag = "v1.0.0";
    let path = format!("/repos/{repo}/releases/tags/{tag}");
    let true_key = release_cache_key(&server.url(), repo, tag, true);

    {
        let cache_group = get_release_cache(&true_key).await;
        let cache = cache_group.get(&true_key).unwrap();
        cache
            .write(&GithubRelease {
                assets: vec![make_asset("cached-from-versions-host.tar.gz")],
                ..make_release(tag)
            })
            .unwrap();
    }

    let direct_release = GithubRelease {
        assets: vec![make_asset("direct-github-api.tar.gz")],
        ..make_release(tag)
    };
    let mock = server
        .mock("GET", path.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&direct_release).unwrap())
        .expect(1)
        .create_async()
        .await;

    let release = get_release_for_url_with_versions_host(&server.url(), repo, tag, false)
        .await
        .unwrap();
    assert_eq!(release.assets[0].name, "direct-github-api.tar.gz");
    mock.assert_async().await;
}
// #10343: a first page made up entirely of prereleases must not yield "no
// versions found" -- the fallback follows the Link header to a later page.
#[tokio::test]
async fn test_list_releases_paginates_past_all_prerelease_first_page() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    let repo = "owner/all-prerelease-first-page";

    let page1 = vec![
        make_prerelease("v2.0.0-alpha.2"),
        make_prerelease("v2.0.0-alpha.1"),
    ];
    let page2 = vec![make_release("v1.0.0")];

    // The first page requests per_page=100 and is entirely prereleases.
    let page1_mock = server
        .mock("GET", format!("/repos/{repo}/releases").as_str())
        .match_query(mockito::Matcher::UrlEncoded(
            "per_page".into(),
            "100".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("link", format!("<{base}/page2>; rel=\"next\"").as_str())
        .with_body(serde_json::to_string(&page1).unwrap())
        .expect(1)
        .create_async()
        .await;
    // The fallback follows the Link header to a second page that has a stable release.
    let page2_mock = server
        .mock("GET", "/page2")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&page2).unwrap())
        .expect(1)
        .create_async()
        .await;

    let releases = list_releases_(&base, repo, false).await.unwrap();
    page1_mock.assert_async().await;
    page2_mock.assert_async().await;
    assert!(
        releases
            .iter()
            .any(|r| r.tag_name == "v1.0.0" && !r.prerelease),
        "stable release from page 2 should be discovered, got {:?}",
        releases.iter().map(|r| &r.tag_name).collect::<Vec<_>>()
    );
}
// The listing filter reaching back into the fetch loop: a stable release
// with no assets is dropped from the version list, so for a caller that
// drops it there is nothing on this page worth stopping for.
#[tokio::test]
async fn test_list_releases_paginates_past_an_assetless_stable_release() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();

    // Published, not a draft, not a prerelease -- and nothing attached.
    let assetless_page = vec![make_release("v2.0.0")];
    let asset_page = vec![GithubRelease {
        assets: vec![asset("tool-x86_64-unknown-linux-gnu.tar.gz")],
        ..make_release("v1.0.0")
    }];

    let repo = "owner/assetless-stable-first-page";
    let page1_mock = server
        .mock("GET", format!("/repos/{repo}/releases").as_str())
        .match_query(mockito::Matcher::UrlEncoded(
            "per_page".into(),
            "100".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("link", format!("<{base}/page2>; rel=\"next\"").as_str())
        .with_body(serde_json::to_string(&assetless_page).unwrap())
        .expect(1)
        .create_async()
        .await;
    let page2_mock = server
        .mock("GET", "/page2")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&asset_page).unwrap())
        .expect(1)
        .create_async()
        .await;

    let releases = list_releases_(&base, repo, true).await.unwrap();
    page1_mock.assert_async().await;
    page2_mock.assert_async().await;
    assert!(
        releases.iter().any(|r| r.tag_name == "v1.0.0"),
        "installable stable release from page 2 should be discovered, got {:?}",
        releases.iter().map(|r| &r.tag_name).collect::<Vec<_>>()
    );

    // The same first page still stops the loop for a caller that keeps
    // asset-less releases. Without this the test above would also pass if
    // the bound had simply been removed for everyone.
    let kept_repo = "owner/assetless-stable-kept";
    let kept_page1_mock = server
        .mock("GET", format!("/repos/{kept_repo}/releases").as_str())
        .match_query(mockito::Matcher::UrlEncoded(
            "per_page".into(),
            "100".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header(
            "link",
            format!("<{base}/page2-kept>; rel=\"next\"").as_str(),
        )
        .with_body(serde_json::to_string(&assetless_page).unwrap())
        .expect(1)
        .create_async()
        .await;
    let kept_page2_mock = server
        .mock("GET", "/page2-kept")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&asset_page).unwrap())
        .expect(0)
        .create_async()
        .await;

    let releases = list_releases_(&base, kept_repo, false).await.unwrap();
    kept_page1_mock.assert_async().await;
    kept_page2_mock.assert_async().await;
    assert!(
        releases.iter().all(|r| r.tag_name != "v1.0.0"),
        "page 2 should not have been fetched, got {:?}",
        releases.iter().map(|r| &r.tag_name).collect::<Vec<_>>()
    );
}
// #10343: once a stable release is seen the fallback stops (no extra API calls).
#[tokio::test]
async fn test_list_releases_stops_when_first_page_has_stable() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    let repo = "owner/stable-on-first-page";

    let page1 = vec![make_prerelease("v1.1.0-alpha.1"), make_release("v1.0.0")];

    let page1_mock = server
        .mock("GET", format!("/repos/{repo}/releases").as_str())
        .match_query(mockito::Matcher::UrlEncoded(
            "per_page".into(),
            "100".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("link", format!("<{base}/page2>; rel=\"next\"").as_str())
        .with_body(serde_json::to_string(&page1).unwrap())
        .expect(1)
        .create_async()
        .await;
    // A stable release is already present, so page 2 must NOT be fetched.
    let page2_mock = server
        .mock("GET", "/page2")
        .with_status(200)
        .with_body("[]")
        .expect(0)
        .create_async()
        .await;

    let releases = list_releases_(&base, repo, false).await.unwrap();
    page1_mock.assert_async().await;
    page2_mock.assert_async().await;
    assert!(releases.iter().any(|r| r.tag_name == "v1.0.0"));
}
// #10343: the prerelease fallback is bounded to MAX_RELEASE_FALLBACK_PAGES pages.
#[tokio::test]
async fn test_list_releases_fallback_pagination_is_bounded() {
    let mut server = mockito::Server::new_async().await;
    let base = server.url();
    let repo = "owner/all-prerelease-many-pages";

    let body = || serde_json::to_string(&vec![make_prerelease("v9.0.0-alpha")]).unwrap();

    // Three all-prerelease pages, each linking to the next.
    let p1 = server
        .mock("GET", format!("/repos/{repo}/releases").as_str())
        .match_query(mockito::Matcher::UrlEncoded(
            "per_page".into(),
            "100".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("link", format!("<{base}/p2>; rel=\"next\"").as_str())
        .with_body(body())
        .expect(1)
        .create_async()
        .await;
    let p2 = server
        .mock("GET", "/p2")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("link", format!("<{base}/p3>; rel=\"next\"").as_str())
        .with_body(body())
        .expect(1)
        .create_async()
        .await;
    let p3 = server
        .mock("GET", "/p3")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("link", format!("<{base}/p4>; rel=\"next\"").as_str())
        .with_body(body())
        .expect(1)
        .create_async()
        .await;
    // The 4th page must never be requested (capped at MAX_RELEASE_FALLBACK_PAGES).
    let p4 = server
        .mock("GET", "/p4")
        .with_status(200)
        .with_body("[]")
        .expect(0)
        .create_async()
        .await;

    let releases = list_releases_(&base, repo, false).await.unwrap();
    p1.assert_async().await;
    p2.assert_async().await;
    p3.assert_async().await;
    p4.assert_async().await;
    assert_eq!(releases.len(), 3);
}
fn with_token_lock(future: impl std::future::Future<Output = ()>) {
    let _lock = crate::testing::lock_ignoring_poison(&TEST_ENV_LOCK);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

// Regression for #6318: every paginated request must carry the Authorization header.
// Before that fix page 2 was sent page 1's *response* headers and went out
// unauthenticated; nothing pinned the fix until now.
#[test]
fn test_list_releases_sends_auth_on_every_page() {
    with_token_lock(async {
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let api = ghes_api_url(&base);
        let repo = "owner/auth-on-every-page";
        let host = url::Url::parse(&base)
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let _token = TokensFileOverrideGuard::set(&host, PAGINATE_TEST_TOKEN);
        let auth = format!("Bearer {PAGINATE_TEST_TOKEN}");

        let page1 = server
            .mock("GET", format!("{API_PATH}/repos/{repo}/releases").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{api}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&vec![make_prerelease("v2.0.0-alpha.1")]).unwrap())
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", format!("{API_PATH}/page2").as_str())
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![make_release("v1.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;

        let releases = list_releases_(&api, repo, false).await.unwrap();
        page1.assert_async().await;
        page2.assert_async().await;
        assert_eq!(releases.len(), 2);
    });
}
// Same regression for the tags loop -- see the release test above.
#[test]
fn test_list_tags_sends_auth_on_every_page() {
    with_token_lock(async {
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let api = ghes_api_url(&base);
        let repo = "owner/auth-on-every-page";
        let host = url::Url::parse(&base)
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let _token = TokensFileOverrideGuard::set(&host, PAGINATE_TEST_TOKEN);
        let auth = format!("Bearer {PAGINATE_TEST_TOKEN}");

        let page1 = server
            .mock("GET", format!("{API_PATH}/repos/{repo}/tags").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{api}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&vec![tag_without_commit("v2.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", format!("{API_PATH}/page2").as_str())
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![tag_without_commit("v1.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;

        let tags = list_tags_(&api, repo, true).await.unwrap();
        page1.assert_async().await;
        page2.assert_async().await;
        assert_eq!(tags, ["v2.0.0", "v1.0.0"]);
    });
}
// `list_tags_with_dates_` paginates unconditionally, so it needs the same guarantee.
// Tags carry no `commit`, which keeps this to the two paginated requests.
#[test]
fn test_list_tags_with_dates_sends_auth_on_every_page() {
    with_token_lock(async {
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let api = ghes_api_url(&base);
        let repo = "owner/auth-on-every-page-dates";
        let host = url::Url::parse(&base)
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let _token = TokensFileOverrideGuard::set(&host, PAGINATE_TEST_TOKEN);
        let auth = format!("Bearer {PAGINATE_TEST_TOKEN}");

        let page1 = server
            .mock("GET", format!("{API_PATH}/repos/{repo}/tags").as_str())
            .match_query(mockito::Matcher::UrlEncoded(
                "per_page".into(),
                "100".into(),
            ))
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", format!("<{api}/page2>; rel=\"next\"").as_str())
            .with_body(serde_json::to_string(&vec![tag_without_commit("v2.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", format!("{API_PATH}/page2").as_str())
            .match_header("authorization", auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&vec![tag_without_commit("v1.0.0")]).unwrap())
            .expect(1)
            .create_async()
            .await;

        let tags = list_tags_with_dates_(&api, repo).await.unwrap();
        page1.assert_async().await;
        page2.assert_async().await;
        assert_eq!(
            tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["v2.0.0", "v1.0.0"]
        );
        assert!(tags.iter().all(|t| t.date.is_none()));
    });
}
