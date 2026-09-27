use super::*;

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
