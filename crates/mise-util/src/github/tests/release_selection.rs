use super::*;

#[test]
fn releases_cache_key_separates_the_suffix_collision() {
    let api = "https://api.github.com";

    // The pair a `-with-assets` suffix would collapse. This cache is global
    // and persisted, so a collision serves one repository the other's
    // releases.
    assert_ne!(
        releases_cache_key(api, "owner/foo", true),
        releases_cache_key(api, "owner/foo-with-assets", false)
    );

    // The flag still separates one repository from itself...
    assert_ne!(
        releases_cache_key(api, "owner/foo", true),
        releases_cache_key(api, "owner/foo", false)
    );
    // ...the api_url still separates two hosts...
    assert_ne!(
        releases_cache_key(api, "owner/foo", false),
        releases_cache_key("https://github.example.com/api/v3", "owner/foo", false)
    );
    // ...and the same inputs still land on the same entry.
    assert_eq!(
        releases_cache_key(api, "owner/foo", true),
        releases_cache_key(api, "owner/foo", true)
    );
}

#[test]
fn stopping_stable_release_ignores_assets_unless_asked() {
    let mut assetless = make_release("v1.0.0");
    let mut nightly = make_release("v2.0.0-nightly");
    nightly.prerelease = true;
    let releases = vec![nightly, assetless.clone()];

    // A caller that keeps every release stops here, exactly as before.
    assert!(has_stopping_stable_release(&releases, false));

    // The `github:` backend does not: it will drop `v1.0.0` from the
    // listing, so stopping on it would hide an installable stable release
    // sitting on the next page.
    assert!(!has_stopping_stable_release(&releases, true));

    assetless.assets = vec![asset("tool-x86_64-unknown-linux-gnu.tar.gz")];
    assert!(has_stopping_stable_release(&[assetless], true));
}

#[test]
fn stopping_stable_release_still_rejects_drafts_and_prereleases() {
    // Assets do not promote a draft or a prerelease into a stopping point.
    let mut draft = make_release("v1.0.0");
    draft.draft = true;
    draft.assets = vec![asset("tool.tar.gz")];

    let mut prerelease = make_release("v2.0.0");
    prerelease.prerelease = true;
    prerelease.assets = vec![asset("tool.tar.gz")];

    let releases = vec![draft, prerelease];
    assert!(!has_stopping_stable_release(&releases, true));
    assert!(!has_stopping_stable_release(&releases, false));
}

#[test]
fn release_date_prefers_published_at() {
    let mut release = make_release("v1.0.0");
    release.created_at = "2026-06-28T17:38:00Z".into();
    release.published_at = Some("2026-08-06T09:16:56Z".into());

    assert_eq!(release.released_at(), "2026-08-06T09:16:56Z");
}

#[test]
fn release_date_falls_back_to_created_at() {
    let mut release = make_release("v1.0.0");
    release.created_at = "2026-06-28T17:38:00Z".into();

    assert_eq!(release.released_at(), "2026-06-28T17:38:00Z");
}

#[test]
fn release_without_published_at_remains_deserializable() {
    let release: GithubRelease = serde_json::from_value(serde_json::json!({
        "tag_name": "v1.0.0",
        "draft": false,
        "prerelease": false,
        "created_at": "2026-06-28T17:38:00Z",
        "assets": []
    }))
    .unwrap();

    assert_eq!(release.released_at(), "2026-06-28T17:38:00Z");
}

#[test]
fn test_build_revision_selects_highest() {
    let releases = vec![
        make_release("3.3.11"),
        make_release("3.3.11-1"),
        make_release("3.3.11-2"),
        make_release("3.3.10-1"),
    ];
    let best = pick_best_build_revision(releases, "3.3.11").unwrap();
    assert_eq!(best.tag_name, "3.3.11-2");
}

#[test]
fn test_numeric_build_revision_selects_highest_without_base_fallback() {
    let releases = vec![
        make_release("3.3.11"),
        make_release("3.3.11-1"),
        make_release("3.3.11-2"),
        make_release("3.3.10-1"),
    ];
    let best = pick_best_numeric_build_revision(releases, "3.3.11").unwrap();
    assert_eq!(best.tag_name, "3.3.11-2");

    let releases = vec![make_release("3.3.11"), make_release("3.3.10-1")];
    assert!(pick_best_numeric_build_revision(releases, "3.3.11").is_none());
}

/// RubyInstaller2 tags releases `RubyInstaller-<version>-<revision>`, so the
/// caller passes the prefixed tag as the "version" (discussion #5227). The
/// prefix comparison must also keep 3.4.4 from matching 3.4.10.
#[test]
fn test_numeric_build_revision_handles_prefixed_tags() {
    let releases = vec![
        make_release("RubyInstaller-3.4.4-1"),
        make_release("RubyInstaller-3.4.4-2"),
        make_release("RubyInstaller-3.4.10-1"),
    ];
    let best = pick_best_numeric_build_revision(releases, "RubyInstaller-3.4.4").unwrap();
    assert_eq!(best.tag_name, "RubyInstaller-3.4.4-2");
}

#[test]
fn test_build_revision_falls_back_to_base() {
    let releases = vec![make_release("3.3.11"), make_release("3.3.10-1")];
    let best = pick_best_build_revision(releases, "3.3.11").unwrap();
    assert_eq!(best.tag_name, "3.3.11");
}

#[test]
fn test_build_revision_no_match() {
    let releases = vec![make_release("3.3.10"), make_release("3.3.10-1")];
    let best = pick_best_build_revision(releases, "3.3.11");
    assert!(best.is_none());
}

#[test]
fn test_build_revision_ignores_non_numeric_suffix() {
    let releases = vec![
        make_release("3.3.11"),
        make_release("3.3.11-rc1"),
        make_release("3.3.11-1"),
    ];
    let best = pick_best_build_revision(releases, "3.3.11").unwrap();
    assert_eq!(best.tag_name, "3.3.11-1");
}
