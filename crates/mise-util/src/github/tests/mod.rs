use super::*;

// Not from mise-versions, so falling back to it skips the asset identity
// check (see `checked_api_asset_url`), as for a private repo.
const ASSET_API_URL: &str = "https://api.github.com/repos/o/r/releases/assets/1";

fn replaced_release(digest: Option<&str>) -> GithubRelease {
    GithubRelease {
        tag_name: "v0.2.76".to_string(),
        draft: false,
        prerelease: false,
        created_at: "2026-09-23T01:40:00Z".to_string(),
        published_at: Some("2026-09-23T01:45:05Z".to_string()),
        assets: vec![GithubAsset {
            from_versions_host: false,
            name: "rumdl.tar.gz".to_string(),
            browser_download_url: String::new(),
            url: String::new(),
            digest: digest.map(str::to_string),
            updated_at: Some("2026-09-23T08:25:23Z".to_string()),
        }],
    }
}

const GITHUB_TOKEN_VARS: [&str; 4] = [
    "MISE_GITHUB_TOKEN",
    "GITHUB_API_TOKEN",
    "GITHUB_TOKEN",
    "MISE_GITHUB_ENTERPRISE_TOKEN",
];

/// Holds [`super::TEST_ENV_LOCK`] and puts the token variables back in `Drop`.
///
/// Restoring in `Drop` rather than after the callback is what makes the lock's poison flag
/// unnecessary: `Drop` runs while unwinding, so a panicking test cannot leave `ghp_test`
/// behind for whatever runs next.
struct GithubTokenGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    // `var_os`, not `var`: the latter reports a non-Unicode value as `None`, which `Drop`
    // would then read as "was unset" and delete. Same shape as `crate::testing::EnvVarGuard`.
    prev: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl GithubTokenGuard {
    fn new() -> Self {
        let lock = crate::testing::lock_ignoring_poison(&super::TEST_ENV_LOCK);
        let prev = GITHUB_TOKEN_VARS
            .iter()
            .map(|name| (*name, std::env::var_os(name)))
            .collect();

        env::remove_var("MISE_GITHUB_TOKEN");
        env::remove_var("GITHUB_API_TOKEN");
        env::set_var("GITHUB_TOKEN", "ghp_test");
        env::remove_var("MISE_GITHUB_ENTERPRISE_TOKEN");

        Self { _lock: lock, prev }
    }
}

impl Drop for GithubTokenGuard {
    fn drop(&mut self) {
        for (name, prev) in self.prev.drain(..) {
            match prev {
                Some(v) => env::set_var(name, v),
                None => env::remove_var(name),
            }
        }
    }
}

fn with_github_token<F, R>(test_fn: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = GithubTokenGuard::new();
    test_fn()
}

struct TokensFileOverrideGuard;

impl TokensFileOverrideGuard {
    fn set(host: &str, token: &str) -> Self {
        let mut tokens = HashMap::new();
        tokens.insert(host.to_string(), token.to_string());
        *test_support::TOKENS_FILE_OVERRIDE.write().unwrap() = Some(tokens);
        Self
    }
}

impl Drop for TokensFileOverrideGuard {
    fn drop(&mut self) {
        *test_support::TOKENS_FILE_OVERRIDE.write().unwrap() = None;
    }
}

fn make_release(tag: &str) -> GithubRelease {
    GithubRelease {
        tag_name: tag.to_string(),
        draft: false,
        prerelease: false,
        created_at: String::new(),
        published_at: None,
        assets: vec![],
    }
}

fn asset(name: &str) -> GithubAsset {
    GithubAsset {
        from_versions_host: false,
        name: name.to_string(),
        browser_download_url: format!("https://example.invalid/{name}"),
        url: format!("https://example.invalid/api/{name}"),
        digest: None,
        updated_at: None,
    }
}

fn make_asset(name: &str) -> GithubAsset {
    GithubAsset {
        from_versions_host: false,
        name: name.to_string(),
        browser_download_url: format!("https://github.com/owner/repo/releases/download/{name}"),
        url: format!("https://api.github.com/repos/owner/repo/releases/assets/{name}"),
        digest: None,
        updated_at: None,
    }
}

fn make_prerelease(tag: &str) -> GithubRelease {
    GithubRelease {
        prerelease: true,
        ..make_release(tag)
    }
}

const PAGINATE_TEST_TOKEN: &str = "ghp_paginate_test";

/// A GHES-shaped base URL: `get_headers` only attaches auth to REST API URLs, and for a
/// host that is not api.github.com that means the path must sit under [`API_PATH`].
fn ghes_api_url(base: &str) -> String {
    format!("{base}{API_PATH}")
}

fn tag_without_commit(name: &str) -> GithubTag {
    GithubTag {
        name: name.to_string(),
        commit: None,
    }
}

mod api;
mod asset_notes;
mod listing;
mod mirrored_releases;
mod release_selection;
mod tokens;
