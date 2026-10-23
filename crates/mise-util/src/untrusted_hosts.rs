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
//! netrc is withheld the same way, by default: config loading records every host
//! a project tool's URL options name (or every host at all, when one of them is
//! a template), and no request to such a host gets netrc credentials, a netrc
//! `default` entry included, whichever code path makes it. The exception is the
//! work done for a tool that global or system config declares, which runs inside
//! [`with_trusted_tool_scope`]: the operator chose that tool and its hosts, so
//! its requests keep their credentials even on a host a project also names. A
//! path that misses the scope therefore loses credentials rather than leaking
//! them.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Hosts a project `api_url` names, which get no forge token.
static FORGE_TOKEN_DENIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Hosts a global `api_url` names. The operator chose them, so they keep their
/// token even when a project names the same host.
static FORGE_TOKEN_ALLOWED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Hosts a project tool's URL options name, which get no netrc credentials
/// outside [`with_trusted_tool_scope`].
static NETRC_DENIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Set when a project URL names its host through a template, which renders
/// only at the request, so netrc is withheld from every host.
static NETRC_DENIED_ANY_HOST: AtomicBool = AtomicBool::new(false);

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

/// Records the host of a URL option (a template, possibly) of a tool that
/// project config declares, in safe mode. A URL whose host is itself a template
/// withholds netrc from every host.
pub fn deny_netrc_for_url(url: &str) {
    let authority = url
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default());
    match (authority, host_of(url)) {
        (Some(authority), Some(host)) if !authority.contains('{') => {
            insert(&NETRC_DENIED, host);
        }
        _ => NETRC_DENIED_ANY_HOST.store(true, Ordering::Relaxed),
    }
}

tokio::task_local! {
    static TRUSTED_TOOL: bool;
}

/// Runs `future`, the work for a tool that global or system config declares, so
/// that its requests keep their netrc credentials on a host a project also names.
pub async fn with_trusted_tool_scope<T>(future: impl Future<Output = T>) -> T {
    TRUSTED_TOOL.scope(true, future).await
}

/// Runs `future`, the work for one tool in safe mode, in that tool's scope:
/// trusted for a tool from global or system config (netrc on hosts a project
/// names, `env` in the templates its backend renders), and explicitly untrusted
/// for a tool that project config declares, even when it is reached from a
/// trusted tool's work, such as one tool's dependency pulling in another.
pub async fn with_tool_scope<T>(trusted: bool, future: impl Future<Output = T>) -> T {
    let tool = Box::pin(TRUSTED_TOOL.scope(trusted, future));
    crate::tera::with_templates_trusted(trusted, tool).await
}

/// Wraps `future`, about to be spawned as a task of its own (`tokio::spawn`, a
/// `JoinSet`, [`crate::parallel::parallel`]), so that it runs in the trusted
/// scopes of the task spawning it: task-locals do not pass to a spawned task, so
/// without this the work a global tool spreads over tasks would lose its netrc
/// credentials and its templates' `env`. A future spawned outside those scopes
/// stays outside them.
pub fn inherit_trusted_scope<F: Future>(future: F) -> impl Future<Output = F::Output> {
    let tool = TRUSTED_TOOL.try_with(|trusted| *trusted).unwrap_or(false);
    let templates = crate::tera::in_trusted_templates();
    async move {
        match (tool, templates) {
            (true, true) => with_tool_scope(true, future).await,
            (true, false) => with_trusted_tool_scope(future).await,
            (false, true) => crate::tera::with_trusted_templates(future).await,
            (false, false) => future.await,
        }
    }
}

/// Whether netrc credentials must not be sent with a request for `url`: project
/// config named its host, and the request is not made for a tool that global or
/// system config declares.
pub fn netrc_denied(url: &url::Url) -> bool {
    if TRUSTED_TOOL.try_with(|trusted| *trusted).unwrap_or(false) {
        return false;
    }
    NETRC_DENIED_ANY_HOST.load(Ordering::Relaxed)
        || url
            .host_str()
            .is_some_and(|host| contains(&NETRC_DENIED, host))
}

#[cfg(test)]
pub(crate) fn reset() {
    for set in [&FORGE_TOKEN_DENIED, &FORGE_TOKEN_ALLOWED, &NETRC_DENIED] {
        *set.lock().unwrap() = None;
    }
    NETRC_DENIED_ANY_HOST.store(false, Ordering::Relaxed);
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

    #[test]
    fn test_netrc_denied_by_default_for_project_hosts() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        reset();
        let url = |s: &str| url::Url::parse(s).unwrap();
        deny_netrc_for_url("https://Files.example:443/project-{{ version }}.tgz");
        // whichever path asks, outside a trusted scope
        assert!(netrc_denied(&url("https://files.example/private.env")));
        assert!(netrc_denied(&url("https://FILES.example/other")));
        assert!(!netrc_denied(&url("https://mirror.example/x")));
        // a global tool keeps its credentials on the same host
        let trusted = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(with_trusted_tool_scope(async {
                netrc_denied(&url("https://files.example/private.env"))
            }));
        assert!(!trusted);
        // and keeps them in the tasks that work spawns, but only through
        // `inherit_trusted_scope`
        let (inherited, plain, parallel, outside) = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .unwrap()
            .block_on(async {
                let target = url("https://files.example/private.env");
                let (inherited, plain, parallel) = with_trusted_tool_scope(async {
                    let t = target.clone();
                    let inherited =
                        tokio::spawn(inherit_trusted_scope(async move { netrc_denied(&t) }));
                    let t = target.clone();
                    let plain = tokio::spawn(async move { netrc_denied(&t) });
                    let parallel =
                        crate::parallel::parallel(vec![target.clone()], |t| async move {
                            Ok(netrc_denied(&t))
                        });
                    (
                        inherited.await.unwrap(),
                        plain.await.unwrap(),
                        parallel.await.unwrap(),
                    )
                })
                .await;
                let t = target.clone();
                let outside = tokio::spawn(inherit_trusted_scope(async move { netrc_denied(&t) }))
                    .await
                    .unwrap();
                (inherited, plain, parallel, outside)
            });
        assert!(!inherited);
        assert!(plain);
        assert_eq!(parallel, [false]);
        assert!(outside);

        // a project tool reached from a trusted tool's work is untrusted again
        let (nested, nested_templates, back) = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(with_tool_scope(true, async {
                let target = url("https://files.example/private.env");
                let (nested, templates) = with_tool_scope(false, async {
                    (netrc_denied(&target), crate::tera::in_trusted_templates())
                })
                .await;
                (nested, templates, netrc_denied(&target))
            }));
        assert!(nested);
        assert!(!nested_templates);
        assert!(!back);
        deny_netrc_for_url("https://{{ os() }}.evil.example/tool");
        assert!(netrc_denied(&url("https://linux.evil.example/tool")));
        assert!(netrc_denied(&url("https://mirror.example/x")));
        reset();
    }
}
