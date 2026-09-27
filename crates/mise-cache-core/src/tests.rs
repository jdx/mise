use super::*;

#[test]
fn protocol_json_uses_jcs_key_and_number_encoding() {
    let value = serde_json::json!({"z": 1.0e30, "a": {"d": true, "c": null}});
    assert_eq!(
        canonical_json(&value).unwrap(),
        br#"{"a":{"c":null,"d":true},"z":1e+30}"#
    );
}

#[test]
fn dns_errors_are_not_transient() {
    #[derive(Debug)]
    struct DnsError;

    impl std::fmt::Display for DnsError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("dns error")
        }
    }

    impl std::error::Error for DnsError {}

    let error = eyre::Report::new(DnsError);
    assert!(is_dns_error(error.as_ref()));
    assert!(!is_transient(&error));
}

#[tokio::test]
async fn retries_reqwest_errors_wrapped_by_blob_stream_io() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    drop(listener);

    let request_error = reqwest::Client::new().get(url).send().await.unwrap_err();
    assert!(request_error.is_connect());
    let wrapped = eyre::Report::new(std::io::Error::other(request_error));
    assert!(is_transient(&wrapped));
}

#[test]
fn cache_digest_verifies_its_declared_algorithm() {
    let bytes = b"remote cache blob";
    let sha256 = CacheDigest {
        algorithm: "sha256".into(),
        hash: hex::encode(sha2::Sha256::digest(bytes)),
        size: bytes.len() as u64,
    };
    assert!(sha256.matches_bytes(bytes).unwrap());
    assert!(!sha256.matches_bytes(b"different").unwrap());

    let file = tempfile::NamedTempFile::new().unwrap();
    fs::write(file.path(), bytes).unwrap();
    assert!(sha256.matches_file(file.path()).unwrap());
    assert_eq!(
        CacheDigest::blake3_file(file.path()).unwrap().size,
        bytes.len() as u64
    );
    assert!(
        CacheDigest::blake3_file(file.path())
            .unwrap()
            .matches_bytes(bytes)
            .unwrap()
    );
}

#[test]
fn action_result_keys_require_blake3() {
    let client = RemoteCacheClient::new(RemoteCacheConfig {
        base_url: "http://127.0.0.1:1".parse().unwrap(),
        namespace: "test".into(),
        token: None,
        token_file: None,
        oidc_audience: None,
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        download_timeout: Duration::from_secs(1),
        retries: 0,
    })
    .unwrap();
    let action = CacheDigest {
        algorithm: "sha256".into(),
        hash: "0".repeat(64),
        size: 0,
    };

    assert!(
        client
            .action_result_endpoint(&action)
            .unwrap_err()
            .to_string()
            .contains("must use blake3")
    );
}

