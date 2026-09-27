use super::*;

#[test]
fn test_format_response_body_handles_empty_and_truncates() {
    assert_eq!(format_response_body(" \n\t"), "<empty>");

    let body = "a".repeat(4097);
    let formatted = format_response_body(&body);
    assert_eq!(formatted.strip_suffix("\n<truncated>").unwrap().len(), 4096);
    assert!(formatted.ends_with("\n<truncated>"));
}

#[tokio::test(flavor = "current_thread")]
async fn test_retry_rescues_send_failure_with_no_response() {
    // A connection that is accepted and then closed without a response fails
    // with a reqwest "request" error: connecting succeeded, so it is not
    // is_connect(), and no response arrived, so there is no status. HTTP/2
    // REFUSED_STREAM lands in the same class. Before these were classified as
    // transient, such failures exited on the first attempt even with retries
    // enabled.
    let _guard = set_test_http_retries(1);
    let (port, count) = spawn_canned_server(vec!["", ok_response()]).await;
    let url: Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let reporter = crate::testing::RecordingReport::default();
    let resp =
        crate::resolve_progress::scope(Some(Box::new(reporter.clone())), client.get_async(url))
            .await
            .unwrap();

    let messages = reporter.0.lock().unwrap();
    assert!(
        messages
            .first()
            .unwrap()
            .contains("fetching from 127.0.0.1")
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("retrying 127.0.0.1 (attempt 2"))
    );
    assert!(messages.last().unwrap().contains("fetching from 127.0.0.1"));
    assert!(resp.status().is_success());
    // Two connections: the aborted one, then the retry that succeeded.
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn test_retry_succeeds_after_two_502s() {
    // 2 retries is enough to verify the rescue path (2 failures + 1 success)
    // without paying the third backoff (~12.5s).
    let _guard = set_test_http_retries(2);
    let (port, count) = spawn_canned_server(vec![
        bad_gateway_response(),
        bad_gateway_response(),
        ok_response(),
    ])
    .await;
    let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let resp = client.get_async(url).await.unwrap();
    assert!(resp.status().is_success());
    // Should have served 3 connections: two 502s + one 200.
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn test_prefer_offline_disables_http_retries() {
    let _guard = set_test_prefer_offline(3);
    let (port, count) = spawn_canned_server(vec![bad_gateway_response(), ok_response()]).await;
    let url: Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let err = client.get_async(url).await.unwrap_err();

    assert!(format!("{err:?}").contains("502"));
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        crate::network::fetch_remote_versions_timeout(&Settings::get()),
        Duration::from_secs(3)
    );
}

#[test]
fn test_fetch_client_applies_prefer_offline_timeout_at_request_time() {
    let client = Client::new(Duration::from_secs(30), ClientKind::Fetch).unwrap();
    let _guard = set_test_prefer_offline(3);

    assert_eq!(client.request_timeout(), Duration::from_secs(3));
}

/// Every client kind names its traffic, which is also what proves each one
/// reaches `https_downgrade_policy`: `Client::build` applies the policy
/// unconditionally using this, so there is no arm that can lack it.
#[test]
fn test_every_client_kind_has_a_redirect_subject() {
    let subjects = [ClientKind::Http, ClientKind::Fetch].map(ClientKind::redirect_subject);
    assert!(subjects.iter().all(|subject| !subject.is_empty()));
    assert_ne!(subjects[0], subjects[1]);
}

/// The predicate behind `https_downgrade_policy`, which every client uses.
/// The rejection itself cannot be exercised here: it needs a real HTTPS hop
/// to redirect away from, and mockito serves plain HTTP. Trusting a
/// self-signed cert would mean adding a test-only trust bypass to the
/// client, which is a worse trade than the gap it closes.
#[test]
fn test_https_downgrade_policy_rejects_https_to_http() {
    let https = Url::parse("https://example.com/versions").unwrap();
    let other_https = Url::parse("https://cdn.example.com/versions").unwrap();
    let http = Url::parse("http://cdn.example.com/versions").unwrap();

    assert!(is_https_downgrade(std::slice::from_ref(&https), &http));
    assert!(!is_https_downgrade(
        std::slice::from_ref(&https),
        &other_https
    ));
    assert!(!is_https_downgrade(&[http], &other_https));
}

