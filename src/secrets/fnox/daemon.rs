//! Reads fnox's own daemon cache over its socket, through the thin `fnox-client` crate.
//!
//! mise never starts the daemon and never stores into it: a miss goes to the fnox CLI on the
//! user's terminal, which resolves and stores, and the next run hits. Only an interactive run
//! asks (CI and other non-interactive runs use the CLI with `--no-daemon`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use fnox_client::document::{EnvDocument, EnvScope, KeyRejection};
use fnox_client::{CallError, Client, EnvOutcome, EnvRequest, RuntimeEnv, SocketKey};

use super::{FnoxSource, resolved_from};
use crate::secrets::source::{Catalog, KeySelection, ResolveError, Resolved, SourceCx};

/// Above fnox-client's own 5s I/O timeout: a connect to a stopped daemon with a full backlog
/// has none, and the broker holds the memo lock meanwhile.
pub(super) const DAEMON_TIMEOUT: Duration = Duration::from_secs(10);

/// Everything a daemon call needs, owned: `spawn_blocking` wants `'static`, and the source,
/// the keys and the env are borrowed.
pub(super) struct DaemonCall {
    pub(super) root: PathBuf,
    pub(super) flags: fnox_client::CliFlags,
    /// Exactly the env the CLI fallback gets. fnox-client requires this, or every request
    /// misses.
    pub(super) env: Vec<(String, String)>,
    /// `None` is every key in scope
    pub(super) keys: Option<Vec<String>>,
}

impl DaemonCall {
    fn client(&self) -> Client {
        let get = |k: &str| {
            self.env
                .iter()
                .rev()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
        };
        Client::new(
            SocketKey::from_cli_env(&self.flags, &get),
            &RuntimeEnv::from_env(&get),
        )
    }

    /// One `resolve_env` round trip, and the socket it used. Blocks on std I/O.
    pub(super) fn run(&self) -> (EnvOutcome, PathBuf) {
        let client = self.client();
        let outcome = client.resolve_env(&EnvRequest {
            cwd: &self.root,
            config: std::path::Path::new("fnox.toml"),
            scope: EnvScope::Exec,
            keys: self.keys.as_deref(),
            env: &self.env,
        });
        (outcome, client.socket_path().to_path_buf())
    }
}

/// What mise does with the daemon's answer.
pub(super) enum Plan {
    /// Use it exactly like a CLI document.
    Hit(Box<EnvDocument>),
    /// The config changed since preflight: G1/G2.
    Rejected(KeyRejection),
    /// Run the fnox CLI.
    Cli(Why),
}

/// Why the CLI runs instead. The error kinds never carry values or wire text.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Why {
    /// some keys are not cached, or need a lease; fnox resolves them on the terminal
    Miss(usize),
    /// fnox's config, or `FNOX_DAEMON`, says no daemon
    Disabled,
    /// no daemon is running (fnox may start one itself if its config enables it)
    Absent,
    /// the daemon speaks another protocol
    VersionMismatch,
    /// the daemon accepted the connection but did not answer in time
    TimedOut,
    /// the socket is not owned by the current user
    PeerRejected,
    Unavailable(&'static str),
}

pub(super) fn plan(outcome: EnvOutcome) -> Plan {
    match outcome {
        EnvOutcome::Hit(doc) => Plan::Hit(Box::new(doc)),
        EnvOutcome::Miss { keys } => Plan::Cli(Why::Miss(keys.len())),
        EnvOutcome::Rejected(rejection) => Plan::Rejected(rejection),
        EnvOutcome::Disabled => Plan::Cli(Why::Disabled),
        EnvOutcome::Absent => Plan::Cli(Why::Absent),
        EnvOutcome::VersionMismatch { .. } => Plan::Cli(Why::VersionMismatch),
        EnvOutcome::Unavailable(CallError::PeerRejected(_)) => Plan::Cli(Why::PeerRejected),
        // how a socket read or write timeout surfaces
        EnvOutcome::Unavailable(CallError::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            Plan::Cli(Why::TimedOut)
        }
        EnvOutcome::Unavailable(e) => Plan::Cli(Why::Unavailable(error_kind(&e))),
        // `EnvOutcome` is non-exhaustive
        _ => Plan::Cli(Why::Unavailable("other")),
    }
}

