use mlua::{ExternalResult, Lua, MultiValue, Result, Table};

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
                "head",
                lua.create_async_function(|lua: mlua::Lua, input| async move {
                    head(&lua, input).await
                })?,
            ),
            (
                "download_file",
                lua.create_async_function(|_lua: mlua::Lua, input| async move {
                    download_file(&_lua, input).await
                })?,
            ),
        ])?,
    )
}

async fn get(lua: &Lua, input: Table) -> Result<Table> {
    let url: String = input.get("url").into_lua_err()?;
    let resp = CLIENT.get(&url).send().await.into_lua_err()?;
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    t.set("body", resp.text().await.into_lua_err()?)?;
    Ok(t)
}

async fn download_file(_lua: &Lua, input: MultiValue) -> Result<()> {
    let t: &Table = input.iter().next().unwrap().as_table().unwrap();
    let url: String = t.get("url").into_lua_err()?;
    let path: String = input.iter().nth(1).unwrap().to_string()?;
    let resp = CLIENT.get(&url).send().await.into_lua_err()?;
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
    let resp = CLIENT.head(&url).send().await.into_lua_err()?;
    let t = lua.create_table()?;
    t.set("status_code", resp.status().as_u16())?;
    t.set("headers", get_headers(lua, resp.headers())?)?;
    Ok(t)
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
    use httpmock::prelude::*;
    use httpmock::Method::{GET, HEAD};
    use std::fs;

    #[tokio::test]
    async fn test_get() {
        // Start a local mock server
        let server = MockServer::start();

        // Create a mock endpoint
        let mock = server.mock(|when, then| {
            when.method(GET).path("/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "message": "test response"
                }));
        });

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let url = server.url("/get");
        lua.load(format!(
            r#"
            local http = require("http")
            local resp = http.get({{ url = "{}" }})
            assert(resp.status_code == 200)
            assert(type(resp.body) == "string")
        "#,
            url
        ))
        .exec_async()
        .await
        .unwrap();

        // Verify the mock was called
        mock.assert();
    }

    #[tokio::test]
    async fn test_head() {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(HEAD).path("/get");
            then.status(200)
                .header("content-type", "application/json")
                .header("x-test-header", "test-value");
        });

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        let url = server.url("/get");
        lua.load(format!(
            r#"
            local http = require("http")
            local resp = http.head({{ url = "{}" }})
            assert(resp.status_code == 200)
            assert(type(resp.headers) == "table")
            assert(resp.headers["content-type"] == "application/json")
            assert(resp.headers["x-test-header"] == "test-value")
            assert(resp.content_length == nil)
        "#,
            url
        ))
        .exec_async()
        .await
        .unwrap();

        mock.assert();
    }

    #[tokio::test]
    async fn test_download_file() {
        let server = MockServer::start();

        // Create test content
        let test_content = r#"{"name": "vfox-nodejs", "version": "1.0.0"}"#;

        let mock = server.mock(|when, then| {
            when.method(GET).path("/index.json");
            then.status(200)
                .header("content-type", "application/json")
                .body(test_content);
        });

        let lua = Lua::new();
        mod_http(&lua).unwrap();

        // Use temp_dir for cross-platform compatibility
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("vfox_test_download_file.txt");
        let path_str = path.to_string_lossy();
        let url = server.url("/index.json");

        lua.load(format!(
            r#"
            local http = require("http")
            err = http.download_file({{
                url = "{}",
                headers = {{}}
            }}, "{}")
            assert(err == nil, [[must be nil]])
        "#,
            url, path_str
        ))
        .exec_async()
        .await
        .unwrap();

        // Verify file was downloaded correctly
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("vfox-nodejs"));

        // Clean up
        tokio::fs::remove_file(path).await.unwrap();

        mock.assert();
    }
}
