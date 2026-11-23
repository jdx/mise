//! Hosts that safe mode keeps the operator's credentials away from.
//!
//! Safe mode loads project config without trust, and project config can name
//! the host a request goes to: a forge `api_url`, or a URL a tool downloads or
//! lists versions from.
//!
//! Forge tokens such as `GITHUB_TOKEN` or `MISE_GITHUB_ENTERPRISE_TOKEN` are not
//! scoped to a host, so config loading records the origins (scheme, host and
//! port) a project `api_url` names, and the token lookups skip them. A forge's
//! own public API over https keeps that forge's token.
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

/// Origins (scheme, host and port) a project `api_url` names, which get no forge
/// token.
static FORGE_TOKEN_DENIED: Mutex<Option<HashSet<url::Origin>>> = Mutex::new(None);
/// Origins a global `api_url` names. The operator chose them, so they keep their
/// token even when a project names the same origin.
static FORGE_TOKEN_ALLOWED: Mutex<Option<HashSet<url::Origin>>> = Mutex::new(None);
/// Hosts a project tool's URL options name, which get no netrc credentials
/// outside [`with_trusted_tool_scope`].
static NETRC_DENIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Set when a project URL names its host through a template, which renders
/// only at the request, so netrc is withheld from every host.
static NETRC_DENIED_ANY_HOST: AtomicBool = AtomicBool::new(false);

/// The forge whose token a lookup resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
    Forgejo,
}

impl Forge {
    /// The hosts of the forge's own public service. Its token sent there over
    /// https goes to the service it was issued for, so a project naming one of
    /// them is not a reason to withhold it.
    fn public_hosts(self) -> &'static [&'static str] {
        match self {
            Forge::GitHub => &["github.com", "api.github.com"],
            Forge::GitLab => &["gitlab.com"],
            Forge::Forgejo => &["codeberg.org"],
        }
    }

    /// Whether `origin` is this forge's own public API over https on the default
    /// port.
    fn is_own_public_origin(self, origin: &url::Origin) -> bool {
        match origin {
            url::Origin::Tuple(scheme, host, port) => {
                scheme == "https"
                    && *port == 443
                    && self.public_hosts().contains(&host.to_string().as_str())
            }
            url::Origin::Opaque(_) => false,
        }
    }
}

fn host_of(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    url.host_str().map(str::to_ascii_lowercase)
}

fn origin_of(url: &str) -> Option<url::Origin> {
    let origin = url::Url::parse(url).ok()?.origin();
    origin.is_tuple().then_some(origin)
}

fn insert<T: std::hash::Hash + Eq>(set: &Mutex<Option<HashSet<T>>>, item: T) {
    set.lock()
        .unwrap()
        .get_or_insert_with(HashSet::new)
        .insert(item);
}

fn contains(set: &Mutex<Option<HashSet<String>>>, host: &str) -> bool {
    set.lock()
        .unwrap()
        .as_ref()
        .is_some_and(|hosts| hosts.contains(&host.to_ascii_lowercase()))
}

/// Records the origin of an `api_url` that project config set in safe mode.
pub fn deny_forge_token_for_url(api_url: &str) {
    if let Some(origin) = origin_of(api_url) {
        insert(&FORGE_TOKEN_DENIED, origin);
    }
}

/// Records the origin of an `api_url` that global or system config set.
pub fn allow_forge_token_for_url(api_url: &str) {
    if let Some(origin) = origin_of(api_url) {
        insert(&FORGE_TOKEN_ALLOWED, origin);
    }
}

/// Whether `forge`'s token must not be sent with a request for `url`: a project
/// `api_url` named the request's origin, global config does not name it, and it
/// is not `forge`'s own public API over https.
pub fn forge_token_denied_at(forge: Forge, url: &url::Url) -> bool {
    forge_token_denied_where(forge, |origin| *origin == url.origin())
}

/// [`forge_token_denied_at`] for a lookup that knows only the host, such as
/// `mise token`: the token is withheld when any origin on `host` is denied.
pub fn forge_token_denied(forge: Forge, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    forge_token_denied_where(
        forge,
        |origin| matches!(origin, url::Origin::Tuple(_, h, _) if h.to_string() == host),
    )
}

