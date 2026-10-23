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
/// Set when a project URL names its host through a template, so the host is
/// not known until the request: netrc is then withheld from every host.
static NETRC_DENIED_ANY_HOST: Mutex<bool> = Mutex::new(false);
/// The URLs that global or system config names. They keep their netrc
/// credentials even on a host a project also names. This is kept per URL rather
/// than per host: allowing the host would let a project read any other path on
/// the operator's server with the operator's credentials.
static NETRC_ALLOWED_URLS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// The text before the first template tag of the templated URLs that global or
/// system config names (`https://files.example/tool-{{ version }}.tgz` allows
/// `https://files.example/tool-`), since the request renders the template.
static NETRC_ALLOWED_PREFIXES: Mutex<Vec<String>> = Mutex::new(Vec::new());

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

fn normalized_url(url: &str) -> Option<String> {
    url::Url::parse(url).ok().map(String::from)
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

/// The part of a URL template before its first template tag, if it has one.
fn template_prefix(url: &str) -> Option<&str> {
    let tag = [url.find("{{"), url.find("{%"), url.find("{#")]
        .into_iter()
        .flatten()
        .min()?;
    Some(&url[..tag])
}

/// Whether the scheme and host of a URL are written out, before any template tag.
fn host_is_literal(prefix: &str) -> bool {
    prefix
        .split_once("://")
        .is_some_and(|(_, rest)| rest.contains(['/', '?', '#']))
}

/// Records the host of a URL (a template, possibly) that project config set in
/// safe mode, such as a `url` or `version_list_url`.
pub fn deny_netrc_for_url(url: &str) {
    let literal_host = template_prefix(url).is_none_or(host_is_literal);
    match host_of(url) {
        Some(host) if literal_host => insert(&NETRC_DENIED, host),
        _ => *NETRC_DENIED_ANY_HOST.lock().unwrap() = true,
    }
}

/// Records a URL (a template, possibly) that global or system config set.
pub fn allow_netrc_for_url(url: &str) {
    match template_prefix(url) {
        Some(prefix) if host_is_literal(prefix) => {
            NETRC_ALLOWED_PREFIXES
                .lock()
                .unwrap()
                .push(prefix.to_string());
        }
        Some(_) => {}
        None => {
            if let Some(url) = normalized_url(url) {
                NETRC_ALLOWED_URLS
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashSet::new)
                    .insert(url);
            }
        }
    }
}

/// Whether netrc credentials must not be sent with a request for `url`: its host
/// is one project config named, and global or system config does not name this
/// URL itself.
pub fn netrc_denied(url: &url::Url) -> bool {
    let host_denied = *NETRC_DENIED_ANY_HOST.lock().unwrap()
        || url
            .host_str()
            .is_some_and(|host| contains(&NETRC_DENIED, host));
    if !host_denied {
        return false;
    }
    let allowed = NETRC_ALLOWED_URLS
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|urls| urls.contains(url.as_str()))
        || NETRC_ALLOWED_PREFIXES
            .lock()
            .unwrap()
            .iter()
            .any(|prefix| url.as_str().starts_with(prefix.as_str()));
    !allowed
}

#[cfg(test)]
pub(crate) fn reset() {
    for set in [
        &FORGE_TOKEN_DENIED,
        &FORGE_TOKEN_ALLOWED,
        &NETRC_DENIED,
        &NETRC_ALLOWED_URLS,
    ] {
        *set.lock().unwrap() = None;
    }
    *NETRC_DENIED_ANY_HOST.lock().unwrap() = false;
    NETRC_ALLOWED_PREFIXES.lock().unwrap().clear();
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
        let url = |s: &str| url::Url::parse(s).unwrap();
        deny_netrc_for_url("https://files.example/project.txt");
        allow_netrc_for_url("https://FILES.example/global.txt");
        assert!(netrc_denied(&url("https://files.example/project.txt")));
        assert!(netrc_denied(&url("https://files.example/secret.env")));
        assert!(!netrc_denied(&url("https://files.example/global.txt")));
        assert!(!netrc_denied(&url("https://evil.example/versions.txt")));
        // templated URLs: the global one allows what it renders to
        deny_netrc_for_url("https://files.example/project-{{ version }}.tgz");
        allow_netrc_for_url("https://files.example/global-{{ version }}.tgz");
        assert!(!netrc_denied(&url(
            "https://files.example/global-1.0.0.tgz"
        )));
        assert!(netrc_denied(&url(
            "https://files.example/project-1.0.0.tgz"
        )));
        assert!(!netrc_denied(&url("https://other.example/x")));
        // a host that only a template names denies every host but the allowed URLs
        deny_netrc_for_url("https://{{ os() }}.evil.example/tool");
        assert!(netrc_denied(&url("https://linux.evil.example/tool")));
        assert!(netrc_denied(&url("https://other.example/x")));
        assert!(!netrc_denied(&url("https://files.example/global.txt")));
        reset();
    }
}