#[tokio::test]
async fn downloads_negotiated_blob_packs_and_omits_missing_objects() {
    let mut server = mockito::Server::new_async().await;
    let first_bytes = b"first packed blob";
    let second_bytes = b"second packed blob";
    let first = CacheDigest::blake3(first_bytes);
    let second = CacheDigest::blake3(second_bytes);
    let missing = CacheDigest::blake3(b"missing packed blob");
    let capabilities = server
        .mock("GET", "/v1/capabilities")
        .match_header(PROTOCOL_HEADER, "1")
        .match_header(AUTHORIZATION.as_str(), "Bearer test-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "protocol":{"major":1},
                "features":{"blob_packs":true},
                "limits":{"max_batch_items":100,"max_pack_bytes":1024}
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let packed = encode_blob_pack(&[
        (&first, first_bytes.as_slice()),
        (&second, second_bytes.as_slice()),
    ]);
    let packed_len = packed.len().to_string();
    let packed_blobs = 2.to_string();
    let packed_payload_bytes = (first.size + second.size).to_string();
    let request = server
        .mock("POST", "/v1/blobs:pack")
        .match_header(PROTOCOL_HEADER, "1")
        .match_header(NAMESPACE_HEADER, "test")
        .match_header("content-type", DIGEST_LIST_MEDIA_TYPE)
        .with_status(200)
        .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
        .with_header("content-length", &packed_len)
        .with_header(BLOB_PACK_BLOBS_HEADER, &packed_blobs)
        .with_header(BLOB_PACK_BYTES_HEADER, &packed_payload_bytes)
        .with_body(packed)
        .expect(1)
        .create_async()
        .await;
    let client = test_client(&server);
    let staging = tempfile::tempdir().unwrap();

    let pack = client
        .get_blob_pack(
            &[first.clone(), missing, second.clone(), first.clone()],
            staging.path(),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(pack.requests, 1);
    assert_eq!(pack.blob_count, 2);
    assert_eq!(pack.payload_bytes, first.size + second.size);
    assert_eq!(
        pack.framed_bytes,
        BLOB_PACK_MAGIC.len() as u64 + 2 * BLOB_PACK_HEADER_BYTES + first.size + second.size
    );
    assert_eq!(pack.blobs.len(), 2);
    assert_eq!(fs::read(&pack.blobs[0].1).unwrap(), first_bytes);
    assert_eq!(fs::read(&pack.blobs[1].1).unwrap(), second_bytes);
    capabilities.assert_async().await;
    request.assert_async().await;
}

#[tokio::test]
async fn rejects_mismatched_blob_pack_metadata() {
    let mut server = mockito::Server::new_async().await;
    let contents = b"packed blob";
    let digest = CacheDigest::blake3(contents);
    mock_blob_pack_capabilities(&mut server).await;
    server
        .mock("POST", "/v1/blobs:pack")
        .with_status(200)
        .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
        .with_header(BLOB_PACK_BLOBS_HEADER, "2")
        .with_body(encode_blob_pack(&[(&digest, contents.as_slice())]))
        .create_async()
        .await;
    let client = test_client(&server);
    let staging = tempfile::tempdir().unwrap();

    let error = client
        .get_blob_pack(&[digest], staging.path())
        .await
        .err()
        .unwrap();

    assert!(error.to_string().contains("blob count metadata mismatch"));
}

#[tokio::test]
async fn rejects_malformed_blob_pack_metadata() {
    let mut server = mockito::Server::new_async().await;
    let contents = b"packed blob";
    let digest = CacheDigest::blake3(contents);
    mock_blob_pack_capabilities(&mut server).await;
    server
        .mock("POST", "/v1/blobs:pack")
        .with_status(200)
        .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
        .with_header(BLOB_PACK_BYTES_HEADER, "not-a-number")
        .with_body(encode_blob_pack(&[(&digest, contents.as_slice())]))
        .create_async()
        .await;
    let client = test_client(&server);
    let staging = tempfile::tempdir().unwrap();

    let error = client
        .get_blob_pack(&[digest], staging.path())
        .await
        .err()
        .unwrap();

    assert!(error.to_string().contains("not an unsigned integer"));
}

#[tokio::test]
async fn rejects_unrequested_blob_pack_frames() {
    let mut server = mockito::Server::new_async().await;
    let requested = CacheDigest::blake3(b"requested");
    let injected_bytes = b"not requested";
    let injected = CacheDigest::blake3(injected_bytes);
    server
        .mock("GET", "/v1/capabilities")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "protocol":{"major":1},
                "features":{"blob_packs":true},
                "limits":{"max_batch_items":100,"max_pack_bytes":1024}
            })
            .to_string(),
        )
        .create_async()
        .await;
    server
        .mock("POST", "/v1/blobs:pack")
        .with_status(200)
        .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
        .with_body(encode_blob_pack(&[(&injected, injected_bytes.as_slice())]))
        .create_async()
        .await;
    let client = test_client(&server);
    let staging = tempfile::tempdir().unwrap();

    let error = client
        .get_blob_pack(&[requested], staging.path())
        .await
        .err()
        .unwrap();

    assert!(error.to_string().contains("unrequested digest"));
}

#[tokio::test]
async fn falls_back_when_blob_packs_are_not_advertised() {
    let mut server = mockito::Server::new_async().await;
    let capabilities = server
        .mock("GET", "/v1/capabilities")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let client = test_client(&server);
    let staging = tempfile::tempdir().unwrap();

    assert!(
        client
            .get_blob_pack(&[CacheDigest::blake3(b"blob")], staging.path())
            .await
            .unwrap()
            .is_none()
    );
    capabilities.assert_async().await;
}

