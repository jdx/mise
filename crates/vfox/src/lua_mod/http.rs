use std::future::Future;

use mlua::{BorrowedStr, ExternalResult, Lua, MultiValue, Result, Table, Value};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use reqwest::{RequestBuilder, Response};
use url::Url;

use crate::http::{
    CLIENT, HttpCancellation, http_cancellation, http_retry_attempts, is_transient, retry_async,
    retry_delay, should_retry_status,
};

async fn cancel_on_interrupt<T, F>(operation: F) -> Result<T>
where
    F: Future<Output = std::result::Result<T, reqwest::Error>>,
{
    let mut cancellation = http_cancellation().subscribe();
    cancel_on_signal(operation, cancellation.cancelled()).await
}

async fn cancel_on_signal<T, F, C>(operation: F, cancelled: C) -> Result<T>
where
    F: Future<Output = std::result::Result<T, reqwest::Error>>,
    C: Future<Output = ()>,
{
    tokio::select! {
        result = operation => result.into_lua_err(),
        () = cancelled => Err(mlua::Error::runtime("interrupted")),
    }
}

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

fn add_default_headers_for_request(
    lua: &Lua,
    original_url: &str,
    request_url: &str,
    headers: HeaderMap,
) -> HeaderMap {
    let same_origin = Url::parse(original_url)
        .ok()
        .zip(Url::parse(request_url).ok())
        .is_some_and(|(original, request)| original.origin() == request.origin());

    // Do not forward credentials selected for the original origin, but retain
    // non-sensitive plugin headers that may be required by the replacement.
    if !same_origin {
        return headers
            .iter()
            .filter(|(name, _)| !is_sensitive_header(name))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
    }

    add_default_headers(lua, original_url, headers)
}

