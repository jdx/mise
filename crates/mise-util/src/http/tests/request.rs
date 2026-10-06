use super::*;

#[test]
fn relay_and_direct_responses_share_status_contract() {
    let response =
        |status| Response::from(http::Response::builder().status(status).body("").unwrap());
    let options = SendOnceOptions::new(None, true);
    assert!(options.check_response(response(403)).is_err());
    assert!(options.check_response(response(500)).is_err());
    assert!(options.check_response(response(416)).is_err());
    assert!(
        options
            .clone()
            .allow_range_not_satisfiable()
            .check_response(response(416))
            .is_ok()
    );
    assert!(
        options
            .clone()
            .allow_range_not_satisfiable()
            .check_response(response(403))
            .is_err()
    );
    assert!(
        options
            .allow_error_status()
            .check_response(response(403))
            .is_ok()
    );
}

#[test]
fn download_state_placeholder_is_the_length_tempfile_will_produce() {
    // What the hint measures has to be what Windows will see. Compared against a name
    // `tempfile` actually produces rather than against the placeholder's own definition:
    // asserting `PREFIX.len() + 6` would be true whatever `tempfile` does, and the number
    // that matters is its suffix width, which is its choice and not ours.
    let placeholder = format!("{DOWNLOAD_STATE_PREFIX}XXXXXX");
    let dir = tempfile::tempdir().unwrap();
    let real = tempfile::NamedTempFile::with_prefix_in(DOWNLOAD_STATE_PREFIX, dir.path())
        .unwrap()
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .chars()
        .count();
    assert_eq!(
        real,
        placeholder.chars().count(),
        "tempfile's generated name is {real} characters; the stand-in the hint measures is \
         {}. A stand-in shorter than the real name lets a path over the limit measure under \
         the hint's threshold, which is the case this whole change exists for.",
        placeholder.chars().count()
    );

    // And it has to be worth measuring separately from the directory: the reason the parent
    // is not used is that this is meaningfully longer than it.
    let parent = PathBuf::from(r"C:\some\dir");
    let temp = parent.join(&placeholder);
    assert!(
        temp.as_os_str().len() > parent.as_os_str().len() + 16,
        "the stand-in adds {} units, which the hint's own margin would absorb",
        temp.as_os_str().len() - parent.as_os_str().len()
    );
}

#[test]
fn test_resolve_pagination_url() {
    let base = "https://api.github.com/repos/jdx/aube/releases?per_page=100";
    assert_eq!(
        resolve_pagination_url(base, "/repos/jdx/aube/releases?page=2").unwrap(),
        "https://api.github.com/repos/jdx/aube/releases?page=2"
    );
    assert_eq!(
        resolve_pagination_url(
            base,
            "https://api.github.com/repos/jdx/aube/releases?page=2"
        )
        .unwrap(),
        "https://api.github.com/repos/jdx/aube/releases?page=2"
    );
}

#[tokio::test]
async fn test_invalid_url_returns_error_not_panic() {
    // A relative/invalid URL must return an error rather than panicking
    // (previously `into_url().unwrap()` crashed the process). See #3547.
    let client = Client::new(Duration::from_secs(1), ClientKind::Http).unwrap();
    assert!(client.get_bytes("").await.is_err());
    assert!(client.head("").await.is_err());
    assert!(client.get_text("").await.is_err());
    assert!(client.get_text_request("").send().await.is_err());
}

#[tokio::test]
async fn streaming_upload_allows_a_body_longer_than_the_read_timeout() {
    use axum::{Router, body::Bytes, routing::post};

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/upload",
                post(|body: Bytes| async move {
                    assert_eq!(body.as_ref(), b"slow upload body");
                    StatusCode::ACCEPTED
                }),
            ),
        )
        .await
        .unwrap();
    });

    let body = futures_util::stream::unfold(0, |chunk| async move {
        let bytes = match chunk {
            0 => b"slow ".as_slice(),
            1 => b"upload ".as_slice(),
            2 => b"body".as_slice(),
            _ => return None,
        };
        tokio::time::sleep(Duration::from_millis(80)).await;
        Some((
            Ok::<_, std::io::Error>(Bytes::copy_from_slice(bytes)),
            chunk + 1,
        ))
    });
    let client =
        Client::new_shared_without_read_timeout(Duration::from_millis(40), ClientKind::Http);
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        client
            .reqwest()
            .unwrap()
            .post(format!("http://{address}/upload"))
            .header("Content-Length", "16")
            .body(reqwest::Body::wrap_stream(body))
            .send(),
    )
    .await
    .expect("streaming upload should not be bounded by the read timeout")
    .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn test_request_rejects_custom_credentials_on_https_to_http_replacement() {
    // Same-host downgrade: host scoping keeps the credential header, so sending it
    // would expose it in cleartext.
    let server = mockito::Server::new_async().await;
    let replacement = server.url();
    let original = replacement.replacen("http://", "https://", 1);
    let _guard = {
        let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        let mut settings = mise_settings::SettingsPartial::empty();
        settings.url_replacements = Some(indexmap::indexmap! {
            original.clone() => replacement,
        });
        crate::testing::reset_settings(Some(settings));
        SettingsGuard { _lock: lock }
    };
    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", HeaderValue::from_static("secret"));
    let client = Client::new(Duration::from_secs(1), ClientKind::Http).unwrap();

    let err = client
        .get_text_request(format!("{original}/file"))
        .headers(&headers)
        .send()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("refusing to send credentials"));
}

