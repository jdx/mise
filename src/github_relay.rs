//! Session-only GitHub access. Authorization is checked on the initiating machine;
//! neither the remote transport nor its callers can request a credential.
use crate::duration;
use eyre::{Result, bail};

#[derive(Clone, Debug, Default)]
pub(crate) struct Scope {
    #[cfg(unix)]
    options: Options,
    #[cfg(any(unix, test))]
    repositories: std::collections::BTreeSet<String>,
    #[cfg(any(unix, test))]
    all: bool,
}

impl Scope {
    pub(crate) fn from_flags(
        enabled: bool,
        repositories: &[String],
        all: bool,
    ) -> Result<Option<Self>> {
        if !enabled {
            if all || !repositories.is_empty() {
                bail!("repository scope requires --github-relay-read-only");
            }
            return Ok(None);
        }
        if all != repositories.is_empty() {
            bail!("choose --github-relay-repo OWNER/REPO or --github-relay-all-repos, not both");
        }
        let repositories: std::collections::BTreeSet<String> = repositories
            .iter()
            .map(|repo| repository(repo))
            .collect::<Result<_>>()?;
        #[cfg(not(any(unix, test)))]
        let _ = repositories;
        Ok(Some(Self {
            #[cfg(unix)]
            options: Options::default(),
            #[cfg(any(unix, test))]
            repositories,
            #[cfg(any(unix, test))]
            all,
        }))
    }

    #[cfg(any(unix, test))]
    fn permits(&self, repo: &str) -> bool {
        self.all || self.repositories.contains(&repo.to_ascii_lowercase())
    }
}

#[derive(Clone, Debug)]
#[cfg(unix)]
struct Options {
    log_requests: bool,
    jsonl: bool,
    max_duration: std::time::Duration,
    request_timeout: std::time::Duration,
    concurrency: usize,
}

#[cfg(unix)]
impl Default for Options {
    fn default() -> Self {
        Self {
            log_requests: false,
            jsonl: false,
            max_duration: std::time::Duration::ZERO,
            request_timeout: std::time::Duration::from_secs(300),
            concurrency: 8,
        }
    }
}

/// Resolve observability and limits only after explicit access authorization.
pub(crate) fn configure(
    scope: Option<Scope>,
    log_requests: bool,
    no_log_requests: bool,
    format: Option<&str>,
    max_duration: Option<&str>,
) -> Result<Option<Scope>> {
    let Some(scope) = scope else {
        if log_requests || no_log_requests || format.is_some() || max_duration.is_some() {
            bail!("relay options require --github-relay-read-only");
        }
        return Ok(None);
    };
    let settings = crate::config::Settings::get();
    let settings = &settings.github_relay;
    let format = format.unwrap_or(&settings.log_format);
    if !matches!(format, "text" | "jsonl") {
        bail!("relay log format must be text or jsonl");
    }
    let timeout = duration::parse_duration(&settings.request_timeout)?;
    if timeout.is_zero() {
        bail!("relay request timeout must be greater than zero");
    }
    if !(1..=32).contains(&settings.concurrency) {
        bail!("relay concurrency must be between 1 and 32");
    }
    let max_duration = duration::parse_duration(max_duration.unwrap_or(&settings.max_duration))?;
    if std::time::Instant::now().checked_add(timeout).is_none()
        || std::time::Instant::now()
            .checked_add(max_duration)
            .is_none()
    {
        bail!("relay duration is too large");
    }
    #[cfg(not(unix))]
    let _ = max_duration;
    #[cfg(unix)]
    let scope = {
        let mut scope = scope;
        scope.options = Options {
            log_requests: !no_log_requests && (log_requests || settings.log_requests),
            jsonl: format == "jsonl",
            max_duration,
            request_timeout: timeout,
            concurrency: settings.concurrency as usize,
        };
        scope
    };
    Ok(Some(scope))
}

fn repository(value: &str) -> Result<String> {
    let parts: Vec<_> = value.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|s| {
            s.is_empty()
                || *s == "."
                || *s == ".."
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        bail!("expected a GitHub repository in OWNER/REPO form");
    }
    Ok(value.to_ascii_lowercase())
}

/// Expand only unambiguous shorthand, preserving paths and explicit transports.
pub(crate) fn expand_repository(value: &str) -> Result<String> {
    if value.contains(':')
        || value.starts_with(['/', '.', '~'])
        || std::path::Path::new(value).exists()
    {
        return Ok(value.to_string());
    }
    repository(value)?;
    Ok(format!(
        "https://github.com/{}.git",
        value.strip_suffix(".git").unwrap_or(value)
    ))
}

#[derive(Debug, PartialEq)]
#[cfg(any(unix, test))]
struct Target {
    url: String,
    git: bool,
    archive_repo: Option<String>,
}

