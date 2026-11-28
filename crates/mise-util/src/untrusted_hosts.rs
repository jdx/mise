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
//! netrc is withheld by the scope a request is made in. The work done for a
//! tool that project config declares runs inside [`with_tool_scope`] as
//! explicitly untrusted and gets no netrc credentials on any host, a netrc
//! `default` entry included: the hosts its options name are not all the hosts it
//! reaches, since the servers it asks name more of them, as a release does in
//! the link to its asset. The work done for a tool that global or system config
//! declares runs inside [`with_trusted_tool_scope`]: the operator chose that tool
//! and its hosts, so its requests keep their credentials even on a host a
//! project also names.
//!
//! A request made in no tool's scope falls back to the hosts config loading
//! records: every host a project tool's URL options name (or every host at all,
//! when one of them is a template) gets no netrc credentials. One request of a
//! project tool is left to those recorded hosts as well: the one an operator's
//! URL replacement sent to another host. Project settings do not load in safe
//! mode, so the operator chose that host, typically a mirror with a netrc login.
//!
//! The GitHub token that [`crate::http::host_auth_headers`] adds to any request
//! for a GitHub API or raw content URL is withheld from those recorded hosts
//! too, the public forges included, unless the request is a global tool's work.
//! It goes to the service it was issued for, but the project chose the URL and
//! gets the answer back (as versions, say), so the token would read for the
//! project whatever the operator can read.

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
    let host = parse_host(host);
    forge_token_denied_where(
        forge,
        |origin| matches!((origin, &host), (url::Origin::Tuple(_, h, _), Some(host)) if h == host),
    )
}

