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

pub(crate) fn mod_http(lua: &Lua) -> Result<()> {
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
    let headers = if !same_origin {
        headers
            .iter()
            .filter(|(name, _)| !is_sensitive_header(name))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    } else {
        add_default_headers(lua, original_url, headers)
    };

    add_resolved_headers(lua, request_url, headers)
}

fn add_resolved_headers(lua: &Lua, url: &str, mut headers: HeaderMap) -> HeaderMap {
    let Ok(resolver) =
        lua.named_registry_value::<mlua::Function>(crate::http::HTTP_HEADERS_RESOLVER_REGISTRY_KEY)
    else {
        return headers;
    };
    let Ok(table) = resolver.call::<Table>(url) else {
        return headers;
    };
    let Ok(resolved) = into_headers(&table) else {
        return headers;
    };
    for (name, value) in resolved {
        if let Some(name) = name {
            headers.entry(name).or_insert(value);
        }
    }
    headers
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
mod tests;
