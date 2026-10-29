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
async fn test_get_text_cached_failures_keep_whether_the_server_was_reached() {
    let _settings = set_test_http_retries(0);
    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();

    // A closed port fails before any response arrives
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let refused = client
        .get_text_cached(format!("http://127.0.0.1:{port}/refused"))
        .await
        .unwrap_err();
    assert!(is_unreachable(&refused), "{refused:?}");

    // A status from the server is not
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/missing")
        .with_status(404)
        .create_async()
        .await;
    let missing = client
        .get_text_cached(format!("{}/missing", server.url()))
        .await
        .unwrap_err();
    assert!(!is_unreachable(&missing), "{missing:?}");
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

/// A token in a download URL's query must not be handed to the host the
/// download is redirected to, in a `Referer` header or otherwise.
#[tokio::test]
async fn test_redirect_target_is_not_told_the_previous_url() {
    let mut origin = mockito::Server::new_async().await;
    let mut storage = mockito::Server::new_async().await;
    let redirect = origin
        .mock("GET", "/tool.tar.gz?private_token=secret")
        .with_status(302)
        .with_header("location", &format!("{}/tool.tar.gz", storage.url()))
        .expect(1)
        .create_async()
        .await;
    let artifact = storage
        .mock("GET", "/tool.tar.gz")
        .match_header("referer", mockito::Matcher::Missing)
        .with_body("OK")
        .expect(1)
        .create_async()
        .await;
    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();

    let body = client
        .get_text(format!("{}/tool.tar.gz?private_token=secret", origin.url()))
        .await
        .unwrap();

    assert_eq!(body, "OK");
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

#[tokio::test]
async fn test_get_html_accepts_pep_691_html_content_type() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/simple/pkg/")
        .with_status(200)
        .with_header(
            "content-type",
            "application/vnd.pypi.simple.v1+html; api-version=7.2-preview.1",
        )
        .with_body("<!DOCTYPE html><html></html>")
        .expect(1)
        .create_async()
        .await;

    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
    let html = client
        .get_html(format!("{}/simple/pkg/", server.url()))
        .await
        .unwrap();

    assert_eq!(html, "<!DOCTYPE html><html></html>");
    mock.assert();
}

#[test]
fn log_url_hides_userinfo() {
    let url = Url::parse("https://user:TOKEN@git.example.com/api/v1?page=2").unwrap();
    assert_eq!(
        log_url(&url),
        "https://[redacted]@git.example.com/api/v1?page=2"
    );
    let url = Url::parse("https://TOKEN@git.example.com/").unwrap();
    assert_eq!(log_url(&url), "https://[redacted]@git.example.com/");
    let url = Url::parse("https://git.example.com/a@b").unwrap();
    assert_eq!(log_url(&url), "https://git.example.com/a@b");
}

#[test]
fn log_url_hides_query_values() {
    let url =
        Url::parse("https://dl.example.com/a.tar.gz?private_token=s3cret&page=2&raw").unwrap();
    assert_eq!(
        log_url(&url),
        "https://dl.example.com/a.tar.gz?private_token=[redacted]&page=2&[redacted]"
    );
    // A short number is kept only for a paging parameter: a numeric token is
    // a credential like any other.
    let url =
        Url::parse("https://h.example/x?token=123456&access_token=42&per_page=100&limit=50&empty=")
            .unwrap();
    assert_eq!(
        log_url(&url),
        "https://h.example/x?token=[redacted]&access_token=[redacted]&per_page=100&limit=50&empty="
    );
}

/// A status error names the URL it was for. The query there can hold a
/// token, and the error reaches warnings, progress lines and the final report.
#[tokio::test]
async fn status_error_hides_url_query_token() {
    let mut server = mockito::Server::new_async().await;
    let _missing = server
        .mock("GET", "/a.tar.gz?private_token=s3cret")
        .with_status(404)
        .create_async()
        .await;
    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
    let err = client
        .get_text(format!("{}/a.tar.gz?private_token=s3cret", server.url()))
        .await
        .unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains("404"), "{message}");
    assert!(!message.contains("s3cret"), "{message}");
}

#[tokio::test(flavor = "current_thread")]
async fn request_timeout_error_hides_url_credentials() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // Hold every connection open without answering, so the request times out.
    tokio::spawn(async move {
        let mut held = vec![];
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let url = Url::parse(&format!("http://user:TOKEN@127.0.0.1:{port}/x")).unwrap();
    let client = Client::new(Duration::from_millis(100), ClientKind::Fetch).unwrap();
    let err = client
        .send_once_inner(
            Method::GET,
            url,
            &HeaderMap::new(),
            "GET",
            SendOnceOptions::new(None, true),
        )
        .await
        .unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains("HTTP timed out after"), "{message}");
    assert!(
        message.contains(&format!("http://[redacted]@127.0.0.1:{port}/x")),
        "{message}"
    );
    assert!(!message.contains("TOKEN"), "{message}");
}