/// The kind only: an error's text can carry paths or wire content.
fn error_kind(e: &CallError) -> &'static str {
    match e {
        CallError::SocketUnavailable { .. } => "socket unavailable",
        CallError::Io(_) => "io",
        CallError::EmptyResponse => "empty response",
        CallError::Oversize => "oversize reply",
        CallError::Decode(_) => "undecodable reply",
        CallError::Unsupported => "unsupported platform",
        CallError::Daemon(_) => "daemon error",
        CallError::Protocol(_) => "protocol violation",
        _ => "other",
    }
}

fn rejected(r: KeyRejection) -> ResolveError {
    ResolveError::Invalid {
        unknown: r.unknown,
        suggestions: r.suggestions.into_iter().collect(),
        not_injectable: r.not_injectable.into_iter().map(|n| n.key).collect(),
    }
}

impl FnoxSource {
    fn daemon_call(&self, keys: &KeySelection) -> DaemonCall {
        DaemonCall {
            root: self.id.root.clone(),
            flags: self.cli_flags(),
            env: self
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            keys: keys
                .names()
                .map(|names| names.iter().map(|k| k.as_str().to_string()).collect()),
        }
    }

    /// `call` is the blocking socket round trip; tests replace it. Nothing is sent unless fnox
    /// itself says it would use its daemon for this project and env (`catalog.cache`).
    pub(super) async fn cached_with(
        &self,
        cx: &SourceCx,
        keys: &KeySelection,
        catalog: &Catalog,
        limit: Duration,
        call: impl FnOnce(DaemonCall) -> (EnvOutcome, PathBuf) + Send + 'static,
    ) -> Result<Option<Resolved>, ResolveError> {
        if !cx.interactive || catalog.cache != Some(true) || self.daemon_is_stuck() {
            return Ok(None);
        }
        let request = self.daemon_call(keys);
        let (outcome, socket) = match off_runtime(limit, move || call(request)).await {
            Ok(done) => done,
            Err(Stuck::Elapsed) => {
                debug!("secrets: fnox daemon did not answer in time; using the fnox CLI");
                self.mark_daemon_stuck();
                return Ok(None);
            }
            Err(Stuck::Died) => {
                debug!("secrets: fnox daemon call did not finish; using the fnox CLI");
                self.mark_daemon_stuck();
                return Ok(None);
            }
        };
        match plan(outcome) {
            Plan::Hit(doc) => {
                let resolved = resolved_from(*doc, keys, catalog);
                debug!(
                    "secrets: fnox daemon hit ({} keys)",
                    resolved.set.len() + resolved.files.len()
                );
                Ok(Some(resolved))
            }
            Plan::Rejected(r) => Err(rejected(r)),
            Plan::Cli(why) => {
                match why {
                    Why::VersionMismatch => hint!(
                        "fnox_daemon_protocol",
                        "the fnox daemon speaks a different protocol than this mise; using the fnox CLI",
                        ""
                    ),
                    Why::PeerRejected => warn_once!(
                        "fnox daemon socket at {} is not owned by you; using the fnox CLI",
                        crate::file::display_path(&socket)
                    ),
                    Why::TimedOut => {
                        debug!("secrets: fnox daemon timed out; using the fnox CLI");
                        self.mark_daemon_stuck();
                    }
                    other => {
                        debug!("secrets: fnox daemon not used ({other:?}); using the fnox CLI")
                    }
                }
                Ok(None)
            }
        }
    }