#[tokio::test]
async fn disables_blob_packs_when_the_advertised_endpoint_is_unavailable() {
    let mut server = mockito::Server::new_async().await;
    let capabilities = server
        .mock("GET", "/v1/capabilities")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "protocol":{"major":1},
                "features":{"blob_packs":true},
                "limits":{"max_batch_items":100,"max_pack_bytes":1024}
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let request = server
        .mock("POST", "/v1/blobs:pack")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let client = test_client(&server);
    let staging = tempfile::tempdir().unwrap();
    let digest = CacheDigest::blake3(b"blob");

    assert!(
        client
            .get_blob_pack(std::slice::from_ref(&digest), staging.path())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        client
            .get_blob_pack(&[digest], staging.path())
            .await
            .unwrap()
            .is_none()
    );
    capabilities.assert_async().await;
    request.assert_async().await;
}

#[tokio::test]
async fn rejects_truncated_blob_pack_frames() {
    let mut server = mockito::Server::new_async().await;
    let contents = b"complete blob";
    let digest = CacheDigest::blake3(contents);
    let mut pack = encode_blob_pack(&[(&digest, contents.as_slice())]);
    pack.truncate(pack.len() - 3);
    mock_blob_pack_capabilities(&mut server).await;
    server
        .mock("POST", "/v1/blobs:pack")
        .with_status(200)
        .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
        .with_body(pack)
        .create_async()
        .await;
    let client = test_client(&server);
    let staging = tempfile::tempdir().unwrap();

    let error = match client.get_blob_pack(&[digest], staging.path()).await {
        Err(error) => error,
        Ok(_) => panic!("truncated pack should be rejected"),
    };

    assert!(
        error
            .to_string()
            .contains("ended before a blob was complete")
    );
}

#[tokio::test]
async fn rejects_blob_pack_frames_with_corrupt_content() {
    let mut server = mockito::Server::new_async().await;
    let digest = CacheDigest::blake3(b"expected");
    let corrupt = b"corrupt!";
    let pack = encode_blob_pack(&[(&digest, corrupt.as_slice())]);
    mock_blob_pack_capabilities(&mut server).await;
    server
        .mock("POST", "/v1/blobs:pack")
        .with_status(200)
        .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
        .with_body(pack)
        .create_async()
        .await;
    let client = test_client(&server);
    let staging = tempfile::tempdir().unwrap();

    let error = match client.get_blob_pack(&[digest], staging.path()).await {
        Err(error) => error,
        Ok(_) => panic!("corrupt pack should be rejected"),
    };

    assert!(error.to_string().contains("failed digest verification"));
}

#[test]
fn blob_pack_chunk_honors_item_and_byte_limits() {
    let first = CacheDigest::blake3(b"1234");
    let second = CacheDigest::blake3(b"5678");
    let oversized = CacheDigest::blake3(b"123456789");
    let chunk = blob_pack_chunk(
        &[first.clone(), second.clone(), first.clone(), oversized],
        BlobPackLimits {
            max_items: 10,
            max_bytes: 7,
        },
    )
    .unwrap();

    assert_eq!(chunk, vec![first]);

    let chunk = blob_pack_chunk(
        &[CacheDigest::blake3(b"a"), CacheDigest::blake3(b"b")],
        BlobPackLimits {
            max_items: 1,
            max_bytes: 100,
        },
    )
    .unwrap();
    assert_eq!(chunk.len(), 1);
}

#[test]
fn blob_pack_timeout_scales_with_declared_work() {
    let base = Duration::from_secs(10);
    let small = CacheDigest::blake3(b"small");
    assert_eq!(blob_pack_download_timeout(base, &[small]), base);

    let large = CacheDigest {
        algorithm: "blake3".into(),
        hash: "0".repeat(64),
        size: MAX_STAGED_BLOB_PACK_BYTES,
    };
    assert_eq!(
        blob_pack_download_timeout(base, &[large]),
        base.saturating_mul(4)
    );

    let many = (0..=BLOB_PACK_TIMEOUT_ITEMS_PER_UNIT)
        .map(|index| CacheDigest::blake3(index.to_string().as_bytes()))
        .collect::<Vec<_>>();
    assert_eq!(
        blob_pack_download_timeout(base, &many),
        base.saturating_mul(2)
    );
}

#[test]
fn bearer_authorization_headers_are_sensitive() {
    let header = authorization_header(Some(" test-token ")).unwrap().unwrap();
    assert_eq!(header, "Bearer test-token");
    assert!(header.is_sensitive());
    assert!(authorization_header(Some(" ")).unwrap().is_none());
}

