use super::*;

fn unavailable(status: Option<u16>) -> eyre::Report {
    crate::versions_host::VersionsHostUnavailable {
        status,
        message: format!("mise-versions failed with {status:?}"),
    }
    .into()
}

/// Runs [`mirror_or_github`] over `mirrored`, recording whether GitHub was
/// asked; GitHub answers `github`.
async fn fallback(
    mirrored: eyre::Result<Option<&'static str>>,
    github: eyre::Result<Option<&'static str>>,
) -> (eyre::Result<Option<&'static str>>, bool) {
    let asked = std::cell::Cell::new(false);
    let got = mirror_or_github("GitHub release o/r@v1", mirrored, async || {
        asked.set(true);
        github
    })
    .await;
    (got, asked.get())
}

#[tokio::test]
async fn test_mirror_errors_are_retried_on_github() {
    // No connection or a timeout, rate limited, and every 5xx.
    for status in [None, Some(429), Some(500), Some(502), Some(503), Some(504)] {
        let (got, asked) = fallback(Err(unavailable(status)), Ok(Some("github"))).await;
        assert!(asked, "status {status:?}");
        assert_eq!(got.unwrap(), Some("github"), "status {status:?}");
    }
}

#[tokio::test]
async fn test_mirror_answers_are_not_retried_on_github() {
    // Found, a 404, and an empty answer are all mise-versions' to give.
    for mirrored in [Some("mirror"), None, Some("")] {
        let (got, asked) = fallback(Ok(mirrored), Ok(Some("github"))).await;
        assert!(!asked, "{mirrored:?}");
        assert_eq!(got.unwrap(), mirrored);
    }
}

#[tokio::test]
async fn test_mirror_failures_github_cant_fix_are_not_retried() {
    // A 403 (the mirror disabled or restricted), another 4xx, and an answer
    // that doesn't check out.
    for err in [
        unavailable(Some(403)),
        unavailable(Some(400)),
        eyre::eyre!("mise-versions answered for o/r@v1 with release v2"),
    ] {
        let (got, asked) = fallback(Err(err), Ok(Some("github"))).await;
        assert!(!asked);
        let message = format!("{:#}", got.unwrap_err());
        assert!(message.contains("use_versions_host = false"), "{message}");
    }
}

#[tokio::test]
async fn test_a_failed_retry_names_both_failures() {
    let (got, asked) = fallback(
        Err(unavailable(Some(503))),
        Err(eyre::eyre!(
            "HTTP status client error (403 Forbidden) for url (https://api.github.com/repos/o/r/releases/tags/v1)"
        )),
    )
    .await;
    assert!(asked);
    let err = got.unwrap_err();
    let message = format!("{err:#}");
    assert!(
        message.contains("mise-versions failed with Some(503)"),
        "{message}"
    );
    assert!(
        message.contains("403 Forbidden) for url (https://api.github.com"),
        "{message}"
    );
    // main.rs looks for GitHub's 403 in the Debug output to add its token hint.
    assert!(
        format!("{err:?}")
            .contains("HTTP status client error (403 Forbidden) for url (https://api.github.com")
    );
    // Both sources failing still lets a copy cached earlier stand in.
    assert!(crate::versions_host::is_temporarily_unavailable(&err));
}