fn is_sensitive_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    let normalized: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();

    matches!(
        name,
        "authorization"
            | "cookie"
            | "cookie2"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "set-cookie"
            | "www-authenticate"
    ) || normalized.ends_with("auth")
        || [
            "accesskey",
            "accesstoken",
            "apikey",
            "apitoken",
            "authtoken",
            "authentication",
            "authorization",
            "bearertoken",
            "credential",
            "githubtoken",
            "gitlabtoken",
            "idtoken",
            "privatekey",
            "privatetoken",
            "refreshtoken",
            "secret",
            "secretkey",
            "sessionid",
            "sessionkey",
            "sessiontoken",
            "subscriptionkey",
            "vaulttoken",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn rewrite_url(lua: &Lua, url: &str) -> Result<String> {
    match lua.named_registry_value::<mlua::Function>(crate::http::URL_REWRITER_REGISTRY_KEY) {
        Ok(rewriter) => rewriter.call(url),
        Err(_) => Ok(url.to_string()),
    }
}

async fn get(lua: &Lua, input: Table) -> Result<Table> {
    get_with_cancellation(lua, input, http_cancellation()).await
}

async fn get_with_cancellation(
    lua: &Lua,
    input: Table,
    cancellation: &HttpCancellation,
) -> Result<Table> {
    let mut cancellation = cancellation.subscribe();
    let url: String = input.get("url").into_lua_err()?;
    let headers = match input.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let request_url = rewrite_url(lua, &url)?;
    let headers = add_default_headers_for_request(lua, &url, &request_url, headers);
    let resp = cancel_on_signal(
        send_with_retry(CLIENT.get(&request_url).headers(headers)),
        cancellation.cancelled(),
    )
    .await?;
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    let body = cancel_on_signal(resp.text(), cancellation.cancelled()).await?;
    t.set("body", body)?;
    Ok(t)
}

async fn download_file(lua: &Lua, input: MultiValue) -> Result<()> {
    let t: &Table = input.iter().next().unwrap().as_table().unwrap();
    let url: String = t.get("url").into_lua_err()?;
    let headers = match t.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let request_url = rewrite_url(lua, &url)?;
    let headers = add_default_headers_for_request(lua, &url, &request_url, headers);
    let path: String = input.iter().nth(1).unwrap().to_string()?;
    // Retry the whole flow (request + body) so a mid-stream drop restarts the
    // download instead of failing.
    let bytes = cancel_on_interrupt(retry_async(&request_url, || async {
        let resp = CLIENT
            .get(&request_url)
            .headers(headers.clone())
            .send()
            .await?;
        let resp = resp.error_for_status()?;
        resp.bytes().await
    }))
    .await?;
    // Create the parent directory so plugins don't have to shell out to `mkdir`
    // before downloading into a fresh install path.
    if let Some(parent) = std::path::Path::new(&path).parent() {
        tokio::fs::create_dir_all(parent).await.into_lua_err()?;
    }
    let mut file = tokio::fs::File::create(&path).await.into_lua_err()?;
    tokio::io::AsyncWriteExt::write_all(&mut file, &bytes)
        .await
        .into_lua_err()?;
    tokio::io::AsyncWriteExt::flush(&mut file)
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
    let request_url = rewrite_url(lua, &url)?;
    let headers = add_default_headers_for_request(lua, &url, &request_url, headers);
    let resp =
        cancel_on_interrupt(send_with_retry(CLIENT.head(&request_url).headers(headers))).await?;
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    Ok(t)
}

async fn try_get(lua: &Lua, input: Table) -> Result<MultiValue> {
    try_get_with_cancellation(lua, input, http_cancellation()).await
}

async fn try_get_with_cancellation(
    lua: &Lua,
    input: Table,
    cancellation: &HttpCancellation,
) -> Result<MultiValue> {
    let mut cancellation = cancellation.subscribe();
    let url: String = input.get("url").into_lua_err()?;
    let headers = match input.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let request_url = rewrite_url(lua, &url)?;
    let headers = add_default_headers_for_request(lua, &url, &request_url, headers);
    let resp = match cancel_on_signal(
        send_with_retry(CLIENT.get(&request_url).headers(headers)),
        cancellation.cancelled(),
    )
    .await
    {
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
    match cancel_on_signal(resp.text(), cancellation.cancelled()).await {
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
    let request_url = rewrite_url(lua, &url)?;
    let headers = add_default_headers_for_request(lua, &url, &request_url, headers);
    let resp = match cancel_on_interrupt(send_with_retry(
        CLIENT.head(&request_url).headers(headers),
    ))
    .await
    {
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
    let request_url = rewrite_url(lua, &url)?;
    let headers = add_default_headers_for_request(lua, &url, &request_url, headers);
    let path = match input.get(1).and_then(|v| v.to_string().ok()) {
        Some(p) => p,
        None => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(lua.create_string("second argument must be a string path")?),
            ]));
        }
    };
    let bytes = match cancel_on_interrupt(retry_async(&request_url, || async {
        let resp = CLIENT
            .get(&request_url)
            .headers(headers.clone())
            .send()
            .await?;
        let resp = resp.error_for_status()?;
        resp.bytes().await
    }))
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
    // Create the parent directory so plugins don't have to shell out to `mkdir`
    // before downloading into a fresh install path.
    if let Some(parent) = std::path::Path::new(&path).parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        return Ok(MultiValue::from_vec(vec![
            Value::Nil,
            Value::String(lua.create_string(e.to_string())?),
        ]));
    }
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
    if let Err(e) = tokio::io::AsyncWriteExt::flush(&mut file).await {
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
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_http_operation_is_cancelled_on_interrupt() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/pending", listener.local_addr().unwrap());
        let cancellation = HttpCancellation::default();
        let trigger = cancellation.clone();
        let (release_tx, release_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let bytes_read = stream.read(&mut request).unwrap();
            assert!(bytes_read > 0, "client closed before sending its request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
                .unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(50));
            trigger.cancel();
            release_rx.recv().unwrap();
        });

        let lua = Lua::new();
        let input = lua.create_table().unwrap();
        input.set("url", url).unwrap();
        let err = get_with_cancellation(&lua, input, &cancellation)
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), "runtime error: interrupted");
        let mut later = cancellation.subscribe();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), later.cancelled())
                .await
                .is_err(),
            "a later operation should wait for the next cancellation generation"
        );
        release_tx.send(()).unwrap();
        server.join().unwrap();
    }

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
            assert!(!request.headers.contains_key(AUTHORIZATION));
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

    #[tokio::test]
    async fn test_try_download_file_creates_parent_dirs() {
        let server = MockServer::start().await;
        let test_content = "nested content";

        Mock::given(method("GET"))
            .and(path("/file.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(test_content))
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let temp_dir = tempfile::TempDir::new().unwrap();
        // Target a nested directory that does not exist yet.
        let file_path = temp_dir.path().join("a").join("b").join("downloaded.txt");
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
    async fn test_download_file_creates_parent_dirs() {
        let server = MockServer::start().await;
        let test_content = "nested content";

        Mock::given(method("GET"))
            .and(path("/file.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(test_content))
            .mount(&server)
            .await;

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let temp_dir = tempfile::TempDir::new().unwrap();
        let file_path = temp_dir.path().join("x").join("y").join("downloaded.txt");
        let path_str = file_path.to_string_lossy().to_string();
        let url = server.uri() + "/file.txt";

        lua.load(mlua::chunk! {
            local http = require("http")
            local err = http.download_file({ url = $url, headers = {} }, $path_str)
            assert(err == nil, "expected no error, got: " .. tostring(err))
        })
        .exec_async()
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, test_content);
    }
}