/// Goes through `download_file_checksum_pinned` itself, so it covers the
/// client wiring and the verification. The downgrade redirect it allows
/// cannot be served here for the reason given on the test above.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn test_checksum_pinned_download_keeps_match_and_removes_mismatch() {
    let _guard = set_test_http_retries(0);
    let (port, count) =
        spawn_canned_server(vec![full_download_response(), full_download_response()]).await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let helloworld_sha256 = "936a185caaa266bb9cbe981e9e05cb78cd732b0b3280eb944412bb6f8f8f07af";

    let matching = dir.path().join("matching");
    download_file_checksum_pinned(&url, &matching, helloworld_sha256, None)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&matching).unwrap(), b"helloworld");

    let mismatched = dir.path().join("mismatched");
    let _err = download_file_checksum_pinned(&url, &mismatched, &"0".repeat(64), None)
        .await
        .unwrap_err();
    assert!(!mismatched.exists());
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn test_remote_fetch_command_keeps_full_budget_under_prefer_offline() {
    // Commands whose job is to enumerate remote versions/tags (`mise lock`,
    // `ls-remote`, ...) must honor the configured timeout and retries even
    // when prefer_offline is set.
    // https://github.com/jdx/mise/discussions/11185
    let client = Client::new(Duration::from_secs(30), ClientKind::Fetch).unwrap();
    let _guard = set_test_prefer_offline(3);
    let _remote_fetch_guard = AtomicBoolGuard::set(&crate::env::REMOTE_FETCH_COMMAND, true);

    assert_eq!(client.request_timeout(), Duration::from_secs(30));
    assert_eq!(
        crate::network::fetch_remote_versions_timeout(&Settings::get()),
        crate::network::configured_fetch_remote_versions_timeout(&Settings::get())
    );
    assert_eq!(crate::network::http_retries(&Settings::get()), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn test_reqwest_dns_error_is_not_transient_and_opens_circuit() {
    let _settings_guard = set_test_prefer_offline(3);
    let timeout = Duration::from_secs(3);
    let client = Client {
        reqwest: Ok(Client::_new()
            .no_proxy()
            .dns_resolver(FailingDnsResolver)
            .read_timeout(timeout)
            .connect_timeout(timeout)
            .build()
            .unwrap()),
        timeout,
        kind: ClientKind::Fetch,
    };
    let url: Url = "https://mise-dns-regression.invalid/?token=secret"
        .parse()
        .unwrap();
    let host_key = http_host_key(&url).unwrap();
    let _hosts_guard = UnavailableHostsGuard::new(vec![host_key.clone()]);

    let err = client.get_async(url).await.unwrap_err();

    assert!(is_dns_error(err.as_ref()), "unexpected error: {err:#}");
    assert!(!is_transient(&err));
    assert!(
        UNAVAILABLE_HTTP_HOSTS
            .lock()
            .unwrap()
            .contains_key(&host_key)
    );
    assert!(
        !UNAVAILABLE_HTTP_HOSTS
            .lock()
            .unwrap()
            .get(&host_key)
            .unwrap()
            .contains("token=secret")
    );
}

#[test]
fn test_only_download_size_mismatches_are_transient_eof_errors() {
    let mismatch: Report = DownloadSizeMismatch {
        expected: 10,
        actual: 5,
    }
    .into();
    assert!(is_transient(&mismatch));

    let unrelated: Report =
        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "local file truncated").into();
    assert!(!is_transient(&unrelated));
}

#[tokio::test(flavor = "current_thread")]
async fn test_circuit_broken_http_origin_falls_back_to_https() {
    let _settings_guard = set_test_prefer_offline(3);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let http_url: Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let https_url: Url = format!("https://127.0.0.1:{port}/").parse().unwrap();
    let http_origin = http_host_key(&http_url).unwrap();
    let https_origin = http_host_key(&https_url).unwrap();
    let _hosts_guard = UnavailableHostsGuard::new(vec![http_origin.clone(), https_origin]);
    UNAVAILABLE_HTTP_HOSTS
        .lock()
        .unwrap()
        .insert(http_origin, "connection refused".to_string());

    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_inner = accepted.clone();
    let server = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            accepted_inner.fetch_add(1, Ordering::SeqCst);
            let _ = socket.shutdown().await;
        }
    });

    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let err = client.get_async(http_url).await.unwrap_err();
    server.await.unwrap();

    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    assert!(!is_unavailable_http_host_error(&err));
}

#[test]
fn test_unavailable_host_error_preserves_original_cause() {
    let err: Report = UnavailableHttpHost {
        origin: "https://example.com:443".to_string(),
        cause: "connection refused".to_string(),
    }
    .into();

    assert!(is_unavailable_http_host_error(&err));
    assert!(err.to_string().contains("connection refused"));
}

