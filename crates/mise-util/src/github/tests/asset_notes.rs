use super::*;

#[test]
fn test_replaced_asset_note_when_github_digest_matches_download() {
    let release = replaced_release(Some("sha256:90A8"));
    let note = replaced_asset_note("rvben/rumdl", &release, "rumdl.tar.gz", "90a8", true).unwrap();
    assert!(note.contains("rumdl.tar.gz in rvben/rumdl v0.2.76 matches this download"));
    assert!(
        note.contains(
            "(asset updated 2026-09-23T08:25:23Z, release published 2026-09-23T01:45:05Z)"
        )
    );
    assert!(note.contains("update the checksum in mise.lock"));
    let note = replaced_asset_note("rvben/rumdl", &release, "rumdl.tar.gz", "90a8", false).unwrap();
    assert!(note.contains("cached release metadata"));
    assert!(!note.contains("mise.lock"));
}

#[tokio::test]
async fn test_checksum_mismatch_note_skips_other_verification_errors() {
    // A size mismatch must not trigger the GitHub lookup or the hint.
    let err = with_checksum_mismatch_note(
        eyre::eyre!("Size mismatch for rumdl.tar.gz: expected 1, got 2"),
        "https://github.com/rvben/rumdl/releases/download/v0.2.76/rumdl.tar.gz",
        Path::new("/nonexistent"),
        true,
    )
    .await;
    assert_eq!(
        format!("{err:#}"),
        "Size mismatch for rumdl.tar.gz: expected 1, got 2"
    );
}

#[test]
fn test_replaced_asset_note_skips_when_download_differs_from_github() {
    let release = replaced_release(Some("sha256:90a8"));
    assert_eq!(
        replaced_asset_note("rvben/rumdl", &release, "rumdl.tar.gz", "3a02", true),
        None
    );
    let release = replaced_release(None);
    assert_eq!(
        replaced_asset_note("rvben/rumdl", &release, "rumdl.tar.gz", "90a8", true),
        None
    );
    assert_eq!(
        replaced_asset_note("rvben/rumdl", &release, "other.tar.gz", "90a8", true),
        None
    );
}

#[test]
fn test_mirrored_assets_survive_the_release_cache() {
    let api_url = "https://api.github.com/repos/o/cached/releases/assets/7";
    let release = GithubRelease {
        assets: vec![GithubAsset {
            from_versions_host: true,
            url: api_url.to_string(),
            ..make_asset("tool.tar.gz")
        }],
        ..make_release("v1.0.0")
    };
    // Written to and read back from a cache, as a later run would.
    let cached: GithubRelease =
        serde_json::from_str(&serde_json::to_string(&release).unwrap()).unwrap();
    assert!(cached.assets[0].from_versions_host);
    assert!(!crate::versions_host::is_mirrored_asset_api_url(api_url));
    remember_mirrored_assets([&cached]);
    assert!(crate::versions_host::is_mirrored_asset_api_url(api_url));

    // GitHub's own data never carries the flag.
    let direct = serde_json::to_string(&make_asset("tool.tar.gz")).unwrap();
    assert!(!direct.contains("from_versions_host"), "{direct}");
}

#[test]
fn test_same_release_download() {
    let url = "https://github.com/o/r/releases/download/v1.0.0/tool-linux.tar.gz";
    assert!(same_release_download(url, url));
    // A renamed repository: same tag and file.
    assert!(same_release_download(
        url,
        "https://github.com/new-o/new-r/releases/download/v1.0.0/tool-linux.tar.gz"
    ));
    // Encoding differences don't matter.
    assert!(same_release_download(
        "https://github.com/o/r/releases/download/release%2F1/a%20b.zip",
        "https://github.com/o/r/releases/download/release/1/a b.zip"
    ));
    // Another file, another tag, or not a download.
    assert!(!same_release_download(
        url,
        "https://github.com/o/r/releases/download/v1.0.0/tool-windows.zip"
    ));
    assert!(!same_release_download(
        url,
        "https://github.com/o/r/releases/download/v0.9.0/tool-linux.tar.gz"
    ));
    assert!(!same_release_download(url, "https://github.com/o/r"));
}

#[test]
fn test_github_content_headers_request_raw_content() {
    let headers = get_headers("https://api.github.com/repos/o/r/contents/bin/tool").unwrap();

    assert_eq!(headers.get("accept").unwrap(), "application/vnd.github.raw");
}

#[tokio::test]
async fn test_pick_reachable_asset_url_skips_probe_when_urls_equal() {
    // When both URLs are identical there is nothing to fall back to, so no request is
    // made and the URL is returned as-is.
    assert_eq!(
        pick_reachable_asset_url(ASSET_API_URL, ASSET_API_URL).await,
        ASSET_API_URL
    );
}

#[test]
fn test_release_asset_from_url_parses_browser_download_urls() {
    assert_eq!(
        release_asset_from_url(
            "https://github.com/owner/repo/releases/download/v1.2.3/tool-aarch64.tar.gz"
        ),
        Some((
            "owner/repo".to_string(),
            "v1.2.3".to_string(),
            "tool-aarch64.tar.gz".to_string()
        ))
    );
}

#[test]
fn test_release_asset_from_url_decodes_tag_and_asset() {
    assert_eq!(
        release_asset_from_url(
            "https://github.com/owner/repo/releases/download/v1%2Bmeta/tool%20name.tar.gz"
        ),
        Some((
            "owner/repo".to_string(),
            "v1+meta".to_string(),
            "tool name.tar.gz".to_string()
        ))
    );
}

#[test]
fn test_release_asset_from_url_keeps_a_tag_that_spans_segments() {
    // GitHub percent-encodes `@` but leaves a tag's `/` as a path
    // separator, so the asset is the last segment and the tag is the rest.
    assert_eq!(
        release_asset_from_url(
            "https://github.com/biomejs/biome/releases/download/%40biomejs/biome%402.5.2/biome-linux-x64"
        ),
        Some((
            "biomejs/biome".to_string(),
            "@biomejs/biome@2.5.2".to_string(),
            "biome-linux-x64".to_string()
        ))
    );
}

#[test]
fn test_release_asset_from_url_ignores_non_release_urls() {
    assert_eq!(
        release_asset_from_url("https://example.com/owner/repo/releases/download/v1/tool"),
        None
    );
    assert_eq!(
        release_asset_from_url("https://github.com/owner/repo/archive/refs/tags/v1.tar.gz"),
        None
    );
    // An API asset endpoint is already what the fallback resolves to, so it
    // is not itself a browser URL to resolve.
    assert_eq!(
        release_asset_from_url("https://api.github.com/repos/owner/repo/releases/assets/1"),
        None
    );
    // A tag with no asset after it names no file.
    assert_eq!(
        release_asset_from_url("https://github.com/owner/repo/releases/download/v1"),
        None
    );
}
