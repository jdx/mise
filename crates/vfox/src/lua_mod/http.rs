use mlua::{BorrowedStr, ExternalResult, Lua, MultiValue, Result, Table, Value};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use reqwest::{RequestBuilder, Response};
use url::Url;

use crate::http::{
    CLIENT, http_retry_attempts, is_transient, retry_async, retry_delay, should_retry_status,
};

async fn send_with_retry(builder: RequestBuilder) -> std::result::Result<Response, reqwest::Error> {
    let url = builder
        .try_clone()
        .and_then(|b| b.build().ok())
        .map(|r| r.url().to_string())
        .unwrap_or_default();
    let Some(template) = builder.try_clone() else {
        return builder.send().await;
    };

    let attempts = http_retry_attempts().max(1);
    for attempt in 0..attempts {
        let response = template
            .try_clone()
            .expect("cloned request builder should remain cloneable")
            .send()
            .await;

        let transient_err: Option<String> = match response {
            Ok(resp) if should_retry_status(resp.status()) && attempt + 1 < attempts => {
                Some(format!("HTTP {}", resp.status()))
            }
            Ok(resp) => return Ok(resp),
            Err(err) if is_transient(&err) && attempt + 1 < attempts => Some(err.to_string()),
            Err(err) => return Err(err),
        };

        if let Some(msg) = transient_err {
            let delay = retry_delay(attempt);
            log::warn!(
                "HTTP {} attempt {} failed (transient): {}; retrying in {:?}",
                url,
                attempt + 1,
                msg,
                delay
            );
            tokio::time::sleep(delay).await;
        }
    }

    unreachable!("retry loop should always return a response or error")
}

pub fn mod_http(lua: &Lua) -> Result<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set(
        "http",
        lua.create_table_from(vec![
            (
                "get",
                lua.create_async_function(|lua: mlua::Lua, input| async move {
                    get(&lua, input).await
                })?,
            ),
            (
                "try_get",
                lua.create_async_function(|lua: mlua::Lua, input| async move {
                    try_get(&lua, input).await
                })?,
            ),
            (
                "head",
                lua.create_async_function(|lua: mlua::Lua, input| async move {
                    head(&lua, input).await
                })?,
            ),
            (
                "try_head",
                lua.create_async_function(|lua: mlua::Lua, input| async move {
                    try_head(&lua, input).await
                })?,
            ),
            (
                "download_file",
                lua.create_async_function(|lua: mlua::Lua, input| async move {
                    download_file(&lua, input).await
                })?,
            ),
            (
                "try_download_file",
                lua.create_async_function(|lua: mlua::Lua, input| async move {
                    try_download_file(&lua, input).await
                })?,
            ),
        ])?,
    )
}

fn into_headers(table: &Table) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for entry in table.pairs::<BorrowedStr, BorrowedStr>() {
        let (k, v) = entry?;
        map.insert(
            HeaderName::from_bytes(k.as_bytes()).into_lua_err()?,
            HeaderValue::from_str(&v).into_lua_err()?,
        );
    }
    Ok(map)
}

fn github_token(lua: &Lua) -> Option<String> {
    if let Ok(resolver) = lua.named_registry_value::<mlua::Function>("github_token_fn")
        && let Ok(token) = resolver.call::<String>(())
    {
        let token = token.trim();
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }

    if let Ok(token) = lua.named_registry_value::<String>("github_token") {
        let token = token.trim();
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }

    ["MISE_GITHUB_TOKEN", "GITHUB_API_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .find_map(|key| {
            std::env::var(key)
                .ok()
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty())
        })
}

fn add_default_headers(lua: &Lua, url: &str, mut headers: HeaderMap) -> HeaderMap {
    if headers.contains_key(AUTHORIZATION) {
        return headers;
    }

    let Ok(url) = Url::parse(url) else {
        return headers;
    };

    let Some(host) = url.host_str() else {
        return headers;
    };

    // Only attach auth to GitHub REST API URLs. Sending auth to github.com
    // release-download URLs causes GitHub to 302 to objects.githubusercontent.com
    // (instead of the public release-assets host), which then 401s once
    // reqwest strips the Authorization header on the cross-origin redirect.
    // Mirrors src/github.rs::is_github_api_url.
    let is_api =
        host == "api.github.com" || (host.starts_with("api.") && host.ends_with(".ghe.com"));

    if is_api && let Some(token) = github_token(lua) {
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(AUTHORIZATION, value);
        }
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );
    }

    headers
}