#[tokio::test(flavor = "current_thread")]
async fn test_circuit_breaker_is_disabled_without_prefer_offline() {
    let _settings_guard = set_test_http_retries(0);
    let (port, count) = spawn_canned_server(vec![ok_response()]).await;
    let url: Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let host_key = http_host_key(&url).unwrap();
    let _hosts_guard = UnavailableHostsGuard::new(vec![host_key.clone()]);
    UNAVAILABLE_HTTP_HOSTS
        .lock()
        .unwrap()
        .insert(host_key, "connection refused".to_string());

    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let resp = client.get_async(url).await.unwrap();

    assert!(resp.status().is_success());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_parse_content_range() {
    assert_eq!(
        parse_content_range("bytes 5-9/10"),
        Some(ParsedContentRange::Bytes {
            start: 5,
            end: 9,
            total: 10
        })
    );
    assert_eq!(
        parse_content_range("bytes */10"),
        Some(ParsedContentRange::Unsatisfied { total: 10 })
    );
    assert_eq!(parse_content_range("bytes 5-10/10"), None);
    assert_eq!(parse_content_range("items 5-9/10"), None);
}

#[test]
fn test_response_validator_requires_strong_etag_or_last_modified() {
    let mut headers = HeaderMap::new();
    headers.insert(ETAG, HeaderValue::from_static("W/\"weak\""));
    assert_eq!(response_validator(&headers), None);

    headers.insert(
        LAST_MODIFIED,
        HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"),
    );
    assert_eq!(response_validator(&headers), None);

    headers.insert(
        DATE,
        HeaderValue::from_static("Wed, 21 Oct 2015 07:28:59 GMT"),
    );
    assert_eq!(response_validator(&headers), None);

    headers.insert(
        DATE,
        HeaderValue::from_static("Wed, 21 Oct 2015 07:29:00 GMT"),
    );
    assert_eq!(
        response_validator(&headers),
        Some(DownloadValidator::LastModified {
            value: "Wed, 21 Oct 2015 07:28:00 GMT".to_string(),
            response_date: "Wed, 21 Oct 2015 07:29:00 GMT".to_string(),
        })
    );

    headers.insert(ETAG, HeaderValue::from_static("\"strong\""));
    assert_eq!(
        response_validator(&headers),
        Some(DownloadValidator::Etag("\"strong\"".to_string()))
    );
}

#[test]
fn test_cleanup_download_dir_preserves_only_partial_pairs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("artifact.tar.gz"), b"complete").unwrap();
    std::fs::write(dir.path().join(".artifact.tar.gz.mise-part"), b"partial").unwrap();
    std::fs::write(
        dir.path().join(".artifact.tar.gz.mise-part.json"),
        b"metadata",
    )
    .unwrap();
    std::fs::write(dir.path().join(".orphan.mise-part"), b"orphan").unwrap();
    let expired_partial = dir.path().join(".expired.mise-part");
    let expired_state = dir.path().join(".expired.mise-part.json");
    std::fs::write(&expired_partial, b"expired partial").unwrap();
    std::fs::write(&expired_state, b"expired metadata").unwrap();
    let expired_time = filetime::FileTime::from_system_time(
        SystemTime::now() - PARTIAL_DOWNLOAD_MAX_AGE - Duration::from_secs(1),
    );
    filetime::set_file_mtime(&expired_partial, expired_time).unwrap();
    filetime::set_file_mtime(&expired_state, expired_time).unwrap();
    std::fs::create_dir(dir.path().join("extracted")).unwrap();

    cleanup_download_dir(dir.path()).unwrap();

    assert!(!dir.path().join("artifact.tar.gz").exists());
    assert!(!dir.path().join("extracted").exists());
    assert!(dir.path().join(".artifact.tar.gz.mise-part").exists());
    assert!(dir.path().join(".artifact.tar.gz.mise-part.json").exists());
    assert!(!dir.path().join(".orphan.mise-part").exists());
    assert!(!expired_partial.exists());
    assert!(!expired_state.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn test_no_retry_on_404() {
    let _guard = set_test_http_retries(3);
    let (port, count) = spawn_canned_server(vec![not_found_response()]).await;
    let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let err = client.get_async(url).await.unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("404"), "expected 404 in error: {msg}");
    // Should not have retried — only one connection.
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn test_retry_exhausted_on_persistent_500() {
    // Use 1 retry so the test doesn't pay the full backoff schedule;
    // the behavior under test (exhaustion → final error) is the same.
    let _guard = set_test_http_retries(1);
    // 2 connections: initial + 1 retry.
    let (port, count) =
        spawn_canned_server(vec![server_error_response(), server_error_response()]).await;
    let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let err = client.get_async(url).await.unwrap_err();
    assert!(format!("{err:?}").contains("500"));
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn test_text_request_can_override_retry_count() {
    let _guard = set_test_http_retries(3);
    let (port, count) = spawn_canned_server(vec![
        bad_gateway_response(),
        bad_gateway_response(),
        ok_response(),
    ])
    .await;
    let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let err = client
        .get_text_request(url)
        .retries(1)
        .send()
        .await
        .unwrap_err();
    assert!(format!("{err:?}").contains("502"));
    // Should stop after the initial request plus the single overridden retry.
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn test_text_request_respects_offline_mode() {
    let _guard = set_test_offline();
    let (port, count) = spawn_canned_server(vec![ok_response()]).await;
    let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let err = client.get_text_request(url).send().await.unwrap_err();
    assert_eq!(err.to_string(), "offline mode is enabled");
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn test_backoff_strategy_yields_requested_count_beyond_schedule() {
    // Regression: a fixed-length schedule used to silently cap retries at 4.
    // Now extra retries should fall back to the longest delay.
    let delays: Vec<_> = default_backoff_strategy(7).collect();
    assert_eq!(delays.len(), 7);
}

#[tokio::test(flavor = "current_thread")]
async fn test_retries_disabled_fails_immediately() {
    let _guard = set_test_http_retries(0);
    let (port, count) = spawn_canned_server(vec![bad_gateway_response()]).await;
    let url: Url = format!("http://127.0.0.1:{}/", port).parse().unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let err = client.get_async(url).await.unwrap_err();
    assert!(format!("{err:?}").contains("502"));
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}
