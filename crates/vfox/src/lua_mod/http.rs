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
    use std::fs;
    use wiremock::matchers::{method, path};
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

        // Verify file was downloaded correctly
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("vfox-nodejs"));

        // TempDir automatically cleans up when dropped
    }
}
