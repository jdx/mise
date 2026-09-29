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
) -> (Option<Vec<String>>, Vec<u32>) {
    let pages = std::sync::Mutex::new(pages.into_iter().map(Some).collect::<Vec<_>>());
    let requested = std::sync::Mutex::new(vec![]);
    let releases = paginate_mirrored_releases(require_assets, list_all, |page| {
        requested.lock().unwrap().push(page);
        let list = pages.lock().unwrap()[page as usize - 1].take().unwrap();
        async move { list }
    })
    .await;
    (
        releases.map(|r| r.into_iter().map(|r| r.tag_name).collect()),
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
    assert_eq!(releases.unwrap(), ["v2.0.0-rc.1", "v1.0.0"]);
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
    assert_eq!(releases.unwrap(), ["v2.0.0-rc.1", "v1.0.0"]);
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
    assert_eq!(releases.unwrap().len(), MAX_RELEASE_FALLBACK_PAGES);
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
    assert_eq!(releases.unwrap(), ["v2.0.0", "v1.0.0"]);
}

#[tokio::test]
async fn test_mirrored_releases_never_mix_sources() {
    // No mirror answer for a later page discards the earlier ones.
    let (releases, _) = paginate_canned(
        vec![
            Some(mirrored_page(vec![make_prerelease("v2.0.0-rc.1")], Some(2))),
            None,
        ],
        false,
        false,
    )
    .await;
    assert_eq!(releases, None);

    let (releases, _) = paginate_canned(vec![None], false, false).await;
    assert_eq!(releases, None);
}

#[tokio::test]
async fn test_mirrored_releases_stop_on_a_repeating_page() {
    // Even a page that keeps pointing back at itself ends the listing,
    // after at most the pages the mirror serves.
    let requested = std::sync::Mutex::new(0);
    let releases = paginate_mirrored_releases(false, true, |_| {
        *requested.lock().unwrap() += 1;
        async { Some(mirrored_page(vec![make_release("v1")], Some(1))) }
    })
    .await;
    assert!(releases.is_none());
    assert_eq!(
        requested.into_inner().unwrap(),
        crate::versions_host::GITHUB_RELEASES_MAX_PAGES
    );
}

#[tokio::test]
async fn test_mirrored_releases_defer_to_github_past_the_served_pages() {
    let pages = vec![
        Some(mirrored_page(vec![make_release("v2")], Some(2))),
        Some(crate::versions_host::GithubReleasesPage {
            truncated: true,
            ..mirrored_page(vec![make_release("v1")], None)
        }),
    ];
    let (releases, requested) = paginate_canned(pages, false, true).await;
    assert_eq!(releases, None);
    assert_eq!(requested, [1, 2]);

    // Without MISE_LIST_ALL_VERSIONS a stable release ends the listing
    // first, so the mirror's pages are enough.
    let pages = vec![Some(crate::versions_host::GithubReleasesPage {
        truncated: true,
        ..mirrored_page(vec![make_release("v1")], None)
    })];
    let (releases, _) = paginate_canned(pages, false, false).await;
    assert_eq!(releases.unwrap(), ["v1"]);
}
