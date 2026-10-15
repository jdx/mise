use super::*;

#[tokio::test]
async fn test_attested_by_repository_matches_the_requested_repo_without_a_request() {
    // An exact (case-insensitive) match never asks github.com.
    assert!(attested_by_repository("JDX", "Mise", &["jdx/mise".to_string()]).await);
}

fn mirrored_page(
    releases: Vec<GithubRelease>,
    next_page: Option<u32>,
) -> crate::versions_host::GithubReleasesPage {
    crate::versions_host::GithubReleasesPage {
        releases,
        next_page,
        truncated: false,
    }
}

/// Runs the mirror pagination over canned pages, recording which pages it asked for.
async fn paginate_canned(
    pages: Vec<Option<crate::versions_host::GithubReleasesPage>>,
    require_assets: bool,
    list_all: bool,
) -> (eyre::Result<Option<Vec<String>>>, Vec<u32>) {
    let pages = std::sync::Mutex::new(pages.into_iter().map(Some).collect::<Vec<_>>());
    let requested = std::sync::Mutex::new(vec![]);
    let releases = paginate_mirrored_releases(require_assets, list_all, |page| {
        requested.lock().unwrap().push(page);
        let list = pages.lock().unwrap()[page as usize - 1].take().unwrap();
        async move { Ok(list) }
    })
    .await;
    (
        releases.map(|r| r.map(|r| r.into_iter().map(|r| r.tag_name).collect())),
        requested.into_inner().unwrap(),
    )
}

#[tokio::test]
async fn test_mirrored_releases_stop_at_a_stable_release() {
    let (releases, requested) = paginate_canned(
        vec![Some(mirrored_page(
            vec![make_prerelease("v2.0.0-rc.1"), make_release("v1.0.0")],
            Some(2),
        ))],
        false,
        false,
    )
    .await;
    assert_eq!(releases.unwrap().unwrap(), ["v2.0.0-rc.1", "v1.0.0"]);
    assert_eq!(requested, [1]);
}

#[tokio::test]
async fn test_mirrored_releases_follow_next_page_past_prereleases() {
    // An empty page with a next page is what a page of drafts looks like
    // once the mirror has removed them.
    let (releases, requested) = paginate_canned(
        vec![
            Some(mirrored_page(vec![make_prerelease("v2.0.0-rc.1")], Some(2))),
            Some(mirrored_page(vec![], Some(3))),
            Some(mirrored_page(vec![make_release("v1.0.0")], None)),
        ],
        false,
        false,
    )
    .await;
    assert_eq!(releases.unwrap().unwrap(), ["v2.0.0-rc.1", "v1.0.0"]);
    assert_eq!(requested, [1, 2, 3]);
}

#[tokio::test]
async fn test_mirrored_releases_are_bounded_like_the_direct_listing() {
    let pages = (1..=MAX_RELEASE_FALLBACK_PAGES as u32 + 1)
        .map(|p| {
            Some(mirrored_page(
                vec![make_prerelease(&format!("v0.0.{p}-rc"))],
                Some(p + 1),
            ))
        })
        .collect();
    let (releases, requested) = paginate_canned(pages, false, false).await;
    assert_eq!(releases.unwrap().unwrap().len(), MAX_RELEASE_FALLBACK_PAGES);
    assert_eq!(requested.len(), MAX_RELEASE_FALLBACK_PAGES);
}

#[tokio::test]
async fn test_mirrored_releases_require_assets_keeps_paginating() {
    let (releases, _) = paginate_canned(
        vec![
            Some(mirrored_page(vec![make_release("v2.0.0")], Some(2))),
            Some(mirrored_page(
                vec![GithubRelease {
                    assets: vec![make_asset("tool.tar.gz")],
                    ..make_release("v1.0.0")
                }],
                None,
            )),
        ],
        true,
        false,
    )
    .await;
    assert_eq!(releases.unwrap().unwrap(), ["v2.0.0", "v1.0.0"]);
}

#[tokio::test]
async fn test_mirrored_releases_never_mix_sources() {
    // A later page going missing is an error rather than half a list, or a
    // cue to ask GitHub for the rest.
    let (releases, _) = paginate_canned(
        vec![
            Some(mirrored_page(vec![make_prerelease("v2.0.0-rc.1")], Some(2))),
            None,
        ],
        false,
        false,
    )
    .await;
    assert!(releases.is_err());

    // A first page that is a 404: mise-versions doesn't know the repository.
    let (releases, _) = paginate_canned(vec![None], false, false).await;
    assert!(releases.unwrap().is_none());
}

#[tokio::test]
async fn test_mirrored_releases_surface_a_failed_page() {
    let releases = paginate_mirrored_releases(false, false, |_| async {
        Err::<Option<crate::versions_host::GithubReleasesPage>, _>(eyre::eyre!("502"))
    })
    .await;
    assert!(releases.is_err());
}

#[tokio::test]
async fn test_mirrored_releases_stop_on_a_repeating_page() {
    // Even a page that keeps pointing back at itself ends the listing,
    // after at most the pages the mirror serves.
    let requested = std::sync::Mutex::new(0);
    let releases = paginate_mirrored_releases(false, true, |_| {
        *requested.lock().unwrap() += 1;
        async { Ok(Some(mirrored_page(vec![make_release("v1")], Some(1)))) }
    })
    .await;
    assert_eq!(
        releases.unwrap().unwrap().len(),
        crate::versions_host::GITHUB_RELEASES_MAX_PAGES
    );
    assert_eq!(
        requested.into_inner().unwrap(),
        crate::versions_host::GITHUB_RELEASES_MAX_PAGES
    );
}