async fn get(lua: &Lua, input: Table) -> Result<Table> {
    let url: String = input.get("url").into_lua_err()?;
    let headers = match input.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let headers = add_default_headers(lua, &url, headers);
    let resp = send_with_retry(CLIENT.get(&url).headers(headers))
        .await
        .into_lua_err()?;
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    t.set("body", resp.text().await.into_lua_err()?)?;
    Ok(t)
}

async fn download_file(lua: &Lua, input: MultiValue) -> Result<()> {
    let t: &Table = input.iter().next().unwrap().as_table().unwrap();
    let url: String = t.get("url").into_lua_err()?;
    let headers = match t.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let headers = add_default_headers(lua, &url, headers);
    let path: String = input.iter().nth(1).unwrap().to_string()?;
    // Retry the whole flow (request + body) so a mid-stream drop restarts the
    // download instead of failing.
    let bytes = retry_async(&url, || async {
        let resp = CLIENT.get(&url).headers(headers.clone()).send().await?;
        let resp = resp.error_for_status()?;
        resp.bytes().await
    })
    .await
    .into_lua_err()?;
    let mut file = tokio::fs::File::create(&path).await.into_lua_err()?;
    tokio::io::AsyncWriteExt::write_all(&mut file, &bytes)
        .await
        .into_lua_err()?;
    Ok(())
}

async fn head(lua: &Lua, input: Table) -> Result<Table> {
    let url: String = input.get("url").into_lua_err()?;
    let headers = match input.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let headers = add_default_headers(lua, &url, headers);
    let resp = send_with_retry(CLIENT.head(&url).headers(headers))
        .await
        .into_lua_err()?;
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    Ok(t)
}

async fn try_get(lua: &Lua, input: Table) -> Result<MultiValue> {
    let url: String = input.get("url").into_lua_err()?;
    let headers = match input.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let headers = add_default_headers(lua, &url, headers);
    let resp = match send_with_retry(CLIENT.get(&url).headers(headers)).await {
        Ok(resp) => resp,
        Err(e) => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(lua.create_string(e.to_string())?),
            ]));
        }
    };
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    match resp.text().await {
        Ok(body) => t.set("body", body)?,
        Err(e) => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(lua.create_string(e.to_string())?),
            ]));
        }
    }
    Ok(MultiValue::from_vec(vec![Value::Table(t), Value::Nil]))
}

async fn try_head(lua: &Lua, input: Table) -> Result<MultiValue> {
    let url: String = input.get("url").into_lua_err()?;
    let headers = match input.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let headers = add_default_headers(lua, &url, headers);
    let resp = match send_with_retry(CLIENT.head(&url).headers(headers)).await {
        Ok(resp) => resp,
        Err(e) => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(lua.create_string(e.to_string())?),
            ]));
        }
    };
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    Ok(MultiValue::from_vec(vec![Value::Table(t), Value::Nil]))
}

async fn try_download_file(lua: &Lua, input: MultiValue) -> Result<MultiValue> {
    let t = match input.front().and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(lua.create_string("first argument must be a table")?),
            ]));
        }
    };
    let url: String = t.get("url").into_lua_err()?;
    let headers = match t.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let headers = add_default_headers(lua, &url, headers);
    let path = match input.get(1).and_then(|v| v.to_string().ok()) {
        Some(p) => p,
        None => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(lua.create_string("second argument must be a string path")?),
            ]));
        }
    };
    let bytes = match retry_async(&url, || async {
        let resp = CLIENT.get(&url).headers(headers.clone()).send().await?;
        let resp = resp.error_for_status()?;
        resp.bytes().await
    })
    .await
    {
        Ok(bytes) => bytes,
        Err(e) => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(lua.create_string(e.to_string())?),
            ]));
        }
    };
    let mut file = match tokio::fs::File::create(&path).await {
        Ok(f) => f,
        Err(e) => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(lua.create_string(e.to_string())?),
            ]));
        }
    };
    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await {
        return Ok(MultiValue::from_vec(vec![
            Value::Nil,
            Value::String(lua.create_string(e.to_string())?),
        ]));
    }
    Ok(MultiValue::from_vec(vec![Value::Boolean(true), Value::Nil]))
}

