use super::*;

#[tokio::test(flavor = "current_thread")]
async fn test_download_retry_resumes_validated_partial() {
    let _guard = set_test_http_retries(1);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_response(),
        resumed_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let report = RecordingReport::default();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    client
        .download_file_with_headers(&url, &destination, &HeaderMap::new(), Some(&report))
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let requests = requests.lock().unwrap();
    let resumed = requests[1].to_ascii_lowercase();
    assert!(resumed.contains("range: bytes=5-"));
    assert!(resumed.contains("if-range: \"artifact-v1\""));
    assert!(resumed.contains("accept-encoding: identity"));
    assert!(report.positions.lock().unwrap().contains(&5));
    assert!(
        report
            .lengths
            .lock()
            .unwrap()
            .iter()
            .all(|length| *length == 10)
    );

    let partial = PartialDownload::new(
        &destination,
        download_request_hash(&url.parse().unwrap(), &HeaderMap::new()),
    )
    .unwrap();
    assert!(!partial.path.exists());
    assert!(!partial.state_path.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_does_not_reacquire_destination_lock() {
    let (port, count) = spawn_canned_server(vec![ok_response()]).await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let _destination_lock = crate::lock_file::LockFile::new(&destination)
        .lock()
        .unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    tokio::time::timeout(
        Duration::from_secs(2),
        client.download_file(&url, &destination, None),
    )
    .await
    .expect("download deadlocked on a lock already held by its caller")
    .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"OK");
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_retry_resumes_with_last_modified_validator() {
    let _guard = set_test_http_retries(1);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_last_modified_download_response(),
        resumed_last_modified_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    client
        .download_file(&url, &destination, None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let resumed = requests.lock().unwrap()[1].to_ascii_lowercase();
    assert!(resumed.contains("range: bytes=5-"));
    assert!(resumed.contains("if-range: wed, 21 oct 2015 07:28:00 gmt"));
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_resumes_across_calls_without_storing_secrets() {
    let _guard = set_test_http_retries(0);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_response(),
        resumed_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/private/artifact.tar.gz?token=url-secret");
    let parsed_url: Url = url.parse().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer header-secret"),
    );
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    std::fs::write(&destination, b"existing destination").unwrap();
    let partial =
        PartialDownload::new(&destination, download_request_hash(&parsed_url, &headers)).unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let _err = client
        .download_file_with_headers_metadata(&url, &destination, &headers, None)
        .await
        .unwrap_err();
    // The whole point of the partial: the bytes that arrived before the
    // body was cut short have to be on disk, or the `Range: bytes=5-` at
    // the bottom of this test is asking to resume from somewhere the file
    // does not reach. This was intermittently empty on macOS until the
    // download path started flushing on the failure branch too — do not
    // relax it to a length check.
    assert_eq!(std::fs::read(&partial.path).unwrap(), b"hello");
    let state = std::fs::read_to_string(&partial.state_path).unwrap();
    assert!(!state.contains("url-secret"));
    assert!(!state.contains("header-secret"));
    assert!(state.contains("artifact.tar.gz"));
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"existing destination"
    );

    let metadata = client
        .download_file_with_headers_metadata(&url, &destination, &headers, None)
        .await
        .unwrap();
    assert_eq!(
        metadata.effective_filename.as_deref(),
        Some("artifact.tar.gz")
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert!(
        requests.lock().unwrap()[1]
            .to_ascii_lowercase()
            .contains("range: bytes=5-")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_resume_preserves_stored_filename_after_redirect_changes() {
    let _guard = set_test_http_retries(0);
    let (port, count, requests) = spawn_recording_server(vec![
        redirect_to_tar_gz_response(),
        truncated_download_response(),
        redirect_to_zip_response(),
        resumed_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/download");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let _ = client
        .download_file_with_headers_metadata(&url, &destination, &HeaderMap::new(), None)
        .await
        .unwrap_err();

    let metadata = client
        .download_file_with_headers_metadata(&url, &destination, &HeaderMap::new(), None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(metadata.effective_filename.as_deref(), Some("tool.tar.gz"));
    assert_eq!(count.load(Ordering::SeqCst), 4);
    assert!(
        requests.lock().unwrap()[3]
            .to_ascii_lowercase()
            .contains("range: bytes=5-")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_resume_does_not_adopt_filename_when_initial_hint_missing() {
    let _guard = set_test_http_retries(0);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_response(),
        redirect_to_zip_response(),
        resumed_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let _ = client
        .download_file_with_headers_metadata(&url, &destination, &HeaderMap::new(), None)
        .await
        .unwrap_err();

    let metadata = client
        .download_file_with_headers_metadata(&url, &destination, &HeaderMap::new(), None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(metadata.effective_filename, None);
    assert_eq!(count.load(Ordering::SeqCst), 3);
    assert!(
        requests.lock().unwrap()[2]
            .to_ascii_lowercase()
            .contains("range: bytes=5-")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_without_validator_restarts_from_zero() {
    let _guard = set_test_http_retries(1);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_without_validator_response(),
        full_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    client
        .download_file(&url, &destination, None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert!(
        !requests.lock().unwrap()[1]
            .to_ascii_lowercase()
            .contains("range:")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_does_not_resume_automatically_decoded_response() {
    let _guard = set_test_http_retries(0);
    let (port, count) = spawn_canned_server(vec![truncated_encoded_download_response()]).await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let partial = PartialDownload::new(
        &destination,
        download_request_hash(&url.parse().unwrap(), &HeaderMap::new()),
    )
    .unwrap();
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let _err = client
        .download_file(&url, &destination, None)
        .await
        .unwrap_err();

    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(!partial.path.exists());
    assert!(!partial.state_path.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_restarts_when_server_ignores_range() {
    let _guard = set_test_http_retries(0);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_response(),
        full_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let _err = client
        .download_file(&url, &destination, None)
        .await
        .unwrap_err();
    client
        .download_file(&url, &destination, None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert!(
        requests.lock().unwrap()[1]
            .to_ascii_lowercase()
            .contains("range: bytes=5-")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_restarts_after_invalid_content_range() {
    let _guard = set_test_http_retries(0);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_response(),
        invalid_resumed_download_response(),
        full_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let _err = client
        .download_file(&url, &destination, None)
        .await
        .unwrap_err();
    client
        .download_file(&url, &destination, None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 3);
    let requests = requests.lock().unwrap();
    assert!(requests[1].to_ascii_lowercase().contains("range: bytes=5-"));
    assert!(!requests[2].to_ascii_lowercase().contains("range:"));
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_restarts_when_validator_changes() {
    let _guard = set_test_http_retries(0);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_response(),
        changed_validator_download_response(),
        full_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let _err = client
        .download_file(&url, &destination, None)
        .await
        .unwrap_err();
    client
        .download_file(&url, &destination, None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 3);
    let requests = requests.lock().unwrap();
    assert!(requests[1].to_ascii_lowercase().contains("range: bytes=5-"));
    assert!(!requests[2].to_ascii_lowercase().contains("range:"));
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_discards_partial_when_request_headers_change() {
    let _guard = set_test_http_retries(0);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_response(),
        full_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();
    let mut original_headers = HeaderMap::new();
    original_headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer old"));
    let mut changed_headers = HeaderMap::new();
    changed_headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer new"));

    let _err = client
        .download_file_with_headers(&url, &destination, &original_headers, None)
        .await
        .unwrap_err();
    client
        .download_file_with_headers(&url, &destination, &changed_headers, None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert!(
        !requests.lock().unwrap()[1]
            .to_ascii_lowercase()
            .contains("range:")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_recovers_from_unsatisfied_range() {
    let _guard = set_test_http_retries(0);
    let (port, count, requests) = spawn_recording_server(vec![
        truncated_download_response(),
        range_not_satisfiable_response(),
        full_download_response(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/artifact");
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artifact");
    let client = Client::new(Duration::from_secs(2), ClientKind::Http).unwrap();

    let _err = client
        .download_file(&url, &destination, None)
        .await
        .unwrap_err();
    client
        .download_file(&url, &destination, None)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"helloworld");
    assert_eq!(count.load(Ordering::SeqCst), 3);
    let requests = requests.lock().unwrap();
    assert!(requests[1].to_ascii_lowercase().contains("range: bytes=5-"));
    assert!(!requests[2].to_ascii_lowercase().contains("range:"));
}

#[tokio::test(flavor = "current_thread")]
async fn test_download_total_timeout_bounds_trickling_response() {
    let port = spawn_trickling_server().await;
    let url = format!("http://127.0.0.1:{port}/artifact.tar.gz");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("artifact.tar.gz");
    // The server sends a byte every 10ms, so the 100ms idle read timeout
    // never fires. The separate total budget must still end the download.
    let client = Client::new(Duration::from_millis(100), ClientKind::Http).unwrap();
    let started_at = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        client.download_file_with_headers_timeout(
            &url,
            &path,
            &HeaderMap::new(),
            None,
            Duration::from_millis(500),
        ),
    )
    .await
    .expect("download timeout regression test exceeded its independent deadline");
    let err = result.unwrap_err();
    let message = err.to_string();

    assert!(started_at.elapsed() < Duration::from_secs(5));
    assert!(message.contains("HTTP download timed out after 500.0ms"));
    assert!(message.contains(&url));
    assert!(message.contains("attempt 1"));
    assert!(message.contains("bytes received"));
    assert!(!message.contains("attempt 1, 0 bytes received"));
    assert!(message.contains("http_download_timeout"));
    assert!(message.contains("MISE_HTTP_DOWNLOAD_TIMEOUT"));
    assert!(!path.exists());
}
