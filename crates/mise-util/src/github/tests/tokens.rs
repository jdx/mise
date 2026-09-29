use super::*;

#[test]
fn test_token_source_memo_matches_only_the_supplying_token_and_host() {
    let host = "github-source-test.example.com";
    remember_token_source(host, "ghp_test", TokenSource::EnvVar("GITHUB_TOKEN"));

    assert_eq!(
        token_source_for_token(host, "ghp_test"),
        Some(TokenSource::EnvVar("GITHUB_TOKEN"))
    );
    assert_eq!(token_source_for_token(host, "from-netrc"), None);
    assert_eq!(
        token_source_for_token("another-github.example.com", "ghp_test"),
        None
    );
}

#[test]
fn test_get_headers_remembers_tokens_file_source() {
    let _lock = crate::testing::lock_ignoring_poison(&TEST_ENV_LOCK);
    let host = "github-tokens-file-test.example.com";
    let _tokens_file = TokensFileOverrideGuard::set(host, "ghp_from_tokens_file");

    let headers = get_headers(format!("https://{host}/api/v3/repos/owner/repo/releases")).unwrap();

    assert_eq!(
        headers.get(reqwest::header::AUTHORIZATION).unwrap(),
        "Bearer ghp_from_tokens_file"
    );
    assert_eq!(
        token_source_for_token(host, "ghp_from_tokens_file"),
        Some(TokenSource::TokensFile)
    );
}

/// A token carrying a character that is illegal in an HTTP header used to
/// abort the process from `HeaderValue::from_str(..).unwrap()` (#13471).
/// It has to come back as an error, and the error must not leak the token.
#[test]
fn test_get_headers_rejects_token_with_invalid_header_character() {
    let _lock = crate::testing::lock_ignoring_poison(&TEST_ENV_LOCK);
    let host = "github-bad-token-test.example.com";
    let _tokens_file = TokensFileOverrideGuard::set(host, "ghp_bad\nsecret_value");

    let err = get_headers(format!("https://{host}/api/v3/repos/owner/repo/releases"))
        .expect_err("a token that cannot be a header value must be an error, not a panic");
    let msg = err.to_string();

    assert!(msg.contains("invalid GitHub token"), "{msg}");
    assert!(
        !msg.contains("secret_value"),
        "token leaked into error: {msg}"
    );
}

/// A malformed token must not take down an unauthenticated request to an
/// unrelated host: `get_headers` only builds an Authorization header for
/// the host the token belongs to.
#[test]
fn test_get_headers_ignores_invalid_token_for_other_hosts() {
    let _lock = crate::testing::lock_ignoring_poison(&TEST_ENV_LOCK);
    let _tokens_file =
        TokensFileOverrideGuard::set("github-bad-token-other.example.com", "ghp_bad\ntoken");

    let headers = get_headers("https://downloads.example.com/tool.tar.gz")
        .expect("a non-API URL builds no auth header");

    assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
}

#[test]
fn test_parse_github_tokens() {
    let toml = r#"
[tokens."github.com"]
token = "ghp_abc123"

[tokens."github.mycompany.com"]
token = "ghp_def456"
"#;
    let result = parse_github_tokens(toml).unwrap();
    assert_eq!(result.get("github.com").unwrap(), "ghp_abc123");
    assert_eq!(result.get("github.mycompany.com").unwrap(), "ghp_def456");
}

#[test]
fn test_parse_github_tokens_empty() {
    assert!(parse_github_tokens("").is_none());
}