    /// The `mise secrets ls` header line. Asks the daemon who it is only when fnox says the
    /// daemon is enabled; never starts one.
    pub(super) async fn daemon_line(&self, catalog: &Catalog, limit: Duration) -> Option<String> {
        if !fnox_client::platform_supported() {
            return None;
        }
        match catalog.cache {
            Some(false) => return Some("daemon: disabled".to_string()),
            None => {
                return Some(format!(
                    "daemon: not used (fnox {} does not report it)",
                    catalog.tool_version
                ));
            }
            Some(true) => {}
        }
        let call = self.daemon_call(&KeySelection::AllInScope);
        let probe = off_runtime(limit, move || {
            let client = call.client();
            (client.hello(), client.socket_path().to_path_buf())
        })
        .await;
        Some(match probe {
            Ok((hello, socket)) => status_line(hello, &socket),
            Err(_) => "daemon: not responding".to_string(),
        })
    }
}

enum Stuck {
    Elapsed,
    Died,
}

/// Runs a blocking socket call on its own thread, for at most `limit`. Not `spawn_blocking`: a
/// connect to a wedged daemon cannot be cancelled, and tokio's runtime waits for its blocking
/// tasks when it drops, so an abandoned call would delay mise's exit. A detached thread ends
/// with the process.
async fn off_runtime<T: Send + 'static>(
    limit: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Stuck> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("fnox-daemon".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .map_err(|_| Stuck::Died)?;
    match tokio::time::timeout(limit, rx).await {
        Ok(Ok(done)) => Ok(done),
        Ok(Err(_)) => Err(Stuck::Died),
        Err(_) => Err(Stuck::Elapsed),
    }
}