#[cfg(any(unix, test))]
fn validate_path(path: &str) -> Result<()> {
    // Validate decoded segments, retaining the original spelling upstream. Reject
    // encoded separators and percent signs so a second decoder cannot change scope.
    for segment in path.split('/') {
        for (index, byte) in segment.bytes().enumerate() {
            if byte == b'%'
                && !segment
                    .as_bytes()
                    .get(index + 1..index + 3)
                    .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
            {
                bail!("invalid relay path encoding");
            }
        }
        let decoded = urlencoding::decode(segment)?;
        if decoded.is_empty()
            || matches!(decoded.as_ref(), "." | "..")
            || decoded.contains(['/', '\\', '%'])
            || decoded.chars().any(char::is_control)
        {
            bail!("invalid relay path");
        }
    }
    Ok(())
}

#[cfg(any(unix, test))]
fn authorize(scope: &Scope, method: &str, path: &str, query: Option<&str>) -> Result<Target> {
    validate_path(path)?;
    let p: Vec<_> = path.split('/').collect();
    let (owner, repo) = match p.as_slice() {
        ["api", "repos", owner, repo, ..] => (*owner, *repo),
        ["git" | "web", owner, repo, ..] => (*owner, repo.strip_suffix(".git").unwrap_or(repo)),
        _ => bail!("unsupported GitHub operation"),
    };
    let name = repository(&format!("{owner}/{repo}"))?;
    if !scope.permits(&name) {
        bail!("repository is outside the approved relay scope");
    }
    let git = p[0] == "git";
    let allowed = match p.as_slice() {
        ["git", _, _, "info", "refs"] => {
            method == "GET" && query == Some("service=git-upload-pack")
        }
        ["git", _, _, "git-upload-pack"] => method == "POST" && query.is_none(),
        ["api", "repos", _, _] => method == "GET" || method == "HEAD",
        ["api", "repos", _, _, "git", kind, ..] => {
            matches!(*kind, "refs" | "matching-refs") && matches!(method, "GET" | "HEAD")
        }
        ["api", "repos", _, _, kind, ..] => {
            matches!(
                *kind,
                "contents" | "releases" | "tags" | "branches" | "tarball" | "zipball"
            ) && matches!(method, "GET" | "HEAD")
        }
        ["web", _, _, "releases", "download", _, ..] => matches!(method, "GET" | "HEAD"),
        ["web", _, _, "archive", _, ..] => matches!(method, "GET" | "HEAD"),
        _ => false,
    };
    if !allowed {
        bail!("GitHub relay permits read-only repository operations only");
    }
    if !git && let Some(query) = query {
        for (key, _) in url::form_urlencoded::parse(query.as_bytes()) {
            if !matches!(key.as_ref(), "ref" | "page" | "per_page") {
                bail!("unsupported query parameter");
            }
        }
    }
    let host = if p[0] == "api" {
        "api.github.com"
    } else {
        "github.com"
    };
    let suffix = path.split_once('/').expect("validated path").1;
    let mut url = format!("https://{host}/{suffix}");
    let archive_repo = match p.as_slice() {
        ["api", "repos", _, _, "tarball" | "zipball", ..] => Some(name.clone()),
        ["web", _, _, "archive", rest @ ..] => {
            let reference = rest.join("/");
            let (kind, reference) = if let Some(reference) = reference.strip_suffix(".tar.gz") {
                ("tarball", reference)
            } else if let Some(reference) = reference.strip_suffix(".zip") {
                ("zipball", reference)
            } else {
                bail!("unsupported archive format");
            };
            if reference.is_empty() {
                bail!("missing archive reference");
            }
            // The API supplies short-lived private archive links; credentials
            // remain at the API origin and are never attached to that redirect.
            url = format!("https://api.github.com/repos/{name}/{kind}/{reference}");
            Some(name)
        }
        _ => None,
    };
    if let Some(query) = query {
        url.push('?');
        url.push_str(query);
    }
    Ok(Target {
        url,
        git,
        archive_repo,
    })
}