fn get_headers(lua: &Lua, headers: &reqwest::header::HeaderMap) -> Result<Table> {
    let t = lua.create_table()?;
    for (name, value) in headers.iter() {
        t.set(name.as_str(), value.to_str().into_lua_err()?)?;
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_get() {
        // Start a local mock server
        let server = MockServer::start().await;

        // Create a mock endpoint
        Mock::given(method("GET"))
            .and(path("/get"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "message": "test response"
                    }))
                    .insert_header("content-type", "application/json"),
            )
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let url = server.uri() + "/get";
        lua.load(mlua::chunk! {
            local http = require("http")
            local resp = http.get({ url = $url })
            assert(resp.status_code == 200)
            assert(type(resp.body) == "string")
        })
        .exec_async()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_get_headers() {
        // Start a local mock server
        let server = MockServer::start().await;

        // Create a mock endpoint
        Mock::given(method("GET"))
            .and(path("/get"))
            .and(header("Authorization", "Bearer abc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "message": "test response"
                    }))
                    .insert_header("content-type", "application/json"),
            )
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let url = server.uri() + "/get";
        lua.load(mlua::chunk! {
            local http = require("http")
            local resp = http.get({
                url = $url,
                headers = {
                    ["Authorization"] = "Bearer abc"
                }
            })
            assert(resp.status_code == 200)
            assert(type(resp.body) == "string")
        })
        .exec_async()
        .await
        .unwrap();
    }

    #[test]
    fn test_add_default_headers_uses_lazy_resolver() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let calls = Arc::new(AtomicUsize::new(0));
        let lua = Lua::new();

        let calls_inner = calls.clone();
        let resolver = lua
            .create_function(move |_, ()| {
                calls_inner.fetch_add(1, Ordering::SeqCst);
                Ok("ghp_lazy".to_string())
            })
            .unwrap();
        lua.set_named_registry_value("github_token_fn", resolver)
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let headers = add_default_headers(
            &lua,
            "https://api.github.com/repos/neovim/neovim/releases",
            HeaderMap::default(),
        );

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer ghp_lazy")
        );

        // Non-GitHub-API URLs must not invoke the resolver.
        let _ = add_default_headers(&lua, "https://example.com/some/path", HeaderMap::default());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_add_default_headers_lazy_resolver_takes_precedence_over_string() {
        let lua = Lua::new();
        lua.set_named_registry_value("github_token", "ghp_string")
            .unwrap();
        let resolver = lua
            .create_function(|_, ()| Ok("ghp_lazy".to_string()))
            .unwrap();
        lua.set_named_registry_value("github_token_fn", resolver)
            .unwrap();

        let headers = add_default_headers(
            &lua,
            "https://api.github.com/repos/owner/repo",
            HeaderMap::default(),
        );

        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer ghp_lazy")
        );
    }

    #[test]
    fn test_add_default_headers_falls_back_to_string_when_resolver_empty() {
        let lua = Lua::new();
        lua.set_named_registry_value("github_token", "ghp_string")
            .unwrap();
        let resolver = lua.create_function(|_, ()| Ok(String::new())).unwrap();
        lua.set_named_registry_value("github_token_fn", resolver)
            .unwrap();

        let headers = add_default_headers(
            &lua,
            "https://api.github.com/repos/owner/repo",
            HeaderMap::default(),
        );

        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer ghp_string")
        );
    }

    #[test]
    fn test_add_default_headers_uses_registry_token() {
        let lua = Lua::new();
        lua.set_named_registry_value("github_token", " ghp_registry\n")
            .unwrap();

        let headers = add_default_headers(
            &lua,
            "https://api.github.com/repos/neovim/neovim/releases",
            HeaderMap::default(),
        );

        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer ghp_registry")
        );
        assert_eq!(
            headers
                .get("x-github-api-version")
                .and_then(|value| value.to_str().ok()),
            Some("2022-11-28")
        );
    }

    #[test]
    fn test_add_default_headers_keeps_explicit_authorization() {
        let mut headers = HeaderMap::default();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer explicit"));

        let lua = Lua::new();
        let headers = add_default_headers(&lua, "https://api.github.com/repos/owner/repo", headers);

        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer explicit")
        );
    }

    #[test]
    fn test_add_default_headers_skips_release_asset_hosts() {
        let lua = Lua::new();
        lua.set_named_registry_value("github_token", "ghp_registry")
            .unwrap();

        let headers = add_default_headers(
            &lua,
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/file",
            HeaderMap::default(),
        );

        assert!(!headers.contains_key(AUTHORIZATION));
    }

    #[test]
    fn test_add_default_headers_skips_github_release_download_url() {
        // Sending auth to github.com release downloads makes GitHub redirect
        // to objects.githubusercontent.com, which 401s once reqwest strips
        // Authorization on the cross-origin hop.
        let lua = Lua::new();
        lua.set_named_registry_value("github_token", "ghp_registry")
            .unwrap();

        let headers = add_default_headers(
            &lua,
            "https://github.com/JetBrains/kotlin/releases/download/v2.0.20/kotlin-compiler-2.0.20.zip",
            HeaderMap::default(),
        );

        assert!(!headers.contains_key(AUTHORIZATION));
    }

    #[test]
    fn test_add_default_headers_skips_raw_githubusercontent() {
        let lua = Lua::new();
        lua.set_named_registry_value("github_token", "ghp_registry")
            .unwrap();

        let headers = add_default_headers(
            &lua,
            "https://raw.githubusercontent.com/owner/repo/main/file.txt",
            HeaderMap::default(),
        );

        assert!(!headers.contains_key(AUTHORIZATION));
        assert!(!headers.contains_key("x-github-api-version"));
    }

    #[test]
    fn test_add_default_headers_attaches_to_ghe_api_host() {
        let lua = Lua::new();
        lua.set_named_registry_value("github_token", "ghe_token")
            .unwrap();

        let headers = add_default_headers(
            &lua,
            "https://api.octocorp.ghe.com/repos/owner/repo/releases",
            HeaderMap::default(),
        );

        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer ghe_token")
        );
        assert_eq!(
            headers
                .get("x-github-api-version")
                .and_then(|value| value.to_str().ok()),
            Some("2022-11-28")
        );
    }

    #[tokio::test]
    async fn test_head() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .and(path("/get"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .insert_header("x-test-header", "test-value"),
            )
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let url = server.uri() + "/get";
        lua.load(mlua::chunk! {
            local http = require("http")
            local resp = http.head({ url = $url })
            assert(resp.status_code == 200)
            assert(type(resp.headers) == "table")
            assert(resp.headers["content-type"] == "application/json")
            assert(resp.headers["x-test-header"] == "test-value")
            assert(resp.content_length == nil)
        })
        .exec_async()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_head_retries_transient_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            for status in [503, 200] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0_u8; 1024];
                let _ = stream.read(&mut buf).unwrap();
                let response = if status == 200 {
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nX-Test-Header: ok\r\nContent-Length: 0\r\n\r\n"
                } else {
                    "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
                };
                stream.write_all(response.as_bytes()).unwrap();
                stream.flush().unwrap();
            }
        });

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let url = format!("http://{addr}/retry-head");
        lua.load(mlua::chunk! {
            local http = require("http")
            local resp = http.head({ url = $url })
            assert(resp.status_code == 200)
            assert(resp.headers["x-test-header"] == "ok")
        })
        .exec_async()
        .await
        .unwrap();

        server.join().unwrap();
    }

    #[tokio::test]
    async fn test_download_file() {
        let server = MockServer::start().await;

        // Create test content
        let test_content = r#"{"name": "vfox-nodejs", "version": "1.0.0"}"#;

        Mock::given(method("GET"))
            .and(path("/index.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(test_content)
                    .insert_header("content-type", "application/json"),
            )
            .expect(1) // Expect exactly one request
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        // Use isolated temp directory for test isolation
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("download_file.txt");
        let path_str = path.to_string_lossy().to_string();
        let url = server.uri() + "/index.json";

        lua.load(mlua::chunk! {
            local http = require("http")
            err = http.download_file({
                url = $url,
                headers = {}
            }, $path_str)
            assert(err == nil, [[must be nil]])
        })
        .exec_async()
        .await
        .unwrap();

        // Add a small delay to ensure file write is completed
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify file was downloaded correctly with better error handling
        let content = tokio::fs::read_to_string(&path)
            .await
            .unwrap_or_else(|e| panic!("Failed to read file at {:?}: {}", path, e));

        assert!(
            content.contains("vfox-nodejs"),
            "Expected content to contain 'vfox-nodejs', but got: {:?}",
            content
        );

        // TempDir automatically cleans up when dropped
    }

    #[tokio::test]
    async fn test_try_get_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/get"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"message": "ok"}))
                    .insert_header("content-type", "application/json"),
            )
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let url = server.uri() + "/get";
        lua.load(mlua::chunk! {
            local http = require("http")
            local resp, err = http.try_get({ url = $url })
            assert(err == nil, "expected no error, got: " .. tostring(err))
            assert(resp ~= nil, "expected response")
            assert(resp.status_code == 200)
            assert(type(resp.body) == "string")
        })
        .exec_async()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_try_get_failure() {
        let lua = Lua::new();
        mod_http(&lua).unwrap();

        // Use a URL that will fail to connect
        lua.load(mlua::chunk! {
            local http = require("http")
            local resp, err = http.try_get({ url = "http://127.0.0.1:1/" })
            assert(resp == nil, "expected nil response")
            assert(type(err) == "string", "expected error string, got: " .. type(err))
        })
        .exec_async()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_try_head_success() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .and(path("/head"))
            .respond_with(ResponseTemplate::new(200).insert_header("x-test", "value"))
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let url = server.uri() + "/head";
        lua.load(mlua::chunk! {
            local http = require("http")
            local resp, err = http.try_head({ url = $url })
            assert(err == nil, "expected no error")
            assert(resp.status_code == 200)
            assert(resp.headers["x-test"] == "value")
        })
        .exec_async()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_try_head_failure() {
        let lua = Lua::new();
        mod_http(&lua).unwrap();

        lua.load(mlua::chunk! {
            local http = require("http")
            local resp, err = http.try_head({ url = "http://127.0.0.1:1/" })
            assert(resp == nil, "expected nil response")
            assert(type(err) == "string", "expected error string")
        })
        .exec_async()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_try_download_file_success() {
        let server = MockServer::start().await;
        let test_content = "hello world";

        Mock::given(method("GET"))
            .and(path("/file.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(test_content))
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let temp_dir = tempfile::TempDir::new().unwrap();
        let file_path = temp_dir.path().join("downloaded.txt");
        let path_str = file_path.to_string_lossy().to_string();
        let url = server.uri() + "/file.txt";

        lua.load(mlua::chunk! {
            local http = require("http")
            local ok, err = http.try_download_file({ url = $url, headers = {} }, $path_str)
            assert(ok == true, "expected true, got: " .. tostring(ok))
            assert(err == nil, "expected no error, got: " .. tostring(err))
        })
        .exec_async()
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, test_content);
    }

    #[tokio::test]
    async fn test_try_download_file_failure() {
        let lua = Lua::new();
        mod_http(&lua).unwrap();

        lua.load(mlua::chunk! {
            local http = require("http")
            local _, err = http.try_download_file({ url = "http://127.0.0.1:1/", headers = {} }, "/tmp/should_not_exist.txt")
            assert(type(err) == "string", "expected error string, got: " .. type(err))
        })
        .exec_async()
        .await
        .unwrap();
    }
}