#[tokio::test]
async fn test_mirrored_releases_end_at_the_served_pages() {
    let pages = vec![
        Some(mirrored_page(vec![make_release("v2")], Some(2))),
        Some(crate::versions_host::GithubReleasesPage {
            truncated: true,
            ..mirrored_page(vec![make_release("v1")], None)
        }),
    ];
    // Older releases than mise-versions serves are not fetched from GitHub.
    let (releases, requested) = paginate_canned(pages, false, true).await;
    assert_eq!(releases.unwrap().unwrap(), ["v2", "v1"]);
    assert_eq!(requested, [1, 2]);

    // Without MISE_LIST_ALL_VERSIONS a stable release ends the listing
    // first, so the mirror's pages are enough.
    let pages = vec![Some(crate::versions_host::GithubReleasesPage {
        truncated: true,
        ..mirrored_page(vec![make_release("v1")], None)
    })];
    let (releases, _) = paginate_canned(pages, false, false).await;
    assert_eq!(releases.unwrap().unwrap(), ["v1"]);
}

fn tags_page(names: &[&str], next_page: Option<u32>) -> crate::versions_host::GithubTagsPage {
    crate::versions_host::GithubTagsPage {
        tags: names
            .iter()
            .map(|name| crate::versions_host::GithubMirroredTag {
                name: name.to_string(),
                date: None,
            })
            .collect(),
        next_page,
        truncated: false,
    }
}

async fn paginate_tags_canned(
    pages: Vec<Option<crate::versions_host::GithubTagsPage>>,
    list_all: bool,
) -> (eyre::Result<Option<Vec<String>>>, Vec<u32>) {
    let pages = std::sync::Mutex::new(pages.into_iter().map(Some).collect::<Vec<_>>());
    let requested = std::sync::Mutex::new(vec![]);
    let tags = paginate_mirrored_tags(list_all, |page| {
        requested.lock().unwrap().push(page);
        let list = pages.lock().unwrap()[page as usize - 1].take().unwrap();
        async move { Ok(list) }
    })
    .await;
    (
        tags.map(|t| t.map(|t| t.into_iter().map(|t| t.name).collect())),
        requested.into_inner().unwrap(),
    )
}

#[tokio::test]
async fn test_mirrored_tags_read_one_page_unless_listing_all() {
    let pages = || {
        vec![
            Some(tags_page(&["v2", "v1"], Some(2))),
            Some(tags_page(&["v0"], None)),
        ]
    };
    let (tags, requested) = paginate_tags_canned(pages(), false).await;
    assert_eq!(tags.unwrap().unwrap(), ["v2", "v1"]);
    assert_eq!(requested, [1]);

    let (tags, requested) = paginate_tags_canned(pages(), true).await;
    assert_eq!(tags.unwrap().unwrap(), ["v2", "v1", "v0"]);
    assert_eq!(requested, [1, 2]);
}

#[tokio::test]
async fn test_mirrored_tags_404_only_on_the_first_page() {
    let (tags, _) = paginate_tags_canned(vec![None], true).await;
    assert!(tags.unwrap().is_none());

    let (tags, _) = paginate_tags_canned(vec![Some(tags_page(&["v2"], Some(2))), None], true).await;
    assert!(tags.is_err());
}

#[tokio::test]
async fn test_mirrored_tags_stop_at_the_served_pages() {
    let requested = std::sync::Mutex::new(0);
    let tags = paginate_mirrored_tags(true, |page| {
        *requested.lock().unwrap() += 1;
        async move { Ok(Some(tags_page(&["v1"], Some(page + 1)))) }
    })
    .await;
    assert_eq!(
        tags.unwrap().unwrap().len(),
        crate::versions_host::GITHUB_RELEASES_MAX_PAGES
    );
    assert_eq!(
        requested.into_inner().unwrap(),
        crate::versions_host::GITHUB_RELEASES_MAX_PAGES
    );
}

#[tokio::test]
async fn test_stale_copy_stands_in_while_mise_versions_is_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("releases.msgpack.z");
    let unavailable = |status| -> eyre::Result<Vec<String>> {
        Err(crate::versions_host::VersionsHostUnavailable {
            status,
            message: "mise-versions answered".to_string(),
        }
        .into())
    };
    // A copy written earlier, already past its freshness window.
    CacheManagerBuilder::new(&path)
        .build::<Vec<String>>()
        .write(&vec!["v1".to_string()])
        .unwrap();
    let stale = || {
        CacheManagerBuilder::new(&path)
            .with_fresh_duration(Some(std::time::Duration::ZERO))
            .build::<Vec<String>>()
    };

    for status in [Some(503), Some(429), None] {
        let got = cached_or_stale(&stale(), async || unavailable(status)).await;
        assert_eq!(got.unwrap(), ["v1"], "status {status:?}");
    }
    // Restricted or disabled, or a failure that isn't mise-versions': no.
    assert!(
        cached_or_stale(&stale(), async || unavailable(Some(403)))
            .await
            .is_err()
    );
    assert!(
        cached_or_stale(&stale(), async || -> eyre::Result<Vec<String>> {
            Err(eyre::eyre!("bad data"))
        })
        .await
        .is_err()
    );
    // Nothing cached: the failure is the answer.
    let empty = CacheManagerBuilder::new(tmp.path().join("none.msgpack.z")).build::<Vec<String>>();
    assert!(
        cached_or_stale(&empty, async || unavailable(Some(503)))
            .await
            .is_err()
    );
}