fn forge_token_denied_where(forge: Forge, matches: impl Fn(&url::Origin) -> bool) -> bool {
    let denied = FORGE_TOKEN_DENIED.lock().unwrap();
    let Some(denied) = denied.as_ref() else {
        return false;
    };
    let allowed = FORGE_TOKEN_ALLOWED.lock().unwrap();
    denied.iter().any(|origin| {
        matches(origin)
            && !allowed.as_ref().is_some_and(|a| a.contains(origin))
            && !forge.is_own_public_origin(origin)
    })
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
    for set in [&FORGE_TOKEN_DENIED, &FORGE_TOKEN_ALLOWED] {
        *set.lock().unwrap() = None;
    }
    *NETRC_DENIED.lock().unwrap() = None;
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
        assert!(forge_token_denied(Forge::GitHub, "evil.example"));
        assert!(!forge_token_denied(Forge::GitHub, "api.github.com"));
        assert!(!forge_token_denied(Forge::GitHub, "ghe.corp.example"));
        assert!(!forge_token_denied(Forge::GitHub, "other.example"));
        reset();
    }

    #[test]
    fn a_public_forge_keeps_only_its_own_token_over_https() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        reset();
        let url = |s: &str| url::Url::parse(s).unwrap();
        deny_forge_token_for_url("https://gitlab.com/api/v3");
        deny_forge_token_for_url("https://codeberg.org/api/v4");
        deny_forge_token_for_url("http://codeberg.org/api/v1");
        deny_forge_token_for_url("https://api.github.com");

        // another forge's token does not go to a public forge a project names
        assert!(forge_token_denied_at(
            Forge::GitHub,
            &url("https://gitlab.com/api/v3/repos")
        ));
        assert!(forge_token_denied(Forge::GitHub, "gitlab.com"));
        assert!(forge_token_denied_at(
            Forge::GitLab,
            &url("https://codeberg.org/api/v4/projects")
        ));
        // nor a forge's own token over plain http
        assert!(forge_token_denied_at(
            Forge::Forgejo,
            &url("http://codeberg.org/api/v1/repos")
        ));
        // a forge's own public API over https keeps its token
        assert!(!forge_token_denied_at(
            Forge::Forgejo,
            &url("https://codeberg.org/api/v1/repos")
        ));
        assert!(!forge_token_denied_at(
            Forge::GitLab,
            &url("https://gitlab.com/api/v4/projects")
        ));
        assert!(!forge_token_denied_at(
            Forge::GitHub,
            &url("https://api.github.com/repos")
        ));
        assert!(!forge_token_denied(Forge::GitHub, "api.github.com"));
        reset();
    }

    #[test]
    fn a_global_api_url_keeps_its_token_only_at_its_own_origin() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        reset();
        let url = |s: &str| url::Url::parse(s).unwrap();
        allow_forge_token_for_url("https://git.corp.example/api/v4");
        deny_forge_token_for_url("http://git.corp.example:8406/api/v4");
        deny_forge_token_for_url("https://git.corp.example/api/v4");

        assert!(forge_token_denied_at(
            Forge::GitLab,
            &url("http://git.corp.example:8406/api/v4/projects")
        ));
        assert!(!forge_token_denied_at(
            Forge::GitLab,
            &url("https://git.corp.example/api/v4/projects")
        ));
        // a lookup by host alone withholds it while any origin there is denied
        assert!(forge_token_denied(Forge::GitLab, "git.corp.example"));
        reset();
    }

    #[test]
    fn forge_headers_carry_no_token_to_a_project_chosen_forge_origin() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        reset();
        let tokens = |host: &str| {
            Some(std::collections::HashMap::from([(
                host.to_string(),
                "operator-token".to_string(),
            )]))
        };
        deny_forge_token_for_url("https://codeberg.org/api/v4");
        deny_forge_token_for_url("http://codeberg.org/api/v1");

        *crate::gitlab::test_support::TOKENS_FILE_OVERRIDE
            .write()
            .unwrap() = tokens("codeberg.org");
        let gitlab = crate::gitlab::get_headers(
            "https://codeberg.org/api/v4/projects/acme%2Ftool/releases",
            "https://codeberg.org/api/v4",
        )
        .unwrap();
        *crate::gitlab::test_support::TOKENS_FILE_OVERRIDE
            .write()
            .unwrap() = None;

        *crate::forgejo::test_support::TOKENS_FILE_OVERRIDE
            .write()
            .unwrap() = tokens("codeberg.org");
        let forgejo = crate::forgejo::get_headers(
            "http://codeberg.org/api/v1/repos/acme/tool/releases",
            "http://codeberg.org/api/v1",
        )
        .unwrap();
        *crate::forgejo::test_support::TOKENS_FILE_OVERRIDE
            .write()
            .unwrap() = None;
        reset();

        assert!(
            !gitlab.contains_key(reqwest::header::AUTHORIZATION),
            "{gitlab:?}"
        );
        assert!(
            !forgejo.contains_key(reqwest::header::AUTHORIZATION),
            "{forgejo:?}"
        );
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
