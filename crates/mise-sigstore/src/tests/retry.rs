use super::*;

#[test]
fn retryable_status_classification() {
    use reqwest::StatusCode;
    // Transient server-side conditions retry.
    assert!(is_retryable_status(StatusCode::GATEWAY_TIMEOUT)); // 504 — the reported failure
    assert!(is_retryable_status(StatusCode::BAD_GATEWAY)); // 502
    assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE)); // 503
    assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS)); // 429
    // Terminal conditions do not.
    assert!(!is_retryable_status(StatusCode::OK));
    assert!(!is_retryable_status(StatusCode::NOT_FOUND));
    assert!(!is_retryable_status(StatusCode::UNAUTHORIZED));
    assert!(!is_retryable_status(StatusCode::FORBIDDEN));
}

#[test]
fn backoff_grows_and_stays_within_jitter_bounds() {
    // Each attempt's delay must fall in [base/2, base) where base doubles.
    for attempt in 1..=4 {
        let base = DEFAULT_BACKOFF_BASE * (1u32 << (attempt - 1));
        let d = backoff_delay(DEFAULT_BACKOFF_BASE, attempt);
        assert!(d >= base / 2, "attempt {attempt}: {d:?} < {:?}", base / 2);
        assert!(d < base, "attempt {attempt}: {d:?} >= {base:?}");
    }
}

#[test]
fn backoff_zero_base_yields_no_delay() {
    for attempt in 1..=4 {
        assert_eq!(backoff_delay(Duration::ZERO, attempt), Duration::ZERO);
    }
}

/// Spawn a throwaway HTTP server that replies with each status in
/// `statuses` (one per connection, in order) then `200 {body}` for the
/// rest. A `429` reply carries `Retry-After: 0` so the retry path stays
/// fast. Returns the bound `base_url` and a counter of accepted connections.
fn flaky_server(
    statuses: Vec<u16>,
    body: &'static str,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_thread = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let n = hits_thread.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf); // drain the request line/headers
            let (code, payload) = match statuses.get(n) {
                Some(&s) => (s, ""),
                None => (200, body),
            };
            let extra = if code == 429 {
                "Retry-After: 0\r\n"
            } else {
                ""
            };
            let response = format!(
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (format!("http://{addr}"), hits)
}

/// Build a client pointed at a test server with zero backoff so retries
/// don't pay real wall-clock time.
fn test_client(base_url: &str) -> AttestationClient {
    AttestationClient::builder()
        .base_url(base_url)
        .backoff_base(Duration::ZERO)
        .build()
        .unwrap()
}

#[tokio::test]
async fn fetch_attestations_retries_on_5xx() {
    // 504 (the reported failure) then 502, then success — must recover.
    let (base_url, hits) = flaky_server(vec![504, 502], r#"{"attestations":[]}"#);
    let client = test_client(&base_url);
    let result = client
        .fetch_attestations(FetchParams {
            owner: "EarthBuild".to_string(),
            repo: Some("EarthBuild/earthbuild".to_string()),
            digest: "sha256:abc".to_string(),
            limit: 30,
            predicate_type: None,
        })
        .await;

    assert!(
        result.is_ok(),
        "expected recovery after retries: {result:?}"
    );
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "should have taken 2 failed + 1 successful attempt"
    );
}

#[tokio::test]
async fn fetch_attestations_surfaces_error_after_exhausting_retries() {
    // Persistent 504 — exhaust all attempts then surface the API error.
    let (base_url, hits) = flaky_server(vec![504, 504, 504, 504, 504], "");
    let client = test_client(&base_url);
    let err = client
        .fetch_attestations(FetchParams {
            owner: "EarthBuild".to_string(),
            repo: Some("EarthBuild/earthbuild".to_string()),
            digest: "sha256:abc".to_string(),
            limit: 30,
            predicate_type: None,
        })
        .await
        .unwrap_err();

    assert!(matches!(err, AttestationError::Api(_)), "got {err:?}");
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        DEFAULT_RETRIES + 1,
        "should stop after retries + 1 attempts"
    );
}

#[tokio::test]
async fn retries_setting_controls_attempt_count() {
    // retries(0) disables retries: a single 504 surfaces immediately.
    let (base_url, hits) = flaky_server(vec![504, 504], "");
    let client = AttestationClient::builder()
        .base_url(&base_url)
        .retries(0)
        .backoff_base(Duration::ZERO)
        .build()
        .unwrap();
    let err = client
        .fetch_attestations(FetchParams {
            owner: "EarthBuild".to_string(),
            repo: Some("EarthBuild/earthbuild".to_string()),
            digest: "sha256:abc".to_string(),
            limit: 30,
            predicate_type: None,
        })
        .await
        .unwrap_err();

    assert!(matches!(err, AttestationError::Api(_)), "got {err:?}");
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "retries(0) should make exactly one attempt"
    );
}

#[tokio::test]
async fn fetch_attestations_retries_on_429_with_retry_after() {
    // 429 carrying Retry-After (set by the server), then success.
    let (base_url, hits) = flaky_server(vec![429], r#"{"attestations":[]}"#);
    let client = test_client(&base_url);
    let result = client
        .fetch_attestations(FetchParams {
            owner: "EarthBuild".to_string(),
            repo: Some("EarthBuild/earthbuild".to_string()),
            digest: "sha256:abc".to_string(),
            limit: 30,
            predicate_type: None,
        })
        .await;

    assert!(result.is_ok(), "expected recovery after 429: {result:?}");
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "should have taken 1 rate-limited + 1 successful attempt"
    );
}

#[test]
fn retry_after_parses_delta_seconds_and_caps() {
    fn headers_with(value: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(reqwest::header::RETRY_AFTER, value.parse().unwrap());
        }
        headers
    }

    assert_eq!(
        retry_after_delay(&headers_with(Some("2"))),
        Some(Duration::from_secs(2))
    );
    // Capped at RETRY_AFTER_MAX.
    assert_eq!(
        retry_after_delay(&headers_with(Some("9999"))),
        Some(RETRY_AFTER_MAX)
    );
    // HTTP-date form is not delta-seconds → ignored, falls back to backoff.
    assert_eq!(
        retry_after_delay(&headers_with(Some("Wed, 21 Oct 2015 07:28:00 GMT"))),
        None
    );
    // Absent header → no override.
    assert_eq!(retry_after_delay(&headers_with(None)), None);
}

/// Spawn a server whose first connection sends a `200` with a `Content-Length`
/// larger than the bytes actually written, then closes — making the body read
/// fail mid-stream (a transient `is_body()` error). Later connections serve a
/// full `200 {body}`. Returns the `base_url` and a connection counter.
fn body_drop_then_ok_server(
    body: &'static str,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_thread = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let n = hits_thread.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let response = if n == 0 {
                // Promise 1024 bytes, send 4, then drop the connection.
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n{ \"".to_string()
            } else {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            };
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (format!("http://{addr}"), hits)
}

#[tokio::test]
async fn fetch_attestations_retries_on_body_read_failure() {
    // First attempt drops mid-body (transient is_body error); second succeeds.
    let (base_url, hits) = body_drop_then_ok_server(r#"{"attestations":[]}"#);
    let client = test_client(&base_url);
    let result = client
        .fetch_attestations(FetchParams {
            owner: "EarthBuild".to_string(),
            repo: Some("EarthBuild/earthbuild".to_string()),
            digest: "sha256:abc".to_string(),
            limit: 30,
            predicate_type: None,
        })
        .await;

    assert!(
        result.is_ok(),
        "expected recovery after body-read failure: {result:?}"
    );
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "should have taken 1 body-drop + 1 successful attempt"
    );
}