#[cfg(unix)]
pub(crate) mod unix {
    use super::*;
    use axum::{
        Router,
        body::{Body, to_bytes},
        extract::{Request, State},
        response::Response,
    };
    use reqwest::{Client, Method, Url};
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };
    use tokio::{net::UnixListener, sync::Semaphore, task::JoinHandle};
    use tokio_util::sync::CancellationToken;

    // Bound accepted connections too, including clients that never send headers.
    struct BoundedListener<L> {
        inner: L,
        permits: Arc<Semaphore>,
    }
    impl<L> BoundedListener<L> {
        fn new(inner: L) -> Self {
            Self {
                inner,
                permits: Arc::new(Semaphore::new(32)),
            }
        }
    }
    struct Connection<T> {
        inner: T,
        _permit: tokio::sync::OwnedSemaphorePermit,
    }
    impl<T: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Connection<T> {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }
    impl<T: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Connection<T> {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
        }
        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }
    impl<L: axum::serve::Listener> axum::serve::Listener for BoundedListener<L> {
        type Io = Connection<L::Io>;
        type Addr = L::Addr;
        async fn accept(&mut self) -> (Self::Io, Self::Addr) {
            let permit = self
                .permits
                .clone()
                .acquire_owned()
                .await
                .expect("listener semaphore stays open");
            let (inner, addr) = self.inner.accept().await;
            (
                Connection {
                    inner,
                    _permit: permit,
                },
                addr,
            )
        }
        fn local_addr(&self) -> std::io::Result<Self::Addr> {
            self.inner.local_addr()
        }
    }

    /// Session-local audit data; never stores credentials, URLs or response bodies.
    struct Audit {
        destination: String,
        options: Options,
        requests: std::sync::atomic::AtomicU64,
        denied: std::sync::atomic::AtomicU64,
        bytes: std::sync::atomic::AtomicU64,
        repositories: std::sync::Mutex<std::collections::BTreeSet<String>>,
    }

    impl Audit {
        fn new(destination: &str, options: Options) -> Self {
            Self {
                destination: destination.to_string(),
                options,
                requests: Default::default(),
                denied: Default::default(),
                bytes: Default::default(),
                repositories: Default::default(),
            }
        }

        fn operation(&self, scope: &Scope, request: &Request) -> String {
            let method = match request.method().as_str() {
                "GET" => "GET",
                "HEAD" => "HEAD",
                "POST" => "POST",
                _ => "OTHER",
            };
            let Ok(target) = authorize(
                scope,
                request.method().as_str(),
                request.uri().path().strip_prefix('/').unwrap_or_default(),
                request.uri().query(),
            ) else {
                return format!("{method} unapproved operation");
            };
            let url = Url::parse(&target.url).expect("authorized URL");
            let segments: Vec<_> = url.path().trim_start_matches('/').split('/').collect();
            let offset = usize::from(url.host_str() == Some("api.github.com"));
            let repository = format!(
                "{}/{}",
                segments[offset],
                segments[offset + 1].trim_end_matches(".git")
            );
            let mut repositories = self.repositories.lock().unwrap();
            if repositories.len() < 128 {
                repositories.insert(repository.clone());
            }
            // Only fixed route names are shown. Refs, filenames, and URL query
            // values can contain private data unrelated to debugging transport.
            let route = segments.get(offset + 2).copied().unwrap_or("metadata");
            format!("{method} {repository}/{route}")
        }

        fn line(&self, mut event: serde_json::Value) -> String {
            event["destination"] = self.destination.clone().into();
            event["timestamp_ms"] = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .to_string()
                .into();
            if self.options.jsonl {
                event.to_string()
            } else {
                let label = serde_json::to_string(&self.destination).unwrap();
                match event["event"].as_str() {
                    Some("request") => format!(
                        "github-relay [{label}] {} → {} (headers: {}ms)",
                        event["operation"].as_str().unwrap_or_default(),
                        event["status"],
                        event["headers_ms"]
                    ),
                    Some("expired") => format!("github-relay [{label}] borrowed access expired"),
                    Some("summary") => format!(
                        "github-relay [{label}] {} requests, {} denied/unavailable, {} bytes, repositories: {}",
                        event["requests"],
                        event["denied_or_unavailable"],
                        event["bytes"],
                        event["repositories"]
                    ),
                    _ => format!("github-relay [{label}] {event}"),
                }
            }
        }

        fn emit(&self, event: serde_json::Value) {
            use std::io::Write;
            let line = self.line(event);
            // A closed stderr must never panic or affect the remote command.
            let _ = writeln!(std::io::stderr().lock(), "{line}");
        }

        fn summary(&self) {
            use std::sync::atomic::Ordering::Relaxed;
            self.emit(serde_json::json!({"event": "summary", "requests": self.requests.load(Relaxed), "denied_or_unavailable": self.denied.load(Relaxed), "bytes": self.bytes.load(Relaxed), "repositories": *self.repositories.lock().unwrap()}));
        }
    }

    #[derive(Clone)]
    struct Broker {
        audit: Arc<Audit>,
        scope: Scope,
        client: Client,
        permits: Arc<Semaphore>,
        cancel: CancellationToken,
        token: Arc<String>,
        #[cfg(test)]
        test_upstream: Option<String>,
    }

    pub(crate) struct Relay {
        audit: Arc<Audit>,
        expiry: Option<AbortTask>,
        task: JoinHandle<()>,
        directory: tempfile::TempDir,
        cancel: CancellationToken,
    }
    impl Relay {
        pub(crate) fn socket(&self) -> PathBuf {
            self.directory.path().join("relay.sock")
        }
        pub(crate) async fn start(scope: Scope, destination: &str) -> Result<Self> {
            let (token, _) = crate::github::resolve_token("github.com").ok_or_else(|| {
                eyre::eyre!(
                    "no local GitHub credential found; sign in locally with `mise token github`"
                )
            })?;
            // Unix socket names have a small platform limit; TMPDIR can itself
            // exceed it (notably macOS and isolated test environments).
            let directory = tempfile::Builder::new()
                .prefix("mise-relay-")
                .tempdir_in("/tmp")?;
            let listener = UnixListener::bind(directory.path().join("relay.sock"))?;
            let cancel = CancellationToken::new();
            let audit = Arc::new(Audit::new(destination, scope.options.clone()));
            let expiry = expiry_task(cancel.clone(), audit.clone());
            let broker = Broker {
                audit: audit.clone(),
                client: Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(scope.options.request_timeout)
                    .connect_timeout(Duration::from_secs(15))
                    .user_agent("mise-github-relay")
                    .build()?,
                permits: Arc::new(Semaphore::new(scope.options.concurrency)),
                scope,
                cancel: cancel.clone(),
                token: Arc::new(token),
                #[cfg(test)]
                test_upstream: None,
            };
            let task = tokio::spawn(async move {
                let _ = axum::serve(
                    BoundedListener::new(listener),
                    Router::new().fallback(handle).with_state(broker),
                )
                .await;
            });
            Ok(Self {
                audit,
                expiry,
                task,
                directory,
                cancel,
            })
        }
    }
    impl Drop for Relay {
        fn drop(&mut self) {
            self.cancel.cancel();
            self.task.abort();
            self.expiry.take();
            self.audit.summary();
        }
    }

    fn expiry_task(cancel: CancellationToken, audit: Arc<Audit>) -> Option<AbortTask> {
        if audit.options.max_duration.is_zero() {
            return None;
        }
        Some(AbortTask(tokio::spawn(async move {
            tokio::time::sleep(audit.options.max_duration).await;
            cancel.cancel();
            audit.emit(serde_json::json!({"event": "expired"}));
        })))
    }

    async fn handle(State(broker): State<Broker>, request: Request) -> Response {
        if request.method() == Method::GET && request.uri().path() == "/_session" {
            return Response::builder()
                .status(if broker.cancel.is_cancelled() {
                    403
                } else {
                    204
                })
                .body(Body::empty())
                .expect("valid response");
        }
        let started = std::time::Instant::now();
        let audit = broker.audit.clone();
        let operation = audit.operation(&broker.scope, &request);
        audit
            .requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let cancel = broker.cancel.clone();
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(eyre::eyre!("session ended")),
            result = tokio::time::timeout(audit.options.request_timeout, forward(broker, request)) => result.unwrap_or_else(|_| Err(eyre::eyre!("request timeout"))),
        };
        let status = result.as_ref().map(|r| r.status().as_u16()).unwrap_or(403);
        if status >= 400 {
            audit
                .denied
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if audit.options.log_requests && result.is_err() {
            audit.emit(serde_json::json!({"event": "request", "operation": operation, "status": status, "headers_ms": started.elapsed().as_millis()}));
        }
        match result {
            Ok(response) => response,
            // Deliberately never serialize upstream errors, URLs or credentials.
            Err(_) => Response::builder()
                .status(403)
                .body(Body::from("GitHub relay request denied or unavailable"))
                .expect("valid response"),
        }
    }

    async fn forward(broker: Broker, request: Request) -> Result<Response> {
        let deadline = tokio::time::Instant::now() + broker.audit.options.request_timeout;
        let operation = broker.audit.operation(&broker.scope, &request);
        let permit = broker.permits.clone().try_acquire_owned()?;
        let target = authorize(
            &broker.scope,
            request.method().as_str(),
            request.uri().path().strip_prefix('/').unwrap_or_default(),
            request.uri().query(),
        )?;
        let (parts, body) = request.into_parts();
        let body = to_bytes(body, 8 * 1024 * 1024).await?;
        if parts.method != Method::POST && !body.is_empty() {
            bail!("unexpected request body");
        }
        let upstream = target.url.clone();
        #[cfg(test)]
        let upstream = if let Some(base) = &broker.test_upstream {
            let url = Url::parse(&upstream)?;
            format!(
                "{base}{}{}",
                url.path(),
                url.query().map(|q| format!("?{q}")).unwrap_or_default()
            )
        } else {
            upstream
        };
        let mut req = broker.client.request(parts.method.clone(), upstream);
        if target.git {
            req = req.basic_auth("x-access-token", Some(broker.token.as_str()));
        } else {
            req = req.bearer_auth(broker.token.as_str());
        }
        for name in [
            "accept",
            "range",
            "if-range",
            "git-protocol",
            "content-encoding",
        ] {
            if let Some(value) = parts.headers.get(name) {
                req = req.header(name, value);
            }
        }
        if parts.method == Method::POST {
            req = req.header("content-type", "application/x-git-upload-pack-request");
        }
        let sent_at = std::time::Instant::now();
        let mut response = req.body(body).send().await?;
        if broker.audit.options.log_requests {
            broker.audit.emit(serde_json::json!({"event": "request", "operation": operation, "status": response.status().as_u16(), "headers_ms": sent_at.elapsed().as_millis()}));
        }
        // Only asset redirects are followed. Preserve resume headers, never credentials.
        for _ in 0..3 {
            if !response.status().is_redirection() {
                break;
            }
            let location = response
                .headers()
                .get("location")
                .ok_or_else(|| eyre::eyre!("missing redirect"))?
                .to_str()?;
            let url = Url::parse(location)?;
            if target.git
                || !(asset_redirect(&url) || archive_redirect(&url, target.archive_repo.as_deref()))
            {
                bail!("unsupported redirect");
            }
            let redirect_host = url.host_str().unwrap_or_default().to_string();
            let redirected_at = std::time::Instant::now();
            let mut redirected = broker.client.request(parts.method.clone(), url);
            for name in ["range", "if-range"] {
                if let Some(value) = parts.headers.get(name) {
                    redirected = redirected.header(name, value);
                }
            }
            response = redirected.send().await?;
            if broker.audit.options.log_requests {
                broker.audit.emit(serde_json::json!({"event": "request", "operation": format!("{} {redirect_host}/<download>", parts.method), "status": response.status().as_u16(), "headers_ms": redirected_at.elapsed().as_millis()}));
            }
        }
        if response.status().is_redirection() {
            bail!("too many redirects");
        }
        let mut builder = Response::builder().status(response.status());
        for name in [
            "content-type",
            "content-length",
            "content-range",
            "etag",
            "last-modified",
            "link",
        ] {
            if let Some(value) = response.headers().get(name) {
                builder = builder.header(name, value);
            }
        }
        let stream = futures_util::stream::try_unfold(
            (response, permit, broker.cancel, broker.audit, deadline),
            |(mut response, permit, cancel, audit, deadline)| async move {
                let chunk = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(std::io::Error::other("session ended")),
                    result = tokio::time::timeout_at(deadline, response.chunk()) => result.map_err(|_| std::io::Error::other("relay transfer timeout"))?.map_err(|_| std::io::Error::other("relay transfer failed"))?,
                };
                if let Some(chunk) = &chunk {
                    audit
                        .bytes
                        .fetch_add(chunk.len() as u64, std::sync::atomic::Ordering::Relaxed);
                }
                Ok(chunk.map(|chunk| (chunk, (response, permit, cancel, audit, deadline))))
            },
        );
        Ok(builder.body(Body::from_stream(stream))?)
    }

    fn archive_redirect(url: &Url, repository: Option<&str>) -> bool {
        let Some(repository) = repository else {
            return false;
        };
        let parts: Vec<_> = url.path().trim_start_matches('/').split('/').collect();
        safe_redirect_origin(url)
            && url.host_str() == Some("codeload.github.com")
            && parts.len() >= 4
            && format!("{}/{}", parts[0], parts[1]).eq_ignore_ascii_case(repository)
            && matches!(parts[2], "tar.gz" | "zip" | "legacy.tar.gz" | "legacy.zip")
            && validate_path(url.path().trim_start_matches('/')).is_ok()
    }

    fn safe_redirect_origin(url: &Url) -> bool {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.fragment().is_none()
    }

    fn asset_redirect(url: &Url) -> bool {
        safe_redirect_origin(url)
            && matches!(
                url.host_str(),
                Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com")
            )
    }

    /// Git only speaks TCP HTTP. A loopback bridge with a per-session capability
    /// adapts it to the private socket; the broker remains the authority.
    pub(crate) async fn session(socket: &Path, command: Vec<String>) -> Result<()> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let capability = rand::random::<[u8; 32]>()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let prefix = format!("/{capability}/");
        let client = Client::builder()
            .unix_socket(socket)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .read_timeout(Duration::from_secs(300))
            .build()?;
        let permits = Arc::new(Semaphore::new(8));
        let service = Router::new().fallback(move |request: Request| {
            let client = client.clone();
            let prefix = prefix.clone();
            let permits = permits.clone();
            async move {
                let result: Result<Response> = async {
                    let permit = permits.try_acquire_owned()?;
                    let path = request
                        .uri()
                        .path_and_query()
                        .ok_or_else(|| eyre::eyre!("missing path"))?
                        .as_str();
                    let path = path
                        .strip_prefix(&prefix)
                        .filter(|p| p.starts_with("git/"))
                        .ok_or_else(|| eyre::eyre!("invalid capability"))?;
                    let url = format!("http://localhost/{path}");
                    let (parts, body) = request.into_parts();
                    let body = tokio::time::timeout(
                        Duration::from_secs(300),
                        to_bytes(body, 8 * 1024 * 1024),
                    )
                    .await??;
                    let mut req = client.request(parts.method, url);
                    for name in ["accept", "content-type", "git-protocol", "content-encoding"] {
                        if let Some(value) = parts.headers.get(name) {
                            req = req.header(name, value);
                        }
                    }
                    let response = req.body(body).send().await?;
                    let mut builder = Response::builder().status(response.status());
                    for name in ["content-type", "content-length"] {
                        if let Some(value) = response.headers().get(name) {
                            builder = builder.header(name, value);
                        }
                    }
                    let stream = futures_util::stream::try_unfold(
                        (response, permit),
                        |(mut response, permit)| async move {
                            let chunk = response
                                .chunk()
                                .await
                                .map_err(|_| std::io::Error::other("relay disconnected"))?;
                            Ok::<_, std::io::Error>(chunk.map(|chunk| (chunk, (response, permit))))
                        },
                    );
                    Ok(builder.body(Body::from_stream(stream))?)
                }
                .await;
                result.unwrap_or_else(|_| {
                    Response::builder()
                        .status(403)
                        .body(Body::from("relay unavailable"))
                        .expect("valid response")
                })
            }
        });
        let task = tokio::spawn(async move {
            let _ = axum::serve(BoundedListener::new(listener), service).await;
        });
        // Aborting this task is sufficient here: exiting the adapter also closes
        // all accepted loopback connections. Local broker cancellation is separate.
        let guard = AbortTask(task);
        let mut child = if command.is_empty() {
            let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
            let mut child = tokio::process::Command::new(shell);
            child.arg("-l");
            child
        } else {
            let mut child = tokio::process::Command::new(&command[0]);
            child.args(&command[1..]);
            child
        };
        let executable = std::env::current_exe()?;
        let mut paths = vec![
            executable
                .parent()
                .ok_or_else(|| eyre::eyre!("missing mise executable directory"))?
                .to_path_buf(),
        ];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        child.env("PATH", std::env::join_paths(paths)?);
        let count: usize = std::env::var("GIT_CONFIG_COUNT")
            .unwrap_or_else(|_| "0".into())
            .parse()?;
        if count > 1000 {
            bail!("too many inherited Git configuration entries");
        }
        let base = format!("http://{address}/{capability}/git/");
        for (index, source) in [
            "https://github.com/",
            "git@github.com:",
            "ssh://git@github.com/",
        ]
        .iter()
        .enumerate()
        {
            child.env(
                format!("GIT_CONFIG_KEY_{}", count + index),
                format!("url.{base}.insteadOf"),
            );
            child.env(format!("GIT_CONFIG_VALUE_{}", count + index), source);
        }
        child
            .env("GIT_CONFIG_COUNT", (count + 3).to_string())
            .env("MISE_GITHUB_RELAY_SOCKET", socket)
            .kill_on_drop(true);
        let status = wait_command(&mut child, Some(socket)).await?;
        drop(guard);
        Err(crate::request_exit(status.code().unwrap_or(255)))
    }

    struct AbortTask(JoinHandle<()>);
    impl Drop for AbortTask {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    /// Handle TERM/HUP as well as the CLI's existing Ctrl-C cancellation. A
    /// heartbeat also closes remote commands when a non-TTY SSH connection dies.
    pub(crate) async fn wait_command(
        command: &mut tokio::process::Command,
        socket: Option<&Path>,
    ) -> Result<std::process::ExitStatus> {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
        let heartbeat = if let Some(socket) = socket {
            let client = Client::builder()
                .unix_socket(socket)
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()?;
            if !client
                .get("http://localhost/_session")
                .send()
                .await
                .is_ok_and(|r| r.status() == 204)
            {
                bail!("GitHub relay is not connected");
            }
            Some(client)
        } else {
            None
        };
        let disconnected = async {
            let Some(client) = heartbeat else {
                std::future::pending::<()>().await;
                return;
            };
            let mut failures = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if client
                    .get("http://localhost/_session")
                    .send()
                    .await
                    .is_ok_and(|r| r.status() == 204)
                {
                    failures = 0;
                } else {
                    failures += 1;
                    if failures >= 3 {
                        return;
                    }
                }
            }
        };
        let mut child = command.kill_on_drop(true).spawn()?;
        let code = tokio::select! {
            status = child.wait() => return Ok(status?),
            _ = terminate.recv() => 143,
            _ = hangup.recv() => 129,
            _ = disconnected => 255,
        };
        child.kill().await?;
        Err(crate::request_exit(code))
    }

    pub(crate) async fn lifecycle<T>(
        operation: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
        if crate::ui::ctrlc::is_cancelled() {
            return Err(crate::request_exit(130));
        }
        tokio::select! {
            result = operation => result,
            _ = interrupt.recv() => Err(crate::request_exit(130)),
            _ = terminate.recv() => Err(crate::request_exit(143)),
            _ = hangup.recv() => Err(crate::request_exit(129)),
        }
    }

    /// API adapter: an HTTP request over the forwarded private socket. No upstream
    /// authentication headers are sent to the target or supplied by the target.
    pub(crate) async fn request(
        socket: &Path,
        method: Method,
        url: &Url,
        headers: &http::HeaderMap,
    ) -> Result<reqwest::Response> {
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            bail!("unsupported relay destination");
        }
        let prefix = match url.host_str() {
            Some("api.github.com") => "api",
            Some("github.com") => "web",
            _ => bail!("unsupported relay host"),
        };
        let mut relay_url = Url::parse(&format!("http://localhost/{prefix}{}", url.path()))?;
        relay_url.set_query(url.query());
        let client = Client::builder()
            .unix_socket(socket)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .read_timeout(Duration::from_secs(300))
            .build()?;
        let mut req = client.request(method, relay_url);
        for name in ["accept", "range", "if-range"] {
            if let Some(value) = headers.get(name) {
                req = req.header(name, value);
            }
        }
        req.send().await.map_err(|error| {
            eyre::Report::new(error.without_url())
                .wrap_err("GitHub relay disconnected or unavailable")
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn audit_redacts_dynamic_paths_queries_and_denied_requests() {
            let audit = Audit::new(
                "devbox",
                Options {
                    jsonl: true,
                    ..Default::default()
                },
            );
            let scope = Scope::from_flags(true, &["jdx/mise".into()], false)
                .unwrap()
                .unwrap();
            for uri in [
                "/api/repos/jdx/mise/contents/private-secret?ref=query-secret",
                "/api/repos/other/private-secret?token=query-secret",
            ] {
                let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
                let operation = audit.operation(&scope, &request);
                let line = audit.line(serde_json::json!({"event":"request", "operation":operation, "status":403, "headers_ms":1}));
                assert!(!line.contains("private-secret"));
                assert!(!line.contains("query-secret"));
                let event: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(event["destination"], "devbox");
            }
        }

        #[tokio::test]
        async fn access_duration_expires_and_timer_is_owned() {
            let audit = Arc::new(Audit::new(
                "test",
                Options {
                    max_duration: Duration::from_millis(10),
                    ..Default::default()
                },
            ));
            let cancel = CancellationToken::new();
            let _timer = expiry_task(cancel.clone(), audit.clone());
            tokio::time::timeout(Duration::from_secs(1), cancel.cancelled())
                .await
                .unwrap();
            let cancel = CancellationToken::new();
            drop(expiry_task(cancel.clone(), audit));
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(!cancel.is_cancelled());
        }
        #[test]
        fn archives_only_redirect_to_the_authorized_repository() {
            for (url, expected) in [
                (
                    "https://codeload.github.com/jdx/mise/legacy.tar.gz/main?token=ephemeral",
                    true,
                ),
                ("https://codeload.github.com/other/private/zip/main", false),
                ("https://codeload.github.com/jdx/mise/other/main", false),
                ("http://codeload.github.com/jdx/mise/zip/main", false),
                ("https://127.0.0.1/jdx/mise/zip/main", false),
                (
                    "https://codeload.github.com.attacker.invalid/jdx/mise/zip/main",
                    false,
                ),
            ] {
                assert_eq!(
                    archive_redirect(&Url::parse(url).unwrap(), Some("jdx/mise")),
                    expected
                );
                assert!(!archive_redirect(&Url::parse(url).unwrap(), None));
            }
        }
        #[tokio::test]
        async fn broker_authenticates_locally_and_redacts_failures() {
            let mut upstream = mockito::Server::new_async().await;
            let api = upstream
                .mock("GET", "/repos/jdx/mise/releases/latest")
                .match_header("authorization", "Bearer fake-local-token")
                .match_header("range", "bytes=10-")
                .match_header("if-range", "etag-1")
                .with_header("authorization", "must-not-reach-target")
                .with_body("release")
                .create_async()
                .await;
            let git = upstream
                .mock("POST", "/jdx/mise.git/git-upload-pack")
                .match_header("content-type", "application/x-git-upload-pack-request")
                .with_body("pack")
                .create_async()
                .await;
            let redirect = upstream
                .mock("GET", "/repos/jdx/mise/releases/assets/1")
                .with_status(302)
                .with_header("location", "http://127.0.0.1/secret?token=fake-local-token")
                .create_async()
                .await;
            let broker = Broker {
                audit: Arc::new(Audit::new("test", Options::default())),
                scope: Scope::from_flags(true, &["jdx/mise".into()], false)
                    .unwrap()
                    .unwrap(),
                client: Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                permits: Arc::new(Semaphore::new(8)),
                cancel: CancellationToken::new(),
                token: Arc::new("fake-local-token".into()),
                test_upstream: Some(upstream.url()),
            };
            for (method, path, expected) in [
                ("GET", "/api/repos/jdx/mise/releases/latest", 200),
                ("POST", "/git/jdx/mise.git/git-upload-pack", 200),
                ("GET", "/api/repos/jdx/mise/releases/assets/1", 403),
                ("POST", "/api/repos/jdx/mise/releases", 403),
                ("GET", "/api/repos/other/private", 403),
            ] {
                let request = Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", "remote-cannot-select-credentials")
                    .header("range", "bytes=10-")
                    .header("if-range", "etag-1")
                    .body(Body::empty())
                    .unwrap();
                let response = handle(State(broker.clone()), request).await;
                assert_eq!(response.status(), expected);
                assert!(response.headers().get("authorization").is_none());
                let body = to_bytes(response.into_body(), 1024).await.unwrap();
                assert!(!String::from_utf8_lossy(&body).contains("fake-local-token"));
            }
            api.assert_async().await;
            git.assert_async().await;
            redirect.assert_async().await;
            use std::sync::atomic::Ordering::Relaxed;
            assert_eq!(broker.audit.requests.load(Relaxed), 5);
            assert_eq!(broker.audit.denied.load(Relaxed), 3);
            assert_eq!(broker.audit.bytes.load(Relaxed), 11);
            let permits = broker.permits.clone().acquire_many_owned(8).await.unwrap();
            let response = handle(
                State(broker.clone()),
                Request::builder()
                    .uri("/api/repos/jdx/mise/releases/latest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(response.status(), 403);
            drop(permits);
            broker.cancel.cancel();
            let requests = broker.audit.requests.load(Relaxed);
            let heartbeat = handle(
                State(broker.clone()),
                Request::builder()
                    .uri("/_session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(heartbeat.status(), 403);
            assert_eq!(broker.audit.requests.load(Relaxed), requests);
            let response = handle(
                State(broker),
                Request::builder()
                    .uri("/api/repos/jdx/mise")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(response.status(), 403);
        }
        #[test]
        fn redirects_are_exact_https_asset_hosts() {
            for url in [
                "https://release-assets.githubusercontent.com/asset?signature=x",
                "https://objects.githubusercontent.com/asset",
            ] {
                assert!(asset_redirect(&Url::parse(url).unwrap()));
            }
            for url in [
                "https://github.com/asset",
                "https://127.0.0.1/",
                "http://objects.githubusercontent.com/asset",
                "https://objects.githubusercontent.com.evil.invalid/a",
                "https://user@objects.githubusercontent.com/a",
                "https://objects.githubusercontent.com:8443/a",
            ] {
                assert!(!asset_redirect(&Url::parse(url).unwrap()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope::from_flags(true, &["jdx/mise".into()], false)
            .unwrap()
            .unwrap()
    }
    #[test]
    fn scope_requires_explicit_choice() {
        assert!(Scope::from_flags(true, &[], false).is_err());
        assert!(Scope::from_flags(false, &[], true).is_err());
        assert!(Scope::from_flags(true, &["jdx/mise".into()], true).is_err());
        assert!(
            Scope::from_flags(true, &[], true)
                .unwrap()
                .unwrap()
                .permits("other/private")
        );
    }
    #[test]
    fn reads_only() {
        for path in [
            "api/repos/jdx/mise",
            "api/repos/jdx/mise/releases/latest",
            "api/repos/jdx/mise/contents/Cargo.toml",
            "api/repos/jdx/mise/releases/tags/v%C3%A9",
        ] {
            assert!(authorize(&scope(), "GET", path, None).is_ok());
            assert!(authorize(&scope(), "POST", path, None).is_err());
        }
        assert!(authorize(&scope(), "POST", "git/jdx/mise.git/git-upload-pack", None).is_ok());
        assert!(
            authorize(
                &scope(),
                "GET",
                "git/jdx/mise.git/info/refs",
                Some("service=git-upload-pack")
            )
            .is_ok()
        );
        for path in [
            "api/repos/other/private",
            "api/user",
            "api/graphql",
            "api/repos/jdx/mise/../../user",
            "api/repos/jdx/mise/contents/%2e%2e",
            "api/repos/jdx/mise/contents/%252e%252e",
            "api/repos/jdx/mise/contents/a%2Fb",
            "api/repos/jdx/mise/contents/%5c",
            "api/repos/jdx/mise/contents/%00",
            "api/repos/jdx/mise/contents/%zz",
            "git/jdx/mise.git/git-receive-pack",
        ] {
            assert!(authorize(&scope(), "GET", path, None).is_err(), "{path}");
        }
    }
    #[test]
    fn shorthand() {
        assert_eq!(
            expand_repository("jdx/mise").unwrap(),
            "https://github.com/jdx/mise.git"
        );
        for value in [
            "./my/repo",
            "git@github.com:jdx/mise.git",
            "https://example.com/r.git",
            "/tmp/repo",
        ] {
            assert_eq!(expand_repository(value).unwrap(), value);
        }
        assert!(expand_repository("not/a/repository").is_err());
    }

    #[test]
    fn observability_options_never_enable_access() {
        assert!(configure(None, false, false, None, None).unwrap().is_none());
        assert!(configure(None, true, false, None, None).is_err());
        assert!(configure(None, false, false, Some("jsonl"), None).is_err());
        assert!(configure(None, false, false, None, Some("1h")).is_err());
        assert!(configure(Some(scope()), false, false, Some("invalid"), None).is_err());
        assert!(configure(Some(scope()), false, false, None, Some("-1h")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn cli_observability_overrides() {
        let scope = configure(Some(scope()), true, false, Some("jsonl"), Some("1h"))
            .unwrap()
            .unwrap();
        assert!(scope.options.log_requests);
        assert!(scope.options.jsonl);
        assert_eq!(scope.options.max_duration.as_secs(), 3600);
        let scope = configure(Some(scope), true, true, Some("text"), Some("0s"))
            .unwrap()
            .unwrap();
        assert!(!scope.options.log_requests);
        assert!(!scope.options.jsonl);
        assert!(scope.options.max_duration.is_zero());
    }

    #[test]
    fn web_archives_use_scoped_api_downloads() {
        let target = authorize(
            &scope(),
            "GET",
            "web/jdx/mise/archive/refs/tags/v1.tar.gz",
            None,
        )
        .unwrap();
        assert_eq!(
            target.url,
            "https://api.github.com/repos/jdx/mise/tarball/refs/tags/v1"
        );
        assert_eq!(target.archive_repo.as_deref(), Some("jdx/mise"));
        assert!(authorize(&scope(), "GET", "api/repos/jdx/mise/zipball/main", None).is_ok());
        assert!(authorize(&scope(), "POST", "api/repos/jdx/mise/tarball/main", None).is_err());
        assert!(authorize(&scope(), "GET", "web/other/private/archive/main.zip", None).is_err());
    }
}