/// Never includes an error's text: it can carry paths or wire content.
fn status_line(hello: Result<fnox_client::HelloInfo, CallError>, socket: &Path) -> String {
    match hello {
        Ok(info) => format!("daemon: running (protocol {})", info.protocol),
        Err(CallError::PeerRejected(_)) => format!(
            "daemon: socket {} is not owned by you",
            crate::file::display_path(socket)
        ),
        Err(e) if e.is_socket_missing() => "daemon: not running".to_string(),
        Err(_) => "daemon: not responding".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env_diff::EnvMap;
    use crate::secrets::SecretName;
    use crate::secrets::source::{CatalogEntry, InjectMode, KeyKind, SourceId};
    use fnox_client::document::{NotInjectable, SecretValue};
    use indexmap::IndexMap;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn source(env: &[(&str, &str)]) -> FnoxSource {
        FnoxSource {
            id: SourceId {
                kind: "fnox",
                root: PathBuf::from("/p"),
                profile: Some("dev".to_string()),
            },
            bin: PathBuf::from("/bin/fnox"),
            daemon_stuck: std::sync::atomic::AtomicBool::new(false),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<EnvMap>(),
        }
    }

    fn catalog() -> Catalog {
        let mut entries = IndexMap::new();
        for (k, injectable) in [("DATABASE_URL", true), ("SIGNING_KEY", false)] {
            entries.insert(
                SecretName::new(k).unwrap(),
                CatalogEntry {
                    kind: KeyKind::Secret,
                    mode: Some(InjectMode::Exec),
                    as_file: false,
                    injectable,
                    description: None,
                },
            );
        }
        Catalog {
            entries,
            profile: vec![],
            dynamic_leases: vec![],
            tool_version: "1".into(),
            cache: Some(true),
        }
    }

    fn catalog_cache(cache: Option<bool>) -> Catalog {
        Catalog { cache, ..catalog() }
    }

    const LIMIT: Duration = Duration::from_secs(5);

    fn sel(keys: &[&str]) -> KeySelection {
        KeySelection::Keys(keys.iter().map(|k| SecretName::new(k).unwrap()).collect())
    }

    fn doc(set: &[(&str, &str)]) -> EnvDocument {
        EnvDocument::new(
            EnvScope::Exec,
            vec!["dev".into()],
            set.iter()
                .map(|(k, v)| (k.to_string(), SecretValue::new(v.to_string())))
                .collect(),
            IndexMap::new(),
            vec!["SCRUB".into()],
            vec![],
            vec![],
        )
    }

    fn interactive() -> SourceCx {
        SourceCx { interactive: true }
    }

    fn io_error() -> std::io::Error {
        std::io::Error::other("s3cr3t path")
    }

    #[test]
    fn plan_covers_every_outcome() {
        assert!(matches!(plan(EnvOutcome::Hit(doc(&[]))), Plan::Hit(_)));
        assert!(matches!(
            plan(EnvOutcome::Miss {
                keys: vec!["A".into(), "B".into()]
            }),
            Plan::Cli(Why::Miss(2))
        ));
        assert!(matches!(
            plan(EnvOutcome::Rejected(KeyRejection::default())),
            Plan::Rejected(_)
        ));
        assert!(matches!(
            plan(EnvOutcome::Disabled),
            Plan::Cli(Why::Disabled)
        ));
        assert!(matches!(plan(EnvOutcome::Absent), Plan::Cli(Why::Absent)));
        assert!(matches!(
            plan(EnvOutcome::VersionMismatch {
                min: None,
                max: None
            }),
            Plan::Cli(Why::VersionMismatch)
        ));
        assert!(matches!(
            plan(EnvOutcome::Unavailable(CallError::PeerRejected(io_error()))),
            Plan::Cli(Why::PeerRejected)
        ));
        assert!(matches!(
            plan(EnvOutcome::Unavailable(CallError::EmptyResponse)),
            Plan::Cli(Why::Unavailable("empty response"))
        ));
        // the kind never carries the error's own text
        let Plan::Cli(why) = plan(EnvOutcome::Unavailable(CallError::Io(io_error()))) else {
            panic!("expected the CLI");
        };
        assert!(!format!("{why:?}").contains("s3cr3t"));
    }

    #[test]
    fn the_request_follows_the_cli_flags_and_the_source_env() {
        let s = source(&[("AWS_PROFILE", "x")]);
        let call = s.daemon_call(&sel(&["B", "A"]));
        assert_eq!(call.flags.to_args(), ["-P", "dev"]);
        assert_eq!(call.root, PathBuf::from("/p"));
        assert_eq!(call.keys, Some(vec!["A".to_string(), "B".to_string()]));
        assert_eq!(call.env, [("AWS_PROFILE".to_string(), "x".to_string())]);
        assert!(s.daemon_call(&KeySelection::AllInScope).keys.is_none());
    }

    #[tokio::test]
    async fn non_interactive_never_builds_a_client() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let out = source(&[])
            .cached_with(
                &SourceCx { interactive: false },
                &sel(&["DATABASE_URL"]),
                &catalog(),
                LIMIT,
                move |_| {
                    seen.fetch_add(1, Ordering::SeqCst);
                    (EnvOutcome::Absent, PathBuf::new())
                },
            )
            .await;
        assert!(matches!(out, Ok(None)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    async fn cached(
        outcome: impl FnOnce() -> EnvOutcome + Send + 'static,
    ) -> Result<Option<Resolved>, ResolveError> {
        source(&[])
            .cached_with(
                &interactive(),
                &sel(&["DATABASE_URL"]),
                &catalog(),
                LIMIT,
                move |_| (outcome(), PathBuf::new()),
            )
            .await
    }

    #[tokio::test]
    async fn a_hit_is_filtered_like_a_cli_document_and_never_prints_values() {
        let r = cached(|| {
            EnvOutcome::Hit(doc(&[
                ("DATABASE_URL", "postgres://s3cr3t"),
                ("SIGNING_KEY", "s3cr3t-key"),
            ]))
        })
        .await
        .expect("ok")
        .expect("a hit is an answer");
        let set: Vec<_> = r.set.keys().map(|k| k.as_str()).collect();
        // SIGNING_KEY was not requested (G17)
        assert_eq!(set, ["DATABASE_URL"]);
        assert_eq!(r.unrequested, BTreeSet::from(["SIGNING_KEY".to_string()]));
        assert_eq!(r.remove, BTreeSet::from(["SCRUB".to_string()]));
        assert!(!format!("{r:?}").contains("s3cr3t"));
    }

    #[tokio::test]
    async fn everything_but_a_hit_falls_back_to_the_cli() {
        for outcome in [
            (|| EnvOutcome::Miss { keys: vec![] }) as fn() -> EnvOutcome,
            || EnvOutcome::Disabled,
            || EnvOutcome::Absent,
            || EnvOutcome::VersionMismatch {
                min: Some(1),
                max: Some(5),
            },
            || EnvOutcome::Unavailable(CallError::Oversize),
        ] {
            assert!(matches!(cached(outcome).await, Ok(None)));
        }
    }

    #[tokio::test]
    async fn a_rejection_is_g1_and_g2() {
        let rejection = KeyRejection {
            unknown: vec!["DEPLOY_KYE".into()],
            suggestions: IndexMap::from([("DEPLOY_KYE".to_string(), vec!["DEPLOY_KEY".into()])]),
            not_injectable: vec![NotInjectable {
                key: "SIGNING_KEY".into(),
                env: fnox_client::document::EnvMode::Never,
            }],
        };
        let err = source(&[])
            .cached_with(
                &interactive(),
                &sel(&["DEPLOY_KYE"]),
                &catalog(),
                LIMIT,
                move |_| (EnvOutcome::Rejected(rejection), PathBuf::new()),
            )
            .await
            .expect_err("a rejection is an error");
        match err {
            ResolveError::Invalid {
                unknown,
                suggestions,
                not_injectable,
            } => {
                assert_eq!(unknown, ["DEPLOY_KYE"]);
                assert_eq!(suggestions["DEPLOY_KYE"], ["DEPLOY_KEY"]);
                assert_eq!(not_injectable, ["SIGNING_KEY"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn nothing_is_sent_unless_fnox_says_the_daemon_is_enabled() {
        for cache in [None, Some(false)] {
            let called = Arc::new(AtomicUsize::new(0));
            let seen = called.clone();
            let out = source(&[])
                .cached_with(
                    &interactive(),
                    &sel(&["DATABASE_URL"]),
                    &catalog_cache(cache),
                    LIMIT,
                    move |_| {
                        seen.fetch_add(1, Ordering::SeqCst);
                        (EnvOutcome::Absent, PathBuf::new())
                    },
                )
                .await;
            assert!(matches!(out, Ok(None)), "{cache:?}");
            assert_eq!(called.load(Ordering::SeqCst), 0, "{cache:?}");
        }
        let called = Arc::new(AtomicUsize::new(0));
        let seen = called.clone();
        let _ = source(&[])
            .cached_with(
                &interactive(),
                &sel(&["DATABASE_URL"]),
                &catalog_cache(Some(true)),
                LIMIT,
                move |_| {
                    seen.fetch_add(1, Ordering::SeqCst);
                    (EnvOutcome::Absent, PathBuf::new())
                },
            )
            .await;
        assert_eq!(called.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_daemon_that_never_answers_is_abandoned() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let started = std::time::Instant::now();
        let out = source(&[])
            .cached_with(
                &interactive(),
                &sel(&["DATABASE_URL"]),
                &catalog(),
                Duration::from_millis(50),
                move |_| {
                    let _ = rx.recv();
                    (EnvOutcome::Absent, PathBuf::new())
                },
            )
            .await;
        assert!(matches!(out, Ok(None)));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(tx);
    }

    #[tokio::test]
    async fn a_timed_out_daemon_is_skipped_for_the_rest_of_the_run() {
        let s = source(&[]);
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let out = s
            .cached_with(
                &interactive(),
                &sel(&["DATABASE_URL"]),
                &catalog(),
                Duration::from_millis(50),
                move |_| {
                    let _ = rx.recv();
                    (EnvOutcome::Absent, PathBuf::new())
                },
            )
            .await;
        assert!(matches!(out, Ok(None)));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let out = s
            .cached_with(
                &interactive(),
                &sel(&["DATABASE_URL"]),
                &catalog(),
                LIMIT,
                move |_| {
                    seen.fetch_add(1, Ordering::SeqCst);
                    (EnvOutcome::Absent, PathBuf::new())
                },
            )
            .await;
        assert!(matches!(out, Ok(None)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(s.daemon_is_stuck());
        drop(tx);
    }

    #[tokio::test]
    async fn only_a_timeout_marks_the_daemon_stuck() {
        let marks = |outcome: fn() -> EnvOutcome| async move {
            let s = source(&[]);
            let out = s
                .cached_with(
                    &interactive(),
                    &sel(&["DATABASE_URL"]),
                    &catalog(),
                    LIMIT,
                    move |_| (outcome(), PathBuf::new()),
                )
                .await;
            assert!(matches!(out, Ok(None)));
            s.daemon_is_stuck()
        };
        assert!(
            marks(
                || EnvOutcome::Unavailable(CallError::Io(std::io::Error::from(
                    std::io::ErrorKind::TimedOut
                )))
            )
            .await
        );
        assert!(
            marks(
                || EnvOutcome::Unavailable(CallError::Io(std::io::Error::from(
                    std::io::ErrorKind::WouldBlock
                )))
            )
            .await
        );
        for outcome in [
            (|| EnvOutcome::Miss { keys: vec![] }) as fn() -> EnvOutcome,
            || EnvOutcome::Disabled,
            || EnvOutcome::Absent,
            || EnvOutcome::VersionMismatch {
                min: None,
                max: None,
            },
            || EnvOutcome::Unavailable(CallError::EmptyResponse),
            || EnvOutcome::Unavailable(CallError::PeerRejected(std::io::Error::other("x"))),
            || EnvOutcome::Unavailable(CallError::Io(std::io::Error::other("x"))),
        ] {
            assert!(!marks(outcome).await);
        }
    }

    #[test]
    fn status_lines_name_the_failure_without_its_text() {
        let socket = Path::new("/run/fnox/x.sock");
        let peer = status_line(
            Err(CallError::PeerRejected(std::io::Error::other("secret-x"))),
            socket,
        );
        assert!(peer.contains("x.sock"), "{peer}");
        assert!(peer.contains("not owned"), "{peer}");
        assert!(
            !peer.contains("not running") && !peer.contains("secret-x"),
            "{peer}"
        );
        let refused = CallError::SocketUnavailable {
            path: socket.to_path_buf(),
            source: std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
        };
        assert_eq!(status_line(Err(refused), socket), "daemon: not running");
        assert_eq!(
            status_line(Err(CallError::EmptyResponse), socket),
            "daemon: not responding"
        );
    }

    #[test]
    fn the_socket_tests_run_where_fnox_has_a_daemon() {
        assert_eq!(
            fnox_client::platform_supported(),
            cfg!(any(
                target_os = "linux",
                target_os = "macos",
                target_os = "freebsd",
                target_os = "openbsd"
            ))
        );
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd"
    ))]
    mod socket {

        use super::*;
        use fnox_client::wire::Response;
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;

        /// `XDG_RUNTIME_DIR` is a fresh temp dir, so the source's client finds only this
        /// listener, which answers one request.
        struct Daemon {
            _run: tempfile::TempDir,
            source: FnoxSource,
            requests: std::thread::JoinHandle<()>,
        }

        fn bind(source: &FnoxSource) -> UnixListener {
            let socket = source
                .daemon_call(&sel(&["DATABASE_URL"]))
                .client()
                .socket_path()
                .to_path_buf();
            std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
            std::fs::set_permissions(
                socket.parent().unwrap(),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
            UnixListener::bind(&socket).unwrap()
        }

        fn daemon(reply: Option<Response>, serve: bool) -> Daemon {
            let run = tempfile::tempdir().unwrap();
            let xdg = run.path().join("run");
            let source = source(&[("XDG_RUNTIME_DIR", xdg.to_str().unwrap())]);
            let requests = if serve {
                let listener = bind(&source);
                std::thread::spawn(move || {
                    let (stream, _) = listener.accept().unwrap();
                    let mut line = String::new();
                    BufReader::new(&stream).read_line(&mut line).unwrap();
                    if let Some(reply) = reply {
                        let mut stream = &stream;
                        writeln!(stream, "{}", serde_json::to_string(&reply).unwrap()).unwrap();
                    }
                })
            } else {
                std::thread::spawn(|| {})
            };
            Daemon {
                _run: run,
                source,
                requests,
            }
        }

        async fn ask(d: &Daemon) -> Result<Option<Resolved>, ResolveError> {
            d.source
                .cached_with(
                    &interactive(),
                    &sel(&["DATABASE_URL"]),
                    &catalog(),
                    LIMIT,
                    |call| call.run(),
                )
                .await
        }

        #[tokio::test]
        async fn a_hit_over_the_socket_yields_values() {
            let d = daemon(
                Some(Response::Env {
                    document: doc(&[("DATABASE_URL", "postgres://s3cr3t")]),
                }),
                true,
            );
            let r = ask(&d).await.expect("ok").expect("a hit");
            d.requests.join().unwrap();
            assert_eq!(r.set.len(), 1);
            assert_eq!(
                r.set[&SecretName::new("DATABASE_URL").unwrap()].expose(),
                "postgres://s3cr3t"
            );
            assert!(!format!("{r:?}").contains("s3cr3t"));
        }

        #[tokio::test]
        async fn miss_disabled_and_a_silent_close_use_the_cli() {
            for reply in [
                Some(Response::EnvMiss {
                    keys: vec!["DATABASE_URL".into()],
                }),
                Some(Response::Disabled),
                Some(Response::UnsupportedProtocol { min: 1, max: 5 }),
                None,
            ] {
                let d = daemon(reply, true);
                assert!(matches!(ask(&d).await, Ok(None)));
                d.requests.join().unwrap();
            }
        }

        #[tokio::test]
        async fn a_rejection_over_the_socket_is_an_error() {
            let d = daemon(
                Some(Response::EnvRejected(KeyRejection {
                    unknown: vec!["X".into()],
                    ..Default::default()
                })),
                true,
            );
            let err = d
                .source
                .cached_with(&interactive(), &sel(&["X"]), &catalog(), LIMIT, |call| {
                    call.run()
                })
                .await;
            d.requests.join().unwrap();
            assert!(matches!(err, Err(ResolveError::Invalid { .. })));
        }

        #[tokio::test]
        async fn no_socket_spawns_nothing_and_creates_no_runtime_dir() {
            let d = daemon(None, false);
            assert!(matches!(ask(&d).await, Ok(None)));
            let xdg = d._run.path().join("run");
            assert!(!xdg.exists(), "{}", xdg.display());
            assert_eq!(
                d.source.daemon_line(&catalog(), LIMIT).await.unwrap(),
                "daemon: not running"
            );
        }

        #[tokio::test]
        async fn status_line_names_the_protocol() {
            let d = daemon(
                Some(Response::Hello {
                    protocol: 6,
                    min_protocol: 6,
                    fnox_version: "1.39.0".into(),
                    pid: 1,
                }),
                true,
            );
            assert_eq!(
                d.source.daemon_line(&catalog(), LIMIT).await.unwrap(),
                "daemon: running (protocol 6)"
            );
            d.requests.join().unwrap();
        }

        #[tokio::test]
        async fn a_disabled_daemon_is_never_contacted() {
            let run = tempfile::tempdir().unwrap();
            let xdg = run.path().join("run");
            let source = source(&[("XDG_RUNTIME_DIR", xdg.to_str().unwrap())]);
            let listener = bind(&source);
            listener.set_nonblocking(true).unwrap();
            let disabled = catalog_cache(Some(false));
            assert_eq!(
                source.daemon_line(&disabled, LIMIT).await.unwrap(),
                "daemon: disabled"
            );
            let out = source
                .cached_with(
                    &interactive(),
                    &sel(&["DATABASE_URL"]),
                    &disabled,
                    LIMIT,
                    |call| call.run(),
                )
                .await;
            assert!(matches!(out, Ok(None)));
            assert!(
                matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
                "the daemon saw a connection"
            );
        }
    }
}
