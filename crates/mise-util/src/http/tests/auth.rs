use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn test_github_oauth_401_refreshes_and_retries_once() {
    let (port, count, requests) = spawn_recording_server(vec![
        unauthorized_response(),
        github_oauth_token_response(),
        json_empty_array_response(),
    ])
    .await;
    let server_url = format!("http://127.0.0.1:{port}");
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("github-oauth-tokens.toml");
    let _guard = set_test_github_oauth(&server_url, cache_path.clone());
    seed_github_oauth_cache(&cache_path);

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer ghu-stale"));
    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
    let text = client
        .get_text_request(format!("{server_url}/api/v3/repos/owner/repo/releases"))
        .headers(&headers)
        .send()
        .await
        .unwrap_or_else(|err| {
            let requests = requests.lock().unwrap();
            panic!(
                "request failed: {err:#}\nrequests:\n{}",
                requests.join("\n---\n")
            );
        });

    assert_eq!(text, "[]");
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
    let requests = requests.lock().unwrap();
    let first_request = requests[0].to_ascii_lowercase();
    let refresh_request = requests[1].to_ascii_lowercase();
    let retry_request = requests[2].to_ascii_lowercase();
    assert!(first_request.contains("get /api/v3/repos/owner/repo/releases"));
    assert!(first_request.contains("authorization: bearer ghu-stale"));
    assert!(refresh_request.contains("post /login/oauth/access_token"));
    assert!(retry_request.contains("get /api/v3/repos/owner/repo/releases"));
    assert!(retry_request.contains("authorization: bearer ghu-refreshed"));
    let cache = std::fs::read_to_string(cache_path).unwrap();
    assert!(cache.contains("ghu-refreshed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_github_oauth_401_reports_refreshed_token_source() {
    let (port, count, _requests) = spawn_recording_server(vec![
        unauthorized_response(),
        github_oauth_token_response(),
        unauthorized_response(),
    ])
    .await;
    let server_url = format!("http://127.0.0.1:{port}");
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("github-oauth-tokens.toml");
    let _guard = set_test_github_oauth(&server_url, cache_path.clone());
    seed_github_oauth_cache(&cache_path);

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer ghu-stale"));
    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
    let err = client
        .get_text_request(format!("{server_url}/api/v3/repos/owner/repo/releases"))
        .headers(&headers)
        .send()
        .await
        .unwrap_err();
    let msg = format!("{err:?}");

    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert!(
        msg.contains("github auth: yes (token from GitHub OAuth)"),
        "{msg}"
    );
    assert!(
        msg.contains("token from GitHub OAuth was rejected by GitHub"),
        "{msg}"
    );
}

/// Drive a real request through the client so the whole chain is covered:
/// the 403 handler calling `is_github_rate_limited`, the marker reaching
/// `is_transient`, and the retry loop acting on it. Asserting on the
/// request count is the point — passing the flag to
/// `github_forbidden_report` by hand would still pass if the call site
/// stopped supplying it. `/api/v3/` makes the loopback host a GitHub API
/// URL, the same trick the OAuth tests use.
async fn send_github_403(response: &'static str, retries: i64) -> (Report, usize) {
    let attempts = retries as usize + 1;
    let (port, count) = spawn_canned_server(vec![response; 8]).await;
    let _guard = set_test_http_retries(retries);
    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
    let err = client
        .get_text_request(format!(
            "http://127.0.0.1:{port}/api/v3/repos/aubepkg/aube/contents/x.json"
        ))
        .send()
        .await
        .expect_err("403 should be an error");
    let seen = count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        seen <= attempts,
        "server saw {seen} requests, more than the {attempts} allowed"
    );
    (err, seen)
}

/// An exhausted primary limit must be retried. GitHub reports it as 403,
/// which `is_transient`'s status check treats as deterministic, so without
/// the marker a rate limit fails on the first attempt.
#[tokio::test(flavor = "current_thread")]
async fn test_github_rate_limited_403_is_retried() {
    let (err, seen) = send_github_403(github_rate_limited_response(), 3).await;

    assert_eq!(seen, 4, "exhausted limit should be retried: {err:?}");
    assert!(is_transient(&err), "{err:?}");
    // The marker must not change what the user reads. The auth line is
    // deliberately not asserted on: whether a token is attached depends on
    // the environment the test runs in, and `is_github_api_url` matches the
    // `/api/v3/` path used here. `github_forbidden_report` covers it.
    let msg = format!("{err:?}");
    assert!(
        msg.contains("github rate limit: 0/5000 (core), resets at 1789456621"),
        "{msg}"
    );
    assert!(
        msg.contains("API rate limit exceeded for installation"),
        "{msg}"
    );
}

/// The secondary-limit branch: quota remains, but `retry-after` says to
/// back off. Covered separately so neither detector can regress alone.
#[tokio::test(flavor = "current_thread")]
async fn test_github_secondary_rate_limited_403_is_retried() {
    let (err, seen) = send_github_403(github_secondary_rate_limited_response(), 2).await;

    assert_eq!(seen, 3, "secondary limit should be retried: {err:?}");
    assert!(is_transient(&err), "{err:?}");
    assert!(
        format!("{err:?}").contains("secondary rate limit"),
        "{err:?}"
    );
}

/// A 403 with quota left and no `retry-after` is a refusal, not a rate
/// limit. It must stay deterministic so mise does not spend the whole
/// backoff on something that cannot succeed.
#[tokio::test(flavor = "current_thread")]
async fn test_github_forbidden_with_quota_left_is_not_retried() {
    let (err, seen) = send_github_403(github_forbidden_response(), 3).await;

    assert_eq!(seen, 1, "a refusal should not be retried: {err:?}");
    assert!(!is_transient(&err), "{err:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn test_github_forbidden_report_includes_body_and_auth_state() {
    let (port, _count) = spawn_canned_server(vec![github_forbidden_response()]).await;
    let url = format!("http://127.0.0.1:{port}/repos/microsoft/edit/releases");
    let resp = reqwest::Client::new().get(url).send().await.unwrap();
    let rate_limit = github_rate_limit_summary(&resp);
    let status_error = resp
        .error_for_status_ref()
        .expect_err("403 response should be an error");
    let body = resp.text().await.unwrap();
    let err = github_forbidden_report(status_error, true, rate_limit, false, &body);
    let msg = format!("{err:?}");

    assert!(msg.contains("github auth: yes"));
    assert!(msg.contains("github rate limit: 42/5000 (core), resets at 1781337353"));
    assert!(msg.contains(r#"{"message":"secondary rate limit","docs":"url"}"#));
}

#[tokio::test(flavor = "current_thread")]
async fn test_github_unauthorized_report_names_token_source() {
    // env var known → the message names it and includes the token-guide hint.
    let (port, _count) = spawn_canned_server(vec![unauthorized_response()]).await;
    let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");
    let resp = reqwest::Client::new().get(url).send().await.unwrap();
    let status_error = resp
        .error_for_status_ref()
        .expect_err("401 response should be an error");
    let body = resp.text().await.unwrap();
    let err = github_unauthorized_report(
        status_error,
        true,
        Some(&crate::github::TokenSource::EnvVar("GITHUB_TOKEN")),
        &body,
    );
    let msg = format!("{err:?}");

    assert!(
        msg.contains("github auth: yes (token from GITHUB_TOKEN)"),
        "{msg}"
    );
    assert!(msg.contains("Bad credentials"), "{msg}");
    assert!(
        msg.contains("token in `GITHUB_TOKEN` was rejected by GitHub (401 Unauthorized)"),
        "{msg}"
    );
    assert!(msg.contains("github-tokens.html"), "{msg}");
}

#[tokio::test(flavor = "current_thread")]
async fn test_github_unauthorized_report_names_non_env_token_sources() {
    let sources = [
        (crate::github::TokenSource::TokensFile, "github_tokens.toml"),
        (crate::github::TokenSource::GhCli, "gh CLI (hosts.yml)"),
        (
            crate::github::TokenSource::CredentialCommand,
            "credential_command",
        ),
        (crate::github::TokenSource::GithubOauth, "GitHub OAuth"),
        (
            crate::github::TokenSource::GitCredential,
            "git credential fill",
        ),
    ];
    let (port, _count) = spawn_canned_server(vec![unauthorized_response(); sources.len()]).await;
    let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");

    for (source, label) in sources {
        let resp = reqwest::Client::new().get(&url).send().await.unwrap();
        let status_error = resp.error_for_status_ref().unwrap_err();
        let body = resp.text().await.unwrap();
        let msg = format!(
            "{:?}",
            github_unauthorized_report(status_error, true, Some(&source), &body)
        );

        assert!(
            msg.contains(&format!("github auth: yes (token from {label})")),
            "{msg}"
        );
        assert!(
            msg.contains(&format!("token from {label} was rejected by GitHub")),
            "{msg}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_github_unauthorized_report_without_known_source() {
    // Token used but source unknown → generic auth "yes" and generic hint;
    // no token → auth "no" and no hint.
    let (port, _count) =
        spawn_canned_server(vec![unauthorized_response(), unauthorized_response()]).await;
    let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");

    let resp = reqwest::Client::new().get(&url).send().await.unwrap();
    let status_error = resp.error_for_status_ref().unwrap_err();
    let body = resp.text().await.unwrap();
    let used_msg = format!(
        "{:?}",
        github_unauthorized_report(status_error, true, None, &body)
    );
    assert!(used_msg.contains("github auth: yes"), "{used_msg}");
    assert!(!used_msg.contains("token from"), "{used_msg}");
    assert!(used_msg.contains("configured GitHub token"), "{used_msg}");

    let resp = reqwest::Client::new().get(&url).send().await.unwrap();
    let status_error = resp.error_for_status_ref().unwrap_err();
    let body = resp.text().await.unwrap();
    let anon_msg = format!(
        "{:?}",
        github_unauthorized_report(status_error, false, None, &body)
    );
    assert!(anon_msg.contains("github auth: no"), "{anon_msg}");
    assert!(!anon_msg.contains("hint:"), "{anon_msg}");
}

#[tokio::test(flavor = "current_thread")]
async fn test_github_unauthorized_report_ignores_source_when_no_auth_sent() {
    // A GitHub token env var may be present in the process even when this
    // request sent no Authorization header; it must not be reported as used.
    let (port, _count) = spawn_canned_server(vec![unauthorized_response()]).await;
    let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");
    let resp = reqwest::Client::new().get(url).send().await.unwrap();
    let status_error = resp.error_for_status_ref().unwrap_err();
    let body = resp.text().await.unwrap();
    let msg = format!(
        "{:?}",
        github_unauthorized_report(
            status_error,
            false,
            Some(&crate::github::TokenSource::EnvVar("GITHUB_TOKEN")),
            &body
        )
    );

    assert!(msg.contains("github auth: no"), "{msg}");
    assert!(!msg.contains("token from"), "{msg}");
    assert!(!msg.contains("hint:"), "{msg}");
}

#[tokio::test(flavor = "current_thread")]
async fn test_read_bounded_error_body_caps_large_body() {
    // An oversized error body must be truncated during reading, not buffered
    // whole, so a hostile endpoint can't exhaust memory.
    let big_body = "x".repeat(MAX_ERROR_BODY_BYTES + 4096);
    let raw = format!(
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        big_body.len(),
        big_body
    );
    let leaked: &'static str = Box::leak(raw.into_boxed_str());
    let (port, _count) = spawn_canned_server(vec![leaked]).await;
    let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");
    let resp = reqwest::Client::new().get(url).send().await.unwrap();

    let body = read_bounded_error_body(resp, Duration::from_secs(30)).await;
    assert_eq!(body.len(), MAX_ERROR_BODY_BYTES);
}

#[tokio::test(flavor = "current_thread")]
async fn test_read_bounded_error_body_honors_deadline() {
    // A response body that trickles forever (staying under the byte cap and
    // the idle read_timeout) must still be abandoned at the deadline instead
    // of blocking indefinitely.
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            // No Content-Length + `close` → body is read until EOF, which the
            // server never sends; it just trickles one byte at a time.
            let _ = sock
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n")
                .await;
            loop {
                if sock.write_all(b"x").await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    });
    let url = format!("http://127.0.0.1:{port}/repos/owner/repo/releases");
    let resp = reqwest::Client::new().get(url).send().await.unwrap();

    let start = tokio::time::Instant::now();
    let body = read_bounded_error_body(resp, Duration::from_millis(150)).await;
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "read must stop at the deadline"
    );
    assert!(
        body.is_empty(),
        "timed-out read yields no body, got {body:?}"
    );
}

#[test]
fn test_netrc_should_apply_treats_netrc_as_fallback() {
    // No existing auth → netrc fills in (normal fallback).
    assert!(netrc_should_apply(false, false));
    // Explicit auth (e.g. forge token) on a same-host request →
    // netrc must NOT clobber it. This is the regression guard for
    // private GitHub release-asset downloads where a netrc github
    // entry was overriding the resolved Bearer token.
    assert!(!netrc_should_apply(false, true));
    // Host changed via URL replacement → existing auth was built for the
    // original host, so netrc (scoped to the new host) wins.
    assert!(netrc_should_apply(true, true));
    assert!(netrc_should_apply(true, false));
}

fn basic_netrc_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(AUTHORIZATION, HeaderValue::from_static("Basic bmV0cmM="));
    h
}

fn auth_value(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(AUTHORIZATION)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect()
}

#[test]
fn test_apply_netrc_keeps_forge_token_on_un_redirected_url() {
    // Regression: a netrc entry for api.github.com must NOT override the
    // Bearer forge token when the URL was not rewritten. Previously this
    // clobbered the token and broke private release-asset downloads.
    let url: Url = "https://api.github.com/repos/o/r/releases/assets/1"
        .parse()
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer forge-token"),
    );

    let out = apply_netrc_credentials(headers, &url, &url, basic_netrc_headers());
    // Exactly one Authorization header, still the forge token.
    assert_eq!(auth_value(&out), vec!["Bearer forge-token".to_string()]);
}

#[test]
fn test_apply_netrc_fills_in_when_no_existing_auth() {
    let url: Url = "https://example.com/file".parse().unwrap();
    let out = apply_netrc_credentials(HeaderMap::new(), &url, &url, basic_netrc_headers());
    assert_eq!(auth_value(&out), vec!["Basic bmV0cmM=".to_string()]);
}

#[test]
fn test_apply_netrc_overrides_existing_auth_when_url_redirected() {
    // #7164 use case: a URL replacement redirected the request to a
    // private mirror. The pre-existing auth header was built for the
    // original host, so netrc (scoped to the new host) must win — and
    // replace, not duplicate, the Authorization header.
    let original: Url = "https://public.example.com/file".parse().unwrap();
    let redirected: Url = "https://mirror.internal/file".parse().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer stale"));

    let out = apply_netrc_credentials(headers, &original, &redirected, basic_netrc_headers());
    assert_eq!(auth_value(&out), vec!["Basic bmV0cmM=".to_string()]);
}

#[test]
fn test_cross_host_replacement_clears_credentials_without_netrc() {
    let original: Url = "https://user:password@public.example.com/file"
        .parse()
        .unwrap();
    let mut redirected: Url = "https://user:password@mirror.internal/file"
        .parse()
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer stale"));
    headers.insert("x-api-key", HeaderValue::from_static("secret"));
    headers.insert("x-request-id", HeaderValue::from_static("keep-me"));

    clear_cross_host_credentials(&mut headers, &original, &mut redirected);

    assert!(!headers.contains_key(AUTHORIZATION));
    assert!(!headers.contains_key("x-api-key"));
    assert_eq!(headers["x-request-id"], "keep-me");
    assert!(redirected.username().is_empty());
    assert!(redirected.password().is_none());
}

#[test]
fn test_apply_netrc_keeps_forge_token_on_same_host_path_rewrite() {
    // A URL replacement that only rewrites the path/query on the SAME host
    // must not let netrc override the forge token: the token is still valid
    // for that host, and netrc is host-scoped anyway.
    let original: Url = "https://github.com/o/r/releases/download/v1/f.tar.gz"
        .parse()
        .unwrap();
    let rewritten: Url = "https://github.com/o/r/releases/download/v1/f-linux.tar.gz"
        .parse()
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer forge-token"),
    );

    let out = apply_netrc_credentials(headers, &original, &rewritten, basic_netrc_headers());
    assert_eq!(auth_value(&out), vec!["Bearer forge-token".to_string()]);
}

#[test]
fn test_rejects_credentials_on_https_to_http_replacement() {
    let original: Url = "https://public.example.com/file".parse().unwrap();
    let rewritten: Url = "http://mirror.internal/file".parse().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));

    let err = ensure_secure_replacement_credentials(&original, &rewritten, &headers).unwrap_err();
    assert!(err.to_string().contains("refusing to send credentials"));
    let mut cookie_headers = HeaderMap::new();
    cookie_headers.insert(COOKIE, HeaderValue::from_static("session=secret"));
    assert!(ensure_secure_replacement_credentials(&original, &rewritten, &cookie_headers).is_err());
    let mut api_key_headers = HeaderMap::new();
    api_key_headers.insert("x-api-key", HeaderValue::from_static("secret"));
    assert!(
        ensure_secure_replacement_credentials(&original, &rewritten, &api_key_headers).is_err()
    );
    let rewritten_with_credentials: Url =
        "http://user:password@mirror.internal/file".parse().unwrap();
    assert!(
        ensure_secure_replacement_credentials(
            &original,
            &rewritten_with_credentials,
            &HeaderMap::new(),
        )
        .is_err()
    );
    assert!(
        ensure_secure_replacement_credentials(&original, &rewritten, &HeaderMap::new()).is_ok()
    );
    assert!(
        ensure_secure_replacement_credentials(
            &"http://public.example.com/file".parse().unwrap(),
            &rewritten,
            &headers,
        )
        .is_ok()
    );
}
