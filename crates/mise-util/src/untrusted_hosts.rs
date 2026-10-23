//! Hosts that safe mode keeps the operator's credentials away from.
//!
//! Safe mode loads project config without trust, and project config can name
//! the host a request goes to: a forge `api_url`, or an http `version_list_url`.
//! Forge tokens such as `GITHUB_TOKEN` or `MISE_GITHUB_ENTERPRISE_TOKEN` are not
//! scoped to a host, and a netrc `default` entry is not either, so without this
//! a branch could name its own host and receive them. Config loading records the
//! hosts project config names here, and the credential lookups skip them.

use std::collections::HashSet;
use std::sync::Mutex;

/// Hosts a project `api_url` names, which get no forge token.
static FORGE_TOKEN_DENIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Hosts a global `api_url` names. The operator chose them, so they keep their
/// token even when a project names the same host.
static FORGE_TOKEN_ALLOWED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Hosts a project `version_list_url` names, which get no netrc credentials.
static NETRC_DENIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

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

/// Records the host of a `version_list_url` that project config set in safe mode.
pub fn deny_netrc_for_url(url: &str) {
    if let Some(host) = host_of(url) {
        insert(&NETRC_DENIED, host);
    }
}

/// Whether netrc credentials must not be sent to `host`.
pub fn netrc_denied(host: &str) -> bool {
    contains(&NETRC_DENIED, host)
}

#[cfg(test)]
pub(crate) fn reset() {
    for set in [&FORGE_TOKEN_DENIED, &FORGE_TOKEN_ALLOWED, &NETRC_DENIED] {
        *set.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_untrusted_hosts() {
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
        deny_netrc_for_url("https://files.example/versions.txt");
        assert!(netrc_denied("FILES.example"));
        assert!(!netrc_denied("evil.example"));
        reset();
    }
}