#[tokio::test]
async fn test_buffered_bodies_are_bounded_after_decompression() {
    use std::io::{Read, Write};

    // About 1 KiB on the wire, one byte over the limit once inflated.
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let mut padding = std::io::repeat(b' ').take(MAX_BODY_BYTES as u64 - 1);
    std::io::copy(&mut padding, &mut gzip).unwrap();
    gzip.write_all(b"[]").unwrap();

    // Retries are on, and each getter is still expected once: the error is not
    // a transport failure, and the answer would be the same.
    let _settings = set_test_http_retries(1);
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/bomb")
        .with_status(200)
        .with_header("content-type", "text/html")
        .with_header("content-encoding", "gzip")
        .with_body(gzip.finish().unwrap())
        .expect(4)
        .create_async()
        .await;
    let url = format!("{}/bomb", server.url());

    let client = Client::new(Duration::from_secs(30), ClientKind::Http).unwrap();
    for err in [
        client.json::<Vec<String>, _>(&url).await.unwrap_err(),
        client.get_text(&url).await.unwrap_err(),
        client.get_html(&url).await.unwrap_err(),
        client.get_bytes(&url).await.map(|_| ()).unwrap_err(),
    ] {
        assert!(err.to_string().contains("is larger than 1.0 MiB"), "{err}");
        assert!(!is_transient(&err));
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn test_buffer_body_refuses_a_declared_length_over_the_limit() {
    let resp = Response::from(
        http::Response::builder()
            .header(reqwest::header::CONTENT_LENGTH, "9")
            .body(vec![b' '; 9])
            .unwrap(),
    );
    let err = buffer_body_with_limit(resp, 8).await.unwrap_err();
    assert!(err.to_string().contains("is larger than 8 B"), "{err}");
}

#[tokio::test]
async fn test_buffer_body_keeps_what_decoding_the_body_needs() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/latin1")
        .with_status(200)
        .with_header("content-type", "text/plain; charset=iso-8859-1")
        .with_body(b"caf\xe9")
        .create_async()
        .await;
    let url = format!("{}/latin1", server.url());

    let client = Client::new(Duration::from_secs(3), ClientKind::Http).unwrap();
    let unbuffered = client.get_async(&url).await.unwrap().text().await.unwrap();
    let resp = buffer_body(client.get_async(&url).await.unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.url().as_str(), url);
    assert_eq!(resp.text().await.unwrap(), unbuffered);
}

#[tokio::test(flavor = "current_thread")]
async fn test_body_timeout_bounds_trickling_text_response() {
    let port = spawn_trickling_server().await;
    let url = format!("http://127.0.0.1:{port}/SHA256SUMS");
    // The server sends a byte every 10ms, so the 100ms idle read timeout never
    // fires. The body deadline must still end the read, and must not be
    // retried.
    let client = Client::new(Duration::from_millis(100), ClientKind::Http).unwrap();
    let headers = HeaderMap::new();
    let err = tokio::time::timeout(
        Duration::from_secs(5),
        client.send_with_retries_body_timeout(
            RetryableRequest {
                method: Method::GET,
                url: Url::parse(&url).unwrap(),
                headers: &headers,
                verb_label: "GET",
                retries: 3,
                error_for_status: true,
            },
            |resp| async move { Ok(resp.text().await?) },
            Duration::from_millis(200),
        ),
    )
    .await
    .expect("the body deadline did not end the read")
    .unwrap_err();
    let message = err.to_string();

    assert!(
        message.contains("HTTP response body timed out after 200.0ms"),
        "{message}"
    );
    assert!(message.contains(&url), "{message}");
    assert!(message.contains("MISE_HTTP_DOWNLOAD_TIMEOUT"), "{message}");
    assert!(!is_transient(&err));
}

#[tokio::test(flavor = "current_thread")]
async fn test_get_html_body_is_bounded_by_http_download_timeout() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let _settings = {
        let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        let mut settings = mise_settings::SettingsPartial::empty();
        settings.http_download_timeout = Some("200ms".to_string());
        crate::testing::reset_settings(Some(settings));
        SettingsGuard { _lock: lock }
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };
        let mut request = [0u8; 4096];
        let _ = socket.read(&mut request).await;
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n")
            .await;
        while socket.write_all(b"x").await.is_ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let client = Client::new(Duration::from_millis(100), ClientKind::Http).unwrap();
    let err = tokio::time::timeout(
        Duration::from_secs(5),
        client.get_html(format!("http://127.0.0.1:{port}/simple/")),
    )
    .await
    .expect("the body deadline did not end the read")
    .unwrap_err();
    assert!(
        err.to_string().contains("HTTP response body timed out"),
        "{err}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_buffer_body_is_bounded_by_http_download_timeout() {
    let _settings = {
        let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        let mut settings = mise_settings::SettingsPartial::empty();
        settings.http_download_timeout = Some("200ms".to_string());
        crate::testing::reset_settings(Some(settings));
        SettingsGuard { _lock: lock }
    };
    // A caller that sends the request itself, as the OCI registry client does,
    // and reads the body through `buffer_body`. The server sends a byte every
    // 10ms, so the 100ms idle read timeout never fires.
    let port = spawn_trickling_server().await;
    let client = Client::new(Duration::from_millis(100), ClientKind::Http).unwrap();
    let resp = client
        .get_async(format!("http://127.0.0.1:{port}/manifest"))
        .await
        .unwrap();
    let err = tokio::time::timeout(Duration::from_secs(5), buffer_body(resp))
        .await
        .expect("the body deadline did not end the read")
        .unwrap_err();
    assert!(
        err.to_string().contains("HTTP response body timed out"),
        "{err}"
    );
}
