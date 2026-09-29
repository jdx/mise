use super::*;

#[test]
fn test_rewrite_url_defaults_to_original() {
    let lua = Lua::new();
    let url = "https://upstream.example/resource";
    assert_eq!(rewrite_url(&lua, url).unwrap(), url);
}

#[tokio::test]
async fn test_url_rewriter_applies_to_all_lua_http_methods() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/resource"))
        .respond_with(ResponseTemplate::new(200).set_body_string("rewritten"))
        .expect(4)
        .mount(&server)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/resource"))
        .respond_with(ResponseTemplate::new(200))
        .expect(2)
        .mount(&server)
        .await;

    let lua = Lua::new();
    mod_http(&lua).unwrap();
    lua.set_named_registry_value("github_token", "original-host")
        .unwrap();
    let replacement_origin = server.uri();
    let rewriter = lua
        .create_function(move |_, url: String| {
            Ok(url.replacen("https://api.github.com", &replacement_origin, 1))
        })
        .unwrap();
    lua.set_named_registry_value(crate::http::URL_REWRITER_REGISTRY_KEY, rewriter)
        .unwrap();
    let headers_resolver = lua
        .create_function(|lua, _: String| {
            lua.create_table_from([("Authorization", "Basic bWlycm9yOnNlY3JldA==")])
        })
        .unwrap();
    lua.set_named_registry_value(
        crate::http::HTTP_HEADERS_RESOLVER_REGISTRY_KEY,
        headers_resolver,
    )
    .unwrap();

    let temp_dir = tempfile::TempDir::new().unwrap();
    let download_path = temp_dir.path().join("download.txt");
    let try_download_path = temp_dir.path().join("try-download.txt");
    let download_path_str = download_path.to_string_lossy().to_string();
    let try_download_path_str = try_download_path.to_string_lossy().to_string();
    let url = "https://api.github.com/resource";

    lua.load(mlua::chunk! {
        local http = require("http")
        local request = {
            url = $url,
            headers = {
                ["Accept"] = "application/vnd.vfox+json",
                ["Authorization"] = "Bearer plugin-token",
                ["Cookie"] = "session=secret",
                ["Cookie2"] = "session2=secret",
                ["Proxy-Authorization"] = "Basic proxy-secret",
                ["WWW-Authenticate"] = "Bearer challenge-secret",
                ["X-Api-Key"] = "service-secret",
                ["X-ApiKey"] = "service-secret-without-delimiter",
                ["X-Gitlab-Token"] = "gitlab-secret",
                ["X-Session-Id"] = "session-secret",
                ["X-Vault-Token"] = "vault-secret",
                ["X-Auth-Method"] = "mirror-v1",
                ["X-Cache-Key"] = "cache-entry",
                ["X-Request-Key"] = "request-route",
                ["X-Vfox-Test"] = "required",
            },
        }

        local get_resp = http.get(request)
        assert(get_resp.status_code == 200)
        assert(get_resp.body == "rewritten")

        local try_get_resp, try_get_err = http.try_get(request)
        assert(try_get_err == nil)
        assert(try_get_resp.body == "rewritten")

        assert(http.head(request).status_code == 200)
        local try_head_resp, try_head_err = http.try_head(request)
        assert(try_head_err == nil)
        assert(try_head_resp.status_code == 200)

        assert(http.download_file(request, $download_path_str) == nil)
        local ok, try_download_err = http.try_download_file(
            request,
            $try_download_path_str
        )
        assert(ok == true)
        assert(try_download_err == nil)
    })
    .exec_async()
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(download_path).await.unwrap(),
        "rewritten"
    );
    assert_eq!(
        tokio::fs::read_to_string(try_download_path).await.unwrap(),
        "rewritten"
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 6);
    for request in requests {
        assert_eq!(
            request
                .headers
                .get("accept")
                .and_then(|value| value.to_str().ok()),
            Some("application/vnd.vfox+json")
        );
        assert_eq!(
            request
                .headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Basic bWlycm9yOnNlY3JldA==")
        );
        assert!(!request.headers.contains_key("cookie"));
        assert!(!request.headers.contains_key("cookie2"));
        assert!(!request.headers.contains_key("proxy-authorization"));
        assert!(!request.headers.contains_key("www-authenticate"));
        assert!(!request.headers.contains_key("x-api-key"));
        assert!(!request.headers.contains_key("x-apikey"));
        assert!(!request.headers.contains_key("x-gitlab-token"));
        assert!(!request.headers.contains_key("x-session-id"));
        assert!(!request.headers.contains_key("x-vault-token"));
        assert_eq!(
            request
                .headers
                .get("x-auth-method")
                .and_then(|value| value.to_str().ok()),
            Some("mirror-v1")
        );
        assert_eq!(
            request
                .headers
                .get("x-cache-key")
                .and_then(|value| value.to_str().ok()),
            Some("cache-entry")
        );
        assert_eq!(
            request
                .headers
                .get("x-request-key")
                .and_then(|value| value.to_str().ok()),
            Some("request-route")
        );
        assert_eq!(
            request
                .headers
                .get("x-vfox-test")
                .and_then(|value| value.to_str().ok()),
            Some("required")
        );
        assert!(!request.headers.contains_key("x-github-api-version"));
    }
}

#[test]
fn test_same_origin_rewrite_keeps_default_github_headers() {
    let lua = Lua::new();
    lua.set_named_registry_value("github_token", "same-origin")
        .unwrap();
    let mut input_headers = HeaderMap::new();
    input_headers.insert("x-api-key", HeaderValue::from_static("same-origin-secret"));

    let headers = add_default_headers_for_request(
        &lua,
        "https://api.github.com/repos/owner/repo",
        "https://api.github.com/mirror/owner/repo",
        input_headers,
    );

    assert_eq!(
        headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer same-origin")
    );
    assert_eq!(
        headers
            .get("x-github-api-version")
            .and_then(|value| value.to_str().ok()),
        Some("2022-11-28")
    );
    assert_eq!(
        headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok()),
        Some("same-origin-secret")
    );
}