/// `host` as a URL holds it: lowercase, and an IPv6 address with or without
/// its brackets (`host_str` writes them, a host typed on the command line may
/// not) as the address itself.
fn parse_host(host: &str) -> Option<url::Host> {
    let bare = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    match bare.parse() {
        Ok(address) => Some(url::Host::Ipv6(address)),
        Err(_) => url::Host::parse(host).ok(),
    }
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
/// withholds netrc from every host. The template is looked for in the authority
/// the URL parser reads, the one the request goes to, where a `{` in userinfo
/// is percent-encoded.
pub fn deny_netrc_for_url(url: &str) {
    let host = url::Url::parse(url).ok().and_then(|url| {
        let authority = url[..url::Position::BeforePath].to_ascii_lowercase();
        let templated = authority.contains('{') || authority.contains("%7b");
        url.host_str()
            .filter(|_| !templated)
            .map(str::to_ascii_lowercase)
    });
    match host {
        Some(host) => insert(&NETRC_DENIED, host),
        None => NETRC_DENIED_ANY_HOST.store(true, Ordering::Relaxed),
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
/// for a tool that project config declares (no netrc on any host but one an
/// operator's URL replacement moves a request to), even when it is reached from
/// a trusted tool's work, such as one tool's dependency pulling in another.
pub async fn with_tool_scope<T>(trusted: bool, future: impl Future<Output = T>) -> T {
    let tool = Box::pin(TRUSTED_TOOL.scope(trusted, future));
    crate::tera::with_templates_trusted(trusted, tool).await
}

/// Wraps `future`, about to be spawned as a task of its own (`tokio::spawn`, a
/// `JoinSet`, [`crate::parallel::parallel`]), so that it runs in the scopes of
/// the task spawning it: task-locals do not pass to a spawned task, so without
/// this the work a global tool spreads over tasks would lose its netrc
/// credentials and its templates' `env`, and the work a project tool spreads
/// over tasks would get netrc credentials back on hosts no project option
/// names. A future spawned outside those scopes stays outside them.
pub fn inherit_trusted_scope<F: Future>(future: F) -> impl Future<Output = F::Output> {
    let tool = TRUSTED_TOOL.try_with(|trusted| *trusted).ok();
    let templates = crate::tera::in_trusted_templates();
    async move {
        match (tool, templates) {
            (Some(true), true) => with_tool_scope(true, future).await,
            (Some(true), false) => with_trusted_tool_scope(future).await,
            (Some(false), true) => {
                let future = crate::tera::with_trusted_templates(future);
                TRUSTED_TOOL.scope(false, future).await
            }
            (Some(false), false) => with_tool_scope(false, future).await,
            (None, true) => crate::tera::with_trusted_templates(future).await,
            (None, false) => future.await,
        }
    }
}

/// The tool scope this task runs in: `Some(true)` for a tool from global or
/// system config, `Some(false)` for one from project config, `None` outside any
/// tool's work. Which credentials a request carries depends on it, so a result
/// shared between requests (a download cached for the invocation) is keyed on it.
pub fn current_tool_scope() -> Option<bool> {
    TRUSTED_TOOL.try_with(|trusted| *trusted).ok()
}

/// Whether netrc credentials must not be sent with a request for `url`. The
/// work for a tool that global or system config declares keeps them. The work
/// for a tool that project config declares gets none, whatever the host, since
/// the servers it asks choose the hosts of its further requests. A request made
/// in no tool's scope gets none on a host project config named.
pub fn netrc_denied(url: &url::Url) -> bool {
    match TRUSTED_TOOL.try_with(|trusted| *trusted) {
        Ok(true) => false,
        Ok(false) => true,
        Err(_) => netrc_denied_for_host(url),
    }
}

/// [`netrc_denied`] for a request that an operator's URL replacement sent to
/// `url`, on another host than the one asked for. The operator chose that
/// host, so the work for a project tool keeps its netrc credentials there,
/// unless project config names the host itself.
pub fn netrc_denied_on_replacement_host(url: &url::Url) -> bool {
    if TRUSTED_TOOL.try_with(|trusted| *trusted).unwrap_or(false) {
        return false;
    }
    netrc_denied_for_host(url)
}

/// Whether a request for `url` goes without the GitHub token that
/// [`crate::http::host_auth_headers`] adds on the strength of the host alone:
/// true when project config named the host in a URL option (or named a
/// templated host), and the request is not the work of a tool from global or
/// system config. Forge backends ask `github::get_headers` for a repository's
/// own URLs themselves and keep the token on the public forges.
pub fn host_auth_denied(url: &url::Url) -> bool {
    netrc_denied_on_replacement_host(url)
}

fn netrc_denied_for_host(url: &url::Url) -> bool {
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
    fn an_ipv6_api_url_is_denied_by_its_address() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        reset();
        let api_url = "http://[::1]:8443/api/v3";
        deny_forge_token_for_url(api_url);
        let parsed = url::Url::parse(api_url).unwrap();
        assert!(forge_token_denied(
            Forge::GitHub,
            parsed.host_str().unwrap()
        ));
        assert!(forge_token_denied(Forge::GitHub, "[::1]"));
        assert!(forge_token_denied(Forge::GitHub, "::1"));
        assert!(forge_token_denied(Forge::GitHub, "0:0:0:0:0:0:0:1"));
        assert!(!forge_token_denied(Forge::GitHub, "::2"));
        assert!(!forge_token_denied(Forge::GitHub, "127.0.0.1"));
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

    #[test]
    fn a_project_tool_gets_no_netrc_on_a_host_only_a_server_names() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        reset();
        let url = |s: &str| url::Url::parse(s).unwrap();
        deny_netrc_for_url("https://forge.example/api/v4");
        // a release's asset link on a host no option of the tool names, asked
        // for in the tool's scope and in a task that work spawns
        let (unnamed, spawned) = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .unwrap()
            .block_on(with_tool_scope(false, async {
                let target = url("https://assets.example/tool-linux-x64");
                let t = target.clone();
                let spawned = tokio::spawn(inherit_trusted_scope(async move { netrc_denied(&t) }));
                (netrc_denied(&target), spawned.await.unwrap())
            }));
        assert!(unnamed);
        assert!(spawned);
        // the host an operator's URL replacement sends a request to keeps the
        // login, unless a project option names that host itself
        let (mirror, named) = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(with_tool_scope(false, async {
                (
                    netrc_denied_on_replacement_host(&url("https://mirror.example/x")),
                    netrc_denied_on_replacement_host(&url("https://forge.example/x")),
                )
            }));
        assert!(!mirror);
        assert!(named);
        // a global tool's work keeps the login on any host
        let trusted = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(with_trusted_tool_scope(async {
                netrc_denied(&url("https://assets.example/tool-linux-x64"))
            }));
        assert!(!trusted);
        reset();
    }

    #[test]
    fn a_template_host_the_url_parser_reads_withholds_netrc_from_every_host() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        let url = |s: &str| url::Url::parse(s).unwrap();
        for spelling in [
            "https:///v{version}.attacker.example/tool",
            "https:/{version}.attacker.example/x://y",
            "https:{version}.attacker.example/tool?x=://unused/",
            "https://{version}@attacker.example/tool",
        ] {
            reset();
            deny_netrc_for_url(spelling);
            assert!(
                netrc_denied(&url("https://v1.attacker.example/tool")),
                "{spelling}"
            );
            assert!(netrc_denied(&url("https://other.example/x")), "{spelling}");
        }
        // a literal host is recorded by itself
        reset();
        deny_netrc_for_url("https:///files.example/tool-{version}");
        assert!(netrc_denied(&url("https://files.example/tool-1")));
        assert!(!netrc_denied(&url("https://other.example/x")));
        reset();
    }
}
