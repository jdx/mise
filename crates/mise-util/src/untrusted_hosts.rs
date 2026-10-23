//! Hosts that safe mode keeps the operator's credentials away from.
//!
//! Safe mode loads project config without trust, and project config can name
//! the host a request goes to: a forge `api_url`, or a URL a tool downloads or
//! lists versions from.
//!
//! Forge tokens such as `GITHUB_TOKEN` or `MISE_GITHUB_ENTERPRISE_TOKEN` are not
//! scoped to a host, so config loading records the hosts a project `api_url`
//! names, and the token lookups skip them.
//!
//! netrc is decided by who asks rather than by the URL: work done for a tool
//! that project config declares runs inside [`with_project_netrc_scope`], which
//! names the hosts that tool's project config chose, and those get no netrc
//! credentials (a netrc `default` entry included). Requests for tools from
//! global or system config run outside any scope and are unaffected.

use std::collections::HashSet;
use std::future::Future;
use std::sync::{Arc, Mutex};

/// Hosts a project `api_url` names, which get no forge token.
static FORGE_TOKEN_DENIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Hosts a global `api_url` names. The operator chose them, so they keep their
/// token even when a project names the same host.
static FORGE_TOKEN_ALLOWED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// The public forges. A token sent there goes to the service it was issued for,
/// so a project naming one of them is not a reason to withhold it.
const PUBLIC_FORGE_HOSTS: [&str; 4] =
    ["github.com", "api.github.com", "gitlab.com", "codeberg.org"];

fn host_of(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    url.host_str().map(str::to_ascii_lowercase)
}

fn insert(set: &Mutex<Option<HashSet<String>>>, host: String) {
    set.lock()
        .unwrap()
        .get_or_insert_with(HashSet::new)
        .insert(host);
}

fn contains(set: &Mutex<Option<HashSet<String>>>, host: &str) -> bool {
    set.lock()
        .unwrap()
        .as_ref()
        .is_some_and(|hosts| hosts.contains(&host.to_ascii_lowercase()))
}

/// Records the host of an `api_url` that project config set in safe mode.
pub fn deny_forge_token_for_url(api_url: &str) {
    if let Some(host) = host_of(api_url)
        && !PUBLIC_FORGE_HOSTS.contains(&host.as_str())
    {
        insert(&FORGE_TOKEN_DENIED, host);
    }
}

/// Records the host of an `api_url` that global or system config set.
pub fn allow_forge_token_for_url(api_url: &str) {
    if let Some(host) = host_of(api_url) {
        insert(&FORGE_TOKEN_ALLOWED, host);
    }
}

/// Whether forge tokens must not be sent to `host`.
pub fn forge_token_denied(host: &str) -> bool {
    contains(&FORGE_TOKEN_DENIED, host) && !contains(&FORGE_TOKEN_ALLOWED, host)
}

/// The hosts that the project config of the tool being worked on chose.
#[derive(Debug, Default)]
struct ProjectNetrcScope {
    hosts: HashSet<String>,
    /// A URL names its host through a template, which renders only at the
    /// request, so every host is withheld.
    any_host: bool,
}

tokio::task_local! {
    static PROJECT_NETRC_SCOPE: Arc<ProjectNetrcScope>;
}

/// Runs `future`, the work for a tool that project config declares, so that
/// none of its requests to a host one of `urls` names gets netrc credentials.
/// `urls` are that tool's URL options (templates, possibly); a URL whose host is
/// itself a template withholds netrc from every host for the tool.
pub async fn with_project_netrc_scope<'a, T>(
    urls: impl IntoIterator<Item = &'a str>,
    future: impl Future<Output = T>,
) -> T {
    let mut scope = ProjectNetrcScope::default();
    for url in urls {
        let authority = url
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default());
        match (authority, host_of(url)) {
            (Some(authority), Some(host)) if !authority.contains('{') => {
                scope.hosts.insert(host);
            }
            _ => scope.any_host = true,
        }
    }
    PROJECT_NETRC_SCOPE.scope(Arc::new(scope), future).await
}

/// Whether netrc credentials must not be sent with a request for `url`, because
/// it is made for a project tool whose config named its host.
pub fn netrc_denied(url: &url::Url) -> bool {
    PROJECT_NETRC_SCOPE
        .try_with(|scope| {
            scope.any_host
                || url
                    .host_str()
                    .is_some_and(|host| scope.hosts.contains(&host.to_ascii_lowercase()))
        })
        .unwrap_or(false)
}

#[cfg(test)]
pub(crate) fn reset() {
    for set in [&FORGE_TOKEN_DENIED, &FORGE_TOKEN_ALLOWED] {
        *set.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_untrusted_forge_hosts() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        reset();
        deny_forge_token_for_url("https://Evil.example/api/v3");
        deny_forge_token_for_url("https://api.github.com");
        deny_forge_token_for_url("https://ghe.corp.example/api/v3");
        allow_forge_token_for_url("https://ghe.corp.example/api/v3");
        assert!(forge_token_denied("evil.example"));
        assert!(!forge_token_denied("api.github.com"));
        assert!(!forge_token_denied("ghe.corp.example"));
        assert!(!forge_token_denied("other.example"));
        reset();
    }

    #[tokio::test]
    async fn test_project_netrc_scope() {
        let url = |s: &str| url::Url::parse(s).unwrap();
        // outside a scope (a global tool, or no tool at all) nothing is withheld
        assert!(!netrc_denied(&url("https://files.example/private.env")));
        with_project_netrc_scope(
            [
                "https://Files.example:443/project-{{ version }}.tgz",
                "https://lists.example/versions.txt",
            ],
            async {
                assert!(netrc_denied(&url("https://files.example/private.env")));
                assert!(netrc_denied(&url("https://FILES.example/other")));
                assert!(netrc_denied(&url("https://lists.example/versions.txt")));
                // a host the project did not name, such as a URL replacement's
                assert!(!netrc_denied(&url("https://mirror.example/x")));
            },
        )
        .await;
        with_project_netrc_scope(["https://{{ os() }}.evil.example/tool"], async {
            assert!(netrc_denied(&url("https://linux.evil.example/tool")));
            assert!(netrc_denied(&url("https://mirror.example/x")));
        })
        .await;
        assert!(!netrc_denied(&url("https://files.example/private.env")));
    }
}
