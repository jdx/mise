use super::*;

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