#[test]
fn test_parse_github_tokens_empty_tokens() {
    let toml = "[tokens]\n";
    let result = parse_github_tokens(toml).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_parse_github_tokens_missing_token_field() {
    let toml = r#"
[tokens."github.com"]
something_else = "value"
"#;
    let result = parse_github_tokens(toml).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_api_host_token_lookup_hosts() {
    assert_eq!(
        token_lookup_hosts("api.github.com"),
        vec!["github.com", "api.github.com"]
    );
    assert_eq!(
        token_lookup_hosts("api.octocorp.ghe.com"),
        vec!["octocorp.ghe.com", "api.octocorp.ghe.com"]
    );
    assert_eq!(
        token_lookup_hosts("github.example.com"),
        vec!["github.example.com"]
    );
}

/// An enterprise token is scoped to a private GHES instance and must never
/// be sent to a public github.com service. raw.githubusercontent.com is the
/// trap: it is neither `github.com` nor `api.github.com` literally, so a
/// host-string comparison classifies it as enterprise and leaks the token.
#[test]
fn test_enterprise_token_is_not_sent_to_public_raw_content() {
    // Takes the env lock, snapshots the token vars, restores them on drop.
    let _guard = GithubTokenGuard::new();
    // Only an enterprise token configured, which is the leaking case.
    env::remove_var("GITHUB_TOKEN");
    env::set_var("MISE_GITHUB_ENTERPRISE_TOKEN", "ghes-secret");

    for host in ["raw.githubusercontent.com", "github.com", "api.github.com"] {
        // `resolve_token` would allow git credential helpers to run, which
        // can block or prompt; the enterprise exclusion is unaffected by
        // skipping them.
        if let Some((token, _)) = resolve_token_inner(host, false) {
            assert_ne!(
                token, "ghes-secret",
                "{host} must not receive MISE_GITHUB_ENTERPRISE_TOKEN"
            );
        }
    }
}

/// The token must never ride a cleartext request.
#[test]
fn test_raw_githubusercontent_over_http_gets_no_token() {
    with_github_token(|| {
        let headers =
            get_headers("http://raw.githubusercontent.com/owner/repo/main/file.txt").unwrap();
        assert!(
            !headers.contains_key(reqwest::header::AUTHORIZATION),
            "an http raw-content URL must not carry the token"
        );
    });
}

/// A private repository's raw file is a 404 without a token and a 200 with
/// one, so this host does need the bearer token. Pinned separately from the
/// API hosts because it must NOT also receive the API version header, and
/// separately from the asset hosts, which must receive no token at all.
#[test]
fn test_raw_githubusercontent_uses_github_token() {
    with_github_token(|| {
        let headers =
            get_headers("https://raw.githubusercontent.com/owner/repo/main/file.txt").unwrap();
        assert!(
            headers.contains_key(reqwest::header::AUTHORIZATION),
            "raw.githubusercontent.com should carry the github.com token"
        );
        assert!(
            !headers.contains_key("x-github-api-version"),
            "raw.githubusercontent.com is not an API host"
        );
    });
}

#[test]
fn test_only_github_api_urls_use_github_token() {
    with_github_token(|| {
        for url in [
            "https://github.com/api/v3/repos/owner/repo/releases",
            "https://github.com/cuotos/ecs-exec-pf/releases/download/v0.3.0/ecs-exec-pf_0.3.0_Linux_x86_64.tar.gz",
            "https://github.example.com/owner/repo/releases/download/v1.0.0/file.tar.gz",
            "https://objects.githubusercontent.com/github-production-release-asset",
            "https://objects-origin.githubusercontent.com/github-production-release-asset",
            "https://release-assets.githubusercontent.com/github-production-release-asset",
            "https://octocorp.ghe.com/api/v3/repos/owner/repo/releases",
            "https://octocorp.ghe.com/owner/repo/releases/download/v1.0.0/file.tar.gz",
        ] {
            let headers = get_headers(url).unwrap();
            assert!(
                !headers.contains_key(reqwest::header::AUTHORIZATION),
                "{url} should not use GitHub auth"
            );
            assert!(
                !headers.contains_key("x-github-api-version"),
                "{url} should not use GitHub API version"
            );
        }

        let headers = get_headers("https://api.github.com/repos/owner/repo/releases").unwrap();
        assert!(headers.contains_key(reqwest::header::AUTHORIZATION));
        assert!(headers.contains_key("x-github-api-version"));

        let headers =
            get_headers("https://api.github.com/repos/owner/repo/releases/assets/1").unwrap();
        assert!(headers.contains_key(reqwest::header::AUTHORIZATION));
        assert_eq!(headers.get("accept").unwrap(), "application/octet-stream");

        let headers =
            get_headers("https://github.example.com/api/v3/repos/owner/repo/releases").unwrap();
        assert!(headers.contains_key(reqwest::header::AUTHORIZATION));
        assert!(headers.contains_key("x-github-api-version"));

        let headers =
            get_headers("https://api.octocorp.ghe.com/repos/owner/repo/releases").unwrap();
        assert!(headers.contains_key(reqwest::header::AUTHORIZATION));
        assert!(headers.contains_key("x-github-api-version"));
    });
}

#[test]
fn test_get_headers_rejects_relative_url() {
    let err = get_headers("/repos/jdx/aube/releases").unwrap_err();
    assert!(
        err.to_string()
            .contains("invalid request URL for GitHub auth headers"),
        "unexpected error: {err}"
    );
}