fn test_client(server: &mockito::ServerGuard) -> RemoteCacheClient {
    RemoteCacheClient::new(RemoteCacheConfig {
        base_url: server.url().parse().unwrap(),
        namespace: "test".into(),
        token: Some("test-token".into()),
        token_file: None,
        oidc_audience: None,
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        download_timeout: Duration::from_secs(1),
        retries: 0,
    })
    .unwrap()
}

async fn mock_blob_pack_capabilities(server: &mut mockito::ServerGuard) {
    server
        .mock("GET", "/v1/capabilities")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "protocol":{"major":1},
                "features":{"blob_packs":true},
                "limits":{"max_batch_items":100,"max_pack_bytes":1024}
            })
            .to_string(),
        )
        .create_async()
        .await;
}

fn encode_blob_pack(entries: &[(&CacheDigest, &[u8])]) -> Vec<u8> {
    let mut pack = BLOB_PACK_MAGIC.to_vec();
    for (digest, contents) in entries {
        assert_eq!(digest.size, contents.len() as u64);
        pack.push(match digest.algorithm.as_str() {
            "blake3" => 1,
            "sha256" => 2,
            algorithm => panic!("unexpected test digest algorithm {algorithm}"),
        });
        pack.extend(hex::decode(&digest.hash).unwrap());
        pack.extend(digest.size.to_be_bytes());
        pack.extend_from_slice(contents);
    }
    pack
}

#[tokio::test]
async fn token_file_credentials_are_reloaded() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache-token");
    fs::write(&path, "first-token\n").unwrap();
    let credential = RemoteCacheCredential::File(path.clone());

    let first = credential.authorization().await.unwrap().unwrap();
    assert_eq!(first, "Bearer first-token");
    assert!(first.is_sensitive());

    fs::write(path, "rotated-token\n").unwrap();
    let rotated = credential.authorization().await.unwrap().unwrap();
    assert_eq!(rotated, "Bearer rotated-token");
}

#[tokio::test]
async fn github_actions_oidc_tokens_are_acquired_and_cached() {
    let mut server = mockito::Server::new_async().await;
    let expires_at = unix_timestamp().unwrap() + 3600;
    let token = test_jwt(expires_at);
    let token_response = serde_json::json!({"value":token}).to_string();
    let request = server
        .mock("GET", "/oidc")
        .match_query(mockito::Matcher::UrlEncoded(
            "audience".into(),
            "https://cache.example.com".into(),
        ))
        .match_header("authorization", "Bearer request-secret")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(token_response)
        .expect(1)
        .create_async()
        .await;
    let credential = GithubActionsOidcCredential::new(
        "https://cache.example.com",
        format!("{}/oidc?api-version=1&audience=old", server.url())
            .parse()
            .unwrap(),
        "request-secret",
        reqwest::Client::new(),
        0,
    )
    .unwrap();
    assert_eq!(
        credential.request_url.query_pairs().collect::<Vec<_>>(),
        vec![
            ("api-version".into(), "1".into()),
            ("audience".into(), "https://cache.example.com".into()),
        ]
    );

    let first = credential.authorization().await.unwrap();
    let second = credential.authorization().await.unwrap();

    assert_eq!(first, format!("Bearer {token}"));
    assert_eq!(first, second);
    assert!(first.is_sensitive());
    request.assert_async().await;
}

#[test]
fn oidc_request_urls_require_https_except_for_loopback() {
    validate_oidc_request_url(&"https://example.com/oidc".parse().unwrap()).unwrap();
    validate_oidc_request_url(&"http://127.0.0.1:3000/oidc".parse().unwrap()).unwrap();
    assert!(validate_oidc_request_url(&"http://example.com/oidc".parse().unwrap()).is_err());
}

fn test_jwt(expires_at: u64) -> String {
    let header = URL_SAFE_NO_PAD.encode(b"{}");
    let claims =
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&serde_json::json!({"exp":expires_at})).unwrap());
    format!("{header}.{claims}.signature")
}

#[test]
fn remote_urls_require_https_for_authenticated_requests() {
    for url in [
        "http://localhost:3000",
        "http://127.0.0.1:3000",
        "http://[::1]:3000",
        "https://cache.example.com",
    ] {
        validate_remote_url(&url.parse().unwrap(), true).unwrap();
    }
    let insecure: Url = "http://cache.example.com".parse().unwrap();
    assert!(validate_remote_url(&insecure, true).is_err());
    validate_remote_url(&insecure, false).unwrap();
    assert!(validate_remote_url(&"ftp://localhost/cache".parse().unwrap(), false).is_err());
}