#[tokio::test(flavor = "current_thread")]
async fn test_request_scopes_credentials_before_refusing_a_downgrade() {
    // Host-changing downgrade: the credential header belongs to the original host and
    // is removed, so the request proceeds without it rather than failing. Refusing
    // here would break an http mirror fronting an https origin (#7164).
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/file")
        .match_header("x-api-key", mockito::Matcher::Missing)
        .with_body("ok")
        .create_async()
        .await;
    let replacement = server.url();
    let _guard = {
        let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        let mut settings = mise_settings::SettingsPartial::empty();
        settings.url_replacements = Some(indexmap::indexmap! {
            "https://secure.example.com".to_string() => replacement,
        });
        crate::testing::reset_settings(Some(settings));
        SettingsGuard { _lock: lock }
    };
    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", HeaderValue::from_static("secret"));
    let client = Client::new(Duration::from_secs(1), ClientKind::Http).unwrap();

    let body = client
        .get_text_request("https://secure.example.com/file")
        .headers(&headers)
        .send()
        .await
        .unwrap();

    assert_eq!(body, "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn test_client_initialization_error_is_returned_not_panicked() {
    let client = Client::with_init_error("builder error: OpenSSL error");

    let err = client.get_text("https://example.com").await.unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains("Could not initialize the HTTP client"));
    assert!(message.contains("builder error: OpenSSL error"));
}

#[tokio::test]
async fn test_get_html_accepts_text_html_without_doctype() {
    let mut server = mockito::Server::new_async().await;
    let expected_body = "<html><body>package index</body></html>";
    let mock = server
        .mock("GET", "/simple")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_body(expected_body)
        .expect(1)
        .create_async()
        .await;

    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
    let html = client
        .get_html(format!("{}/simple", server.url()))
        .await
        .unwrap();

    assert_eq!(html, expected_body);
    mock.assert();
}

#[tokio::test]
async fn test_download_metadata_uses_redirected_filename() {
    let mut server = mockito::Server::new_async().await;
    let location = format!("{}/releases/tool.tar.gz", server.url());
    let redirect = server
        .mock("GET", "/download")
        .with_status(302)
        .with_header("location", &location)
        .expect(1)
        .create_async()
        .await;
    let artifact = server
        .mock("GET", "/releases/tool.tar.gz")
        .with_status(200)
        .with_header("content-length", "2")
        .with_body("OK")
        .expect(1)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("download");
    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();

    let metadata = client
        .download_file_with_metadata(format!("{}/download", server.url()), &destination, None)
        .await
        .unwrap();

    assert_eq!(metadata.effective_filename.as_deref(), Some("tool.tar.gz"));
    assert_eq!(std::fs::read(&destination).unwrap(), b"OK");
    redirect.assert();
    artifact.assert();
}

#[test]
fn test_download_filename_hint_excludes_unsafe_or_private_url_parts() {
    let url: Url =
        "https://user:pass@example.com/releases/tool%20name.tar.gz?token=secret#fragment"
            .parse()
            .unwrap();
    assert_eq!(
        download_filename_hint(&url).as_deref(),
        Some("tool name.tar.gz")
    );

    let unsafe_url: Url = "https://example.com/releases/%2E%2E%2Fsecret.tar.gz"
        .parse()
        .unwrap();
    assert_eq!(download_filename_hint(&unsafe_url), None);

    for encoded_name in [
        "tool%3Aname.tar.gz",
        "tool%2Aname.tar.gz",
        "tool%00name.tar.gz",
    ] {
        let url: Url = format!("https://example.com/releases/{encoded_name}")
            .parse()
            .unwrap();
        assert_eq!(download_filename_hint(&url), None, "{encoded_name}");
    }
}

#[tokio::test]
async fn test_get_html_rejects_non_html_content_type() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/plain")
        .with_status(200)
        .with_header("content-type", "text/plain")
        .with_body("<!DOCTYPE html><html></html>")
        .expect(1)
        .create_async()
        .await;

    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
    let err = client
        .get_html(format!("{}/plain", server.url()))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("Got non-HTML text from"));
    mock.assert();
}
