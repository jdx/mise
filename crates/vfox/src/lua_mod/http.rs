use mlua::{BorrowedStr, ExternalResult, Lua, MultiValue, Result, Table, Value};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::http::CLIENT;

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
                lua.create_async_function(|_lua: mlua::Lua, input| async move {
                    download_file(&_lua, input).await
                })?,
            ),
            (
                "try_download_file",
                lua.create_async_function(|_lua: mlua::Lua, input| async move {
                    try_download_file(&_lua, input).await
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

async fn get(lua: &Lua, input: Table) -> Result<Table> {
    let url: String = input.get("url").into_lua_err()?;
    let headers = match input.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let resp = CLIENT
        .get(&url)
        .headers(headers)
        .send()
        .await
        .into_lua_err()?;
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    t.set("body", resp.text().await.into_lua_err()?)?;
    Ok(t)
}

async fn download_file(_lua: &Lua, input: MultiValue) -> Result<()> {
    let t: &Table = input.iter().next().unwrap().as_table().unwrap();
    let url: String = t.get("url").into_lua_err()?;
    let headers = match t.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let path: String = input.iter().nth(1).unwrap().to_string()?;
    let resp = CLIENT
        .get(&url)
        .headers(headers)
        .send()
        .await
        .into_lua_err()?;
    resp.error_for_status_ref().into_lua_err()?;
    let mut file = tokio::fs::File::create(&path).await.into_lua_err()?;
    let bytes = resp.bytes().await.into_lua_err()?;
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
    let resp = CLIENT
        .head(&url)
        .headers(headers)
        .send()
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
    let resp = match CLIENT.get(&url).headers(headers).send().await {
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
    let resp = match CLIENT.head(&url).headers(headers).send().await {
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

async fn try_download_file(_lua: &Lua, input: MultiValue) -> Result<MultiValue> {
    let t = match input.front().and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(_lua.create_string("first argument must be a table")?),
            ]));
        }
    };
    let url: String = t.get("url").into_lua_err()?;
    let headers = match t.get::<Option<Table>>("headers").into_lua_err()? {
        Some(tbl) => into_headers(&tbl)?,
        None => HeaderMap::default(),
    };
    let path = match input.get(1).and_then(|v| v.to_string().ok()) {
        Some(p) => p,
        None => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(_lua.create_string("second argument must be a string path")?),
            ]));
        }
    };
    let resp = match CLIENT.get(&url).headers(headers).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(_lua.create_string(e.to_string())?),
            ]));
        }
    };
    if let Err(e) = resp.error_for_status_ref() {
        return Ok(MultiValue::from_vec(vec![
            Value::Nil,
            Value::String(_lua.create_string(e.to_string())?),
        ]));
    }
    let bytes = match resp.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(_lua.create_string(e.to_string())?),
            ]));
        }
    };
    let mut file = match tokio::fs::File::create(&path).await {
        Ok(f) => f,
        Err(e) => {
            return Ok(MultiValue::from_vec(vec![
                Value::Nil,
                Value::String(_lua.create_string(e.to_string())?),
            ]));
        }
    };
    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await {
        return Ok(MultiValue::from_vec(vec![
            Value::Nil,
            Value::String(_lua.create_string(e.to_string())?),
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
