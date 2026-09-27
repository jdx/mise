use super::*;

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
