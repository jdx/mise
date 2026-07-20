//! OCI Distribution Spec v2 client.
//!
//! Pull side: used by `mise oci build --from <ref>` to stream a base image's
//! layers into the output layout byte-for-byte so digests match.
//!
//! Push side: used by `mise oci push` to upload an OCI image layout directly
//! — no skopeo/crane required. Credentials come from the same sources docker
//! and podman use (see `crate::oci::auth`); anonymous access is used when no
//! credentials are found (e.g. a local `registry:2`).

use std::path::Path;
use std::sync::Arc;

use eyre::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;

use crate::http::HTTP;
use crate::oci::auth::Credential;
use crate::oci::layout::ImageLayout;
use crate::oci::manifest::{
    Descriptor, ImageIndex, ImageManifest, MEDIA_TYPE_DOCKER_MANIFEST,
    MEDIA_TYPE_DOCKER_MANIFEST_LIST, MEDIA_TYPE_OCI_INDEX, MEDIA_TYPE_OCI_MANIFEST,
};
use crate::ui::progress_report::SingleReport;

/// A parsed registry reference.
#[derive(Debug, Clone)]
pub struct Reference {
    pub registry: String,
    pub repository: String,
    pub tag: String,
}

impl Reference {
    /// Parse a reference like:
    ///   `debian:bookworm-slim` → docker.io/library/debian:bookworm-slim
    ///   `ghcr.io/foo/bar:tag` → ghcr.io/foo/bar:tag
    ///   `docker.io/library/node:20` → docker.io/library/node:20
    ///   `ubuntu@sha256:…` → docker.io/library/ubuntu at that digest
    ///
    /// Digest references (`name@sha256:…`) are handled before tag parsing so
    /// the `:` inside the digest isn't mistaken for a tag separator.
    pub fn parse(s: &str) -> Result<Self> {
        // Split off `@sha256:...` (or any `@digest`) first — in the registry
        // v2 URL scheme the full `sha256:hex` string takes the place of the
        // tag for GET /v2/<name>/manifests/<reference>.
        let (name, tag) = if let Some((n, digest)) = s.split_once('@') {
            (n, digest.to_string())
        } else {
            let (n, t) = match s.rsplit_once(':') {
                Some((n, t)) if !t.contains('/') => (n, t.to_string()),
                _ => (s, "latest".to_string()),
            };
            (n, t)
        };

        // Heuristic: if the first path segment contains a '.' or ':' it's the
        // registry host. Otherwise we default to docker.io.
        let (registry, repository) = if let Some(idx) = name.find('/') {
            let head = &name[..idx];
            if head.contains('.') || head.contains(':') || head == "localhost" {
                (head.to_string(), name[idx + 1..].to_string())
            } else {
                ("docker.io".to_string(), name.to_string())
            }
        } else {
            ("docker.io".to_string(), format!("library/{name}"))
        };

        let repository = if registry == "docker.io" && !repository.contains('/') {
            format!("library/{repository}")
        } else {
            repository
        };

        Ok(Self {
            registry,
            repository,
            tag,
        })
    }

    pub fn registry_url(&self) -> String {
        // Loopback registries (localhost:5000 etc.) serve plain HTTP — the
        // same insecure-by-default convention docker applies to 127.0.0.0/8.
        // Non-loopback plain-HTTP registries must be opted in via the
        // `oci.insecure_registries` setting. Evaluate against the
        // user-facing registry name (not the docker.io→registry-1 rewrite
        // below) so it matches the lookups `push_image` does with
        // `self.registry`.
        let scheme = if is_insecure_registry(&self.registry) {
            "http"
        } else {
            "https"
        };
        // docker.io is special — the distribution API is served from
        // registry-1.docker.io even though the canonical name is docker.io.
        let host = if self.registry == "docker.io" {
            "registry-1.docker.io"
        } else {
            &self.registry
        };
        format!("{scheme}://{host}")
    }
}

/// True when `registry` (a `host[:port]` / `[v6]:port` string) should be
/// contacted over plain HTTP: loopback addresses always, plus anything listed
/// in the `oci.insecure_registries` setting.
fn is_insecure_registry(registry: &str) -> bool {
    let settings = crate::config::Settings::get();
    let entries = settings.oci.insecure_registries.as_deref().unwrap_or(&[]);
    is_insecure_registry_in(registry, entries)
}

/// Settings-free core of [`is_insecure_registry`]: loopback, or listed in
/// `entries` (matched on the exact `host[:port]` string or the bare host).
fn is_insecure_registry_in(registry: &str, entries: &[String]) -> bool {
    if is_loopback_registry(registry) {
        return true;
    }
    let host = registry_host(registry);
    entries
        .iter()
        .any(|entry| entry == registry || entry == host)
}

/// The host portion of a `host[:port]` / `[v6]:port` registry string.
fn registry_host(registry: &str) -> &str {
    if let Some(rest) = registry.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        registry.rsplit_once(':').map_or(registry, |(h, _)| h)
    }
}

/// True when `registry` points at a loopback address.
fn is_loopback_registry(registry: &str) -> bool {
    let host = registry_host(registry);
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
}

/// A parsed `WWW-Authenticate` challenge.
enum AuthChallenge {
    Bearer {
        realm: String,
        service: Option<String>,
    },
    Basic,
}

fn parse_auth_challenge(www_auth: &str) -> Option<AuthChallenge> {
    let trimmed = www_auth.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("basic") {
        return Some(AuthChallenge::Basic);
    }
    if !lower.starts_with("bearer") {
        return None;
    }
    // WWW-Authenticate: Bearer realm="https://auth.docker.io/token",service="registry.docker.io"
    let mut realm: Option<String> = None;
    let mut service: Option<String> = None;
    for (key, value) in parse_challenge_params(&trimmed["bearer".len()..]) {
        match key.as_str() {
            "realm" => realm = Some(value),
            "service" => service = Some(value),
            _ => {}
        }
    }
    realm.map(|realm| AuthChallenge::Bearer { realm, service })
}

/// Parse the comma-separated `key=value` / `key="value"` parameters of an
/// auth-scheme challenge. Double-quoted values are honored, so a realm URL
/// with a query string (`realm="https://a/token?x=1,y=2"`) or an echoed
/// scope (`scope="repository:name:pull,push"`) isn't truncated at an
/// interior comma — the bug a naive `split(',')` would hit.
fn parse_challenge_params(s: &str) -> Vec<(String, String)> {
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut params = Vec::new();
    let mut i = 0;
    while i < n {
        // Skip separators / whitespace between parameters.
        while i < n && (bytes[i] == b',' || bytes[i].is_ascii_whitespace()) {
            i += 1;
        }
        // Read the key up to '=' (or ',' for a valueless token we ignore).
        let key_start = i;
        while i < n && bytes[i] != b'=' && bytes[i] != b',' {
            i += 1;
        }
        let key = s[key_start..i].trim().to_ascii_lowercase();
        if i >= n || bytes[i] == b',' {
            continue; // no value — skip
        }
        i += 1; // consume '='
        let value = if i < n && bytes[i] == b'"' {
            i += 1;
            let value_start = i;
            while i < n && bytes[i] != b'"' {
                i += 1;
            }
            let value = s[value_start..i].to_string();
            i += 1; // consume closing quote (if present)
            value
        } else {
            let value_start = i;
            while i < n && bytes[i] != b',' {
                i += 1;
            }
            s[value_start..i].trim().to_string()
        };
        if !key.is_empty() {
            params.push((key, value));
        }
    }
    params
}

/// Read a response header as an owned `String` (empty when absent / non-ASCII).
fn header_str(resp: &reqwest::Response, name: &str) -> String {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// Tracks the `Authorization` header for a sequence of requests to one
/// registry repository, (re)negotiating it from a `WWW-Authenticate`
/// challenge as needed.
///
/// Auth is challenge-driven rather than assumed from the `/v2/` probe: some
/// registries answer `200` on `GET /v2/` yet still challenge the actual
/// manifest / blob / upload requests (e.g. anonymous read but authenticated
/// push, or per-repository policies). The probe is only an upfront
/// optimization so the first real request usually already carries a token;
/// callers must still retry once when an operation returns `401`, feeding the
/// operation's own challenge back into [`AuthSession::answer_challenge`].
struct AuthSession {
    reference: Reference,
    credential: Option<Credential>,
    /// Full scope strings for token requests, e.g.
    /// `repository:me/app:pull,push`. Usually one entry; cross-repository
    /// blob mounts add a `pull` scope for the mount source repo.
    scopes: Vec<String>,
    authorization: Option<String>,
}

impl AuthSession {
    async fn new(reference: Reference, actions: &str) -> Result<Self> {
        let scope = format!("repository:{}:{actions}", reference.repository);
        Self::with_scopes(reference, vec![scope]).await
    }

    async fn with_scopes(reference: Reference, scopes: Vec<String>) -> Result<Self> {
        let credential = crate::oci::auth::resolve_credential(&reference.registry)?;
        let mut session = Self {
            reference,
            credential,
            scopes,
            authorization: None,
        };
        session.probe().await?;
        Ok(session)
    }

    fn header(&self) -> Option<&str> {
        self.authorization.as_deref()
    }

    fn has_credential(&self) -> bool {
        self.credential.is_some()
    }

    /// Best-effort upfront probe of `GET /v2/`. A `401` gets answered now so
    /// the first real request carries a token; a `200` (or a challenge we
    /// can't satisfy yet) simply leaves us anonymous until an operation's own
    /// `401` re-triggers negotiation.
    async fn probe(&mut self) -> Result<()> {
        let url = format!("{}/v2/", self.reference.registry_url());
        let resp = HTTP
            .get_async_with_headers_allow_error_status(&url, &HeaderMap::new())
            .await
            .wrap_err_with(|| format!("probing {url}"))?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            let www_auth = header_str(&resp, "www-authenticate");
            self.answer_challenge(&www_auth).await?;
        }
        Ok(())
    }

    /// Negotiate authorization from a `WWW-Authenticate` challenge string,
    /// storing the resulting header. Returns whether a usable `Authorization`
    /// header was obtained (a Basic challenge with no credentials yields
    /// `false` so the caller can surface an actionable message).
    async fn answer_challenge(&mut self, www_auth: &str) -> Result<bool> {
        match parse_auth_challenge(www_auth) {
            Some(AuthChallenge::Bearer { realm, service }) => {
                let token = fetch_bearer_token(
                    &realm,
                    service.as_deref(),
                    &self.scopes,
                    self.credential.as_ref(),
                )
                .await?;
                self.authorization = token.map(|t| format!("Bearer {t}"));
                Ok(self.authorization.is_some())
            }
            Some(AuthChallenge::Basic) => match &self.credential {
                Some(c) => {
                    self.authorization = Some(c.basic_auth_header());
                    Ok(true)
                }
                None => Ok(false),
            },
            // No / unrecognized challenge — stay with whatever we have.
            None => Ok(false),
        }
    }

    /// Send a request and, if it returns `401`, answer the response's own
    /// challenge and retry once with refreshed authorization. `build` is
    /// called with the current `Authorization` header (if any) and must
    /// produce a complete request — it may be invoked twice, so it reopens
    /// any streamed body itself.
    async fn send<F>(&mut self, build: F) -> Result<reqwest::Response>
    where
        F: Fn(Option<&str>) -> reqwest::RequestBuilder,
    {
        let resp = build(self.header()).send().await?;
        if resp.status() != StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        let www_auth = header_str(&resp, "www-authenticate");
        if self.answer_challenge(&www_auth).await? {
            return Ok(build(self.header()).send().await?);
        }
        Ok(resp)
    }
}

/// Fetch a bearer token from a registry's token endpoint. Anonymous when
/// `credential` is `None` (public pulls); authenticated via Basic auth on the
/// token request otherwise. Docker Hub identity tokens (from Docker Desktop
/// logins) use the OAuth2 refresh-token POST flow instead.
async fn fetch_bearer_token(
    realm: &str,
    service: Option<&str>,
    scopes: &[String],
    credential: Option<&Credential>,
) -> Result<Option<String>> {
    if let Some(c) = credential
        && c.username == "<token>"
    {
        // OAuth2 identity-token flow. Multiple scopes are space-separated in
        // the OAuth2 `scope` parameter.
        let scope = scopes.join(" ");
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", c.secret.as_str()),
            ("client_id", "mise"),
            ("scope", scope.as_str()),
        ];
        if let Some(s) = service {
            form.push(("service", s));
        }
        let resp = HTTP
            .reqwest()
            .post(realm)
            .form(&form)
            .send()
            .await
            .wrap_err_with(|| format!("fetching OAuth2 token from {realm}"))?
            .error_for_status()?;
        let resp: TokenResponse = resp.json().await?;
        return Ok(resp.access_token.or(resp.token));
    }

    let mut url = url::Url::parse(realm)?;
    {
        let mut q = url.query_pairs_mut();
        if let Some(s) = service {
            q.append_pair("service", s);
        }
        // The token endpoint takes one `scope` query param per scope.
        for scope in scopes {
            q.append_pair("scope", scope);
        }
    }
    let mut headers = HeaderMap::new();
    if let Some(c) = credential {
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&c.basic_auth_header())?,
        );
    }
    let resp: TokenResponse = HTTP
        .json_with_headers(url.as_str(), &headers)
        .await
        .wrap_err_with(|| match credential {
            Some(c) => format!(
                "fetching token from {realm} as {} (are the stored credentials still valid?)",
                c.username
            ),
            None => format!("fetching anonymous token from {realm}"),
        })?;
    Ok(resp.token.or(resp.access_token))
}

/// Fetch a manifest (or index) as JSON, retrying once on `401` with a
/// negotiated token. Returns the parsed body and the response `Content-Type`
/// (the caller uses it to distinguish a single manifest from an index).
async fn fetch_manifest_json(
    session: &mut AuthSession,
    url: &str,
    accept: &[&str],
) -> Result<(serde_json::Value, String)> {
    let accept_hdr = accept.join(", ");
    let resp = session
        .send(|auth| {
            let mut rb = HTTP.reqwest().get(url).header("Accept", &accept_hdr);
            if let Some(a) = auth {
                rb = rb.header("Authorization", a);
            }
            rb
        })
        .await
        .wrap_err_with(|| format!("fetching {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let hint = if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            if session.has_credential() {
                " — the stored credentials were rejected or lack access to this image"
            } else {
                " — the image may be private; run `docker login` (or `podman login`) for this registry"
            }
        } else {
            ""
        };
        let body = resp.text().await.unwrap_or_default();
        bail!(
            "fetching {url} failed: {}{hint}\n{}",
            status.as_u16(),
            body.trim()
        );
    }
    let content_type = header_str(&resp, "content-type");
    let body: serde_json::Value = resp
        .json()
        .await
        .wrap_err_with(|| format!("parsing JSON response from {url}"))?;
    Ok((body, content_type))
}

/// Retry a transient-failure-prone operation with mise's standard backoff
/// schedule. Transient means connect/timeout/body errors and 5xx/408/429
/// statuses surfaced via `error_for_status`. A macro rather than a generic
/// fn so the operation expression can reborrow `&mut` state (the
/// [`AuthSession`]) on every attempt.
macro_rules! retry_transient {
    ($verb:expr, $url:expr, $op:expr) => {{
        let mut backoff =
            crate::http::default_backoff_strategy(crate::config::Settings::get().http_retries());
        let mut attempt = 1;
        loop {
            match $op {
                Ok(v) => break Ok(v),
                Err(err) => {
                    if !crate::http::is_transient(&err) {
                        break Err(err);
                    }
                    let Some(delay) = backoff.next() else {
                        break Err(err);
                    };
                    warn!(
                        "{} {} attempt {attempt} failed (transient): {err}; retrying in {delay:?}",
                        $verb, $url
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }};
}

/// Download a blob (config or layer) into memory, refreshing auth on `401`
/// (via [`AuthSession::send`]) and retrying transient failures. `pr` shows
/// byte progress for large layers.
async fn download_blob(
    session: &mut AuthSession,
    url: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<Vec<u8>> {
    retry_transient!("GET", url, download_blob_once(session, url, pr).await)
}

/// One download attempt: GET the blob through the auth session, streaming
/// chunks into memory and advancing `pr` as they arrive.
async fn download_blob_once(
    session: &mut AuthSession,
    url: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<Vec<u8>> {
    let resp = session
        .send(|auth| {
            let mut rb = HTTP.reqwest().get(url);
            if let Some(a) = auth {
                rb = rb.header("Authorization", a);
            }
            rb
        })
        .await
        .wrap_err_with(|| format!("GET {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        // 5xx/408/429 become transient reqwest status errors (retried by the
        // caller); other statuses fall through to a deterministic failure.
        resp.error_for_status_ref()?;
        bail!("fetching blob {url} failed: {}", status.as_u16());
    }
    if let Some(pr) = pr {
        if let Some(len) = resp.content_length() {
            pr.set_length(len);
        }
        pr.set_position(0);
    }
    let mut resp = resp;
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if let Some(pr) = pr {
            pr.inc(chunk.len() as u64);
        }
    }
    Ok(bytes)
}

/// The result of pulling a base image — the config blob and an ordered list
/// of layer descriptors (referenced in the new image manifest we'll build).
pub struct BasePull {
    pub layers: Vec<Descriptor>,
    pub platform: Option<crate::oci::manifest::Platform>,
    /// Parsed config (so the builder can inherit env, cmd, etc.).
    pub config_json: serde_json::Value,
}

pub async fn pull_base_image(
    reference: &str,
    layout: &ImageLayout,
    desired_platform: Option<(&str, &str)>,
) -> Result<BasePull> {
    let r = Reference::parse(reference)?;
    let base_url = r.registry_url();

    // Fetch manifest with both OCI and Docker Accept headers. Try anonymously
    // first, then handle 401 by grabbing a bearer token.
    let manifest_url = format!("{base_url}/v2/{}/manifests/{}", r.repository, r.tag);

    let accept = [
        MEDIA_TYPE_OCI_MANIFEST,
        MEDIA_TYPE_DOCKER_MANIFEST,
        MEDIA_TYPE_OCI_INDEX,
        MEDIA_TYPE_DOCKER_MANIFEST_LIST,
    ];

    // Negotiate auth (stored credentials for private images, anonymous
    // tokens otherwise). Auth is challenge-driven per request, so a registry
    // that answers 200 on /v2/ but guards the manifest still works.
    let mut session = AuthSession::new(r.clone(), "pull").await?;

    // Try OCI/Docker manifest or an index (multi-arch).
    let (body, content_type) = fetch_manifest_json(&mut session, &manifest_url, &accept)
        .await
        .wrap_err_with(|| format!("fetching manifest for {reference}"))?;

    let manifest = resolve_manifest(
        body,
        &r,
        base_url.as_str(),
        &mut session,
        desired_platform,
        &content_type,
    )
    .await?;

    // Validate every registry-supplied digest up front — a malicious
    // registry could otherwise return `sha256:../../etc/passwd` and have it
    // slip through the `blob_path().exists()` cache-check below (which
    // bypasses the digest verification inside `write_blob_with_digest`).
    crate::oci::layout::validate_sha256_digest(&manifest.config.digest)?;
    for layer in &manifest.layers {
        crate::oci::layout::validate_sha256_digest(&layer.digest)?;
    }

    // Download config blob and stream layer blobs into the layout.
    let config_url = format!(
        "{base_url}/v2/{}/blobs/{}",
        r.repository, manifest.config.digest
    );
    let config_bytes = download_blob(&mut session, &config_url, None).await?;
    // Preserve the byte-level digest by writing under the exact digest name.
    layout.write_blob_with_digest(&manifest.config.digest, &config_bytes)?;

    let mpr = crate::ui::multi_progress_report::MultiProgressReport::get();
    for layer in &manifest.layers {
        let layer_url = format!("{base_url}/v2/{}/blobs/{}", r.repository, layer.digest);
        let blob_path = layout.blob_path(&layer.digest);
        if blob_path.exists() {
            continue;
        }
        let pr = mpr.add(&format!("pull {}", short_digest(&layer.digest)));
        pr.set_length(layer.size);
        let bytes = match download_blob(&mut session, &layer_url, Some(&*pr)).await {
            Ok(bytes) => bytes,
            Err(e) => {
                pr.abandon();
                return Err(e);
            }
        };
        layout.write_blob_with_digest(&layer.digest, &bytes)?;
        pr.finish();
    }

    let config_json: serde_json::Value = serde_json::from_slice(&config_bytes)?;
    let platform = config_json
        .get("architecture")
        .and_then(|a| a.as_str())
        .zip(config_json.get("os").and_then(|o| o.as_str()))
        .map(|(arch, os)| crate::oci::manifest::Platform {
            architecture: arch.to_string(),
            os: os.to_string(),
            os_version: None,
            os_features: vec![],
            variant: None,
        });

    Ok(BasePull {
        layers: manifest.layers.clone(),
        platform,
        config_json,
    })
}

/// Given a manifest body (possibly an index with multiple architectures),
/// resolve to a concrete single-image manifest and return its parsed form.
async fn resolve_manifest(
    body: serde_json::Value,
    r: &Reference,
    base_url: &str,
    session: &mut AuthSession,
    desired_platform: Option<(&str, &str)>,
    content_type: &str,
) -> Result<ImageManifest> {
    // The OCI spec marks `mediaType` in the body as SHOULD, not MUST. Some
    // registries omit it, so we also consult the response Content-Type
    // header and a structural fallback (presence of a `manifests` array).
    let body_media_type = body.get("mediaType").and_then(|m| m.as_str()).unwrap_or("");
    let has_manifests_array = body.get("manifests").map(|m| m.is_array()).unwrap_or(false);
    let is_index = body_media_type == MEDIA_TYPE_OCI_INDEX
        || body_media_type == MEDIA_TYPE_DOCKER_MANIFEST_LIST
        || content_type.contains(MEDIA_TYPE_OCI_INDEX)
        || content_type.contains(MEDIA_TYPE_DOCKER_MANIFEST_LIST)
        || (body_media_type.is_empty() && has_manifests_array);

    // If this is an index / manifest list, pick the right child manifest.
    if is_index {
        let manifests = body
            .get("manifests")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();
        let (arch, os) = desired_platform.unwrap_or((std::env::consts::ARCH, std::env::consts::OS));
        let arch = crate::oci::normalize_arch(arch);
        let os = crate::oci::normalize_os(os);
        let picked = manifests.iter().find(|m| {
            let a = m
                .get("platform")
                .and_then(|p| p.get("architecture"))
                .and_then(|a| a.as_str())
                .unwrap_or("");
            let o = m
                .get("platform")
                .and_then(|p| p.get("os"))
                .and_then(|o| o.as_str())
                .unwrap_or("");
            a == arch && o == os
        });
        let picked = picked.ok_or_else(|| {
            eyre::eyre!(
                "no matching platform {arch}/{os} in manifest index for {}",
                r.repository
            )
        })?;
        let digest = picked
            .get("digest")
            .and_then(|d| d.as_str())
            .ok_or_else(|| eyre::eyre!("manifest entry missing digest"))?;
        let manifest_url = format!("{base_url}/v2/{}/manifests/{digest}", r.repository);
        let accept = [MEDIA_TYPE_OCI_MANIFEST, MEDIA_TYPE_DOCKER_MANIFEST];
        let (body, _content_type) = fetch_manifest_json(session, &manifest_url, &accept).await?;
        return parse_single_manifest(body);
    }

    parse_single_manifest(body)
}

fn parse_single_manifest(body: serde_json::Value) -> Result<ImageManifest> {
    let manifest: ImageManifest = serde_json::from_value(body)
        .wrap_err("parsing OCI/Docker manifest; schema v1 manifests are not supported")?;
    Ok(manifest)
}

// ---------------------------------------------------------------------------
// Push
// ---------------------------------------------------------------------------

/// Summary of a completed push, for CLI reporting.
pub struct PushSummary {
    pub manifest_digest: String,
    pub uploaded: usize,
    pub skipped: usize,
    /// Blobs satisfied by cross-repository mount (no bytes transferred).
    pub mounted: usize,
}

/// Blobs above this size upload in chunks (`PATCH` per chunk) instead of a
/// single monolithic `PUT`. Keeps individual request bodies below the limits
/// some registries/CDNs impose (e.g. 100 MB behind Cloudflare) and bounds
/// how much a transient mid-upload failure costs.
const UPLOAD_CHUNK_SIZE: u64 = 64 * 1024 * 1024;

/// The standard annotation naming the base image a manifest was built from
/// (written by `mise oci build`). Push uses it to attempt cross-repository
/// blob mounts when the base lives on the same registry.
pub const ANNOTATION_BASE_NAME: &str = "org.opencontainers.image.base.name";

/// Push an OCI image layout directory to a registry reference.
///
/// Uploads only blobs the registry doesn't already have (HEAD check per
/// blob), then PUTs the manifest under the reference's tag (or digest).
/// Base-image blobs hosted on the same registry are cross-repo mounted
/// instead of re-uploaded when possible.
pub async fn push_image(image_dir: &Path, reference: &str) -> Result<PushSummary> {
    eyre::ensure!(
        !crate::config::Settings::get().offline(),
        "offline mode is enabled"
    );
    let r = Reference::parse(reference)?;
    let layout = ImageLayout {
        root: image_dir.to_path_buf(),
    };

    // Resolve the layout's single manifest. `mise oci build` always writes
    // exactly one manifest into index.json.
    let index_bytes = crate::file::read(image_dir.join("index.json"))?;
    let index: ImageIndex = serde_json::from_slice(&index_bytes).wrap_err("parsing index.json")?;
    let manifest_desc = match index.manifests.as_slice() {
        [one] => one,
        [] => bail!("{}: index.json lists no manifests", image_dir.display()),
        many => bail!(
            "{}: index.json lists {} manifests; multi-manifest layouts are not supported",
            image_dir.display(),
            many.len()
        ),
    };
    let manifest_bytes = layout.read_blob(&manifest_desc.digest)?;
    let manifest: ImageManifest =
        serde_json::from_slice(&manifest_bytes).wrap_err("parsing image manifest blob")?;

    // Cross-repo mount source: the base image's repository, when it lives on
    // the destination registry (and isn't the destination repo itself).
    let mount_from = manifest
        .annotations
        .get(ANNOTATION_BASE_NAME)
        .and_then(|name| Reference::parse(name).ok())
        .filter(|base| base.registry == r.registry && base.repository != r.repository)
        .map(|base| base.repository);

    // Negotiate auth once up front; individual requests still re-negotiate on
    // a 401 (a registry may 200 on /v2/ yet challenge the push operations).
    // Mounting requires pull access on the source repo, so that scope is
    // requested alongside the destination's pull,push.
    let mut scopes = vec![format!("repository:{}:pull,push", r.repository)];
    if let Some(from) = &mount_from {
        scopes.push(format!("repository:{from}:pull"));
    }
    let session = AuthSession::with_scopes(r.clone(), scopes).await?;
    if !session.has_credential() {
        // Not fatal — local registries accept anonymous pushes — but worth
        // surfacing before a 401 does. For loopback / configured-insecure
        // registries anonymous is the normal case, so don't warn there.
        if is_insecure_registry(&r.registry) {
            debug!(
                "no registry credentials found for {} — pushing anonymously",
                r.registry
            );
        } else {
            warn!(
                "no registry credentials found for {} — pushing anonymously. \
                 Run `docker login {}` (or `podman login`) if the push is rejected.",
                r.registry, r.registry
            );
        }
    }
    let mut pusher = Pusher {
        base_url: r.registry_url(),
        repository: r.repository.clone(),
        session,
        mount_from,
    };

    // Config + layers, deduped (identical layers can legitimately repeat).
    let mut blobs: Vec<&Descriptor> = vec![&manifest.config];
    let mut seen = std::collections::HashSet::new();
    seen.insert(manifest.config.digest.as_str());
    for layer in &manifest.layers {
        if seen.insert(layer.digest.as_str()) {
            blobs.push(layer);
        }
    }

    let mpr = crate::ui::multi_progress_report::MultiProgressReport::get();
    let mut uploaded = 0;
    let mut skipped = 0;
    let mut mounted = 0;
    for desc in blobs {
        crate::oci::layout::validate_sha256_digest(&desc.digest)?;
        if pusher.blob_exists(&desc.digest).await? {
            debug!("blob {} already present, skipping", desc.digest);
            skipped += 1;
            continue;
        }
        // Arc so the streaming request body (which must be 'static) can
        // advance the progress bar from inside the byte stream.
        let pr: Arc<dyn SingleReport> = Arc::from(mpr.add(&format!("push {}", blob_label(desc))));
        pr.set_length(desc.size);
        let outcome = match pusher
            .upload_blob(
                &layout.blob_path(&desc.digest),
                &desc.digest,
                desc.size,
                &pr,
            )
            .await
            .wrap_err_with(|| format!("uploading blob {}", desc.digest))
        {
            Ok(outcome) => outcome,
            Err(e) => {
                pr.abandon();
                return Err(e);
            }
        };
        match outcome {
            UploadOutcome::Uploaded => {
                uploaded += 1;
                pr.finish();
            }
            UploadOutcome::Mounted => {
                mounted += 1;
                pr.finish_with_message("mounted from base image repo".into());
            }
        }
    }

    pusher
        .put_manifest(&r.tag, &manifest_desc.media_type, &manifest_bytes)
        .await
        .wrap_err_with(|| format!("pushing manifest to {reference}"))?;

    Ok(PushSummary {
        manifest_digest: manifest_desc.digest.clone(),
        uploaded,
        skipped,
        mounted,
    })
}

/// Progress label for a blob: the tool name when the descriptor carries the
/// mise tool annotation, otherwise a shortened digest.
fn blob_label(desc: &Descriptor) -> String {
    desc.annotations
        .get("dev.mise.tool.short")
        .cloned()
        .unwrap_or_else(|| short_digest(&desc.digest).to_string())
}

/// First 12 hex chars of a `sha256:…` digest, for display.
fn short_digest(digest: &str) -> &str {
    let hex = digest.trim_start_matches("sha256:");
    &hex[..hex.len().min(12)]
}

struct Pusher {
    base_url: String,
    repository: String,
    session: AuthSession,
    /// Repository on the same registry to attempt cross-repo blob mounts
    /// from (the base image's repo, when it matches the destination host).
    mount_from: Option<String>,
}

/// How a blob ended up present in the destination repository.
enum UploadOutcome {
    /// Bytes were transferred.
    Uploaded,
    /// The registry cross-repo mounted it from `mount_from` — no transfer.
    Mounted,
}

impl Pusher {
    /// Returns true only when the registry confirms the blob is present
    /// (`200`). Any other response — `404`, an auth status, or an oddity like
    /// `405 Method Not Allowed` from a proxy that doesn't implement blob
    /// HEAD — is treated as "not present", so the upload proceeds and any
    /// genuine problem surfaces there with a clearer message.
    async fn blob_exists(&mut self, digest: &str) -> Result<bool> {
        let url = format!("{}/v2/{}/blobs/{digest}", self.base_url, self.repository);
        let resp = self
            .session
            .send(|auth| {
                let mut rb = HTTP.reqwest().head(&url);
                if let Some(a) = auth {
                    rb = rb.header("Authorization", a);
                }
                rb
            })
            .await
            .wrap_err_with(|| format!("HEAD {url}"))?;
        Ok(resp.status() == StatusCode::OK)
    }

    /// Upload one blob, retrying the whole sequence on transient failures.
    /// Progress restarts from the beginning on retry (uploads aren't resumed
    /// across attempts — a fresh upload session is opened each time).
    async fn upload_blob(
        &mut self,
        path: &Path,
        digest: &str,
        size: u64,
        pr: &Arc<dyn SingleReport>,
    ) -> Result<UploadOutcome> {
        // Fail early (and clearly) if the blob file is unreadable, rather than
        // letting an empty-body request surface later as a confusing registry
        // digest/upload error with no hint about the real cause.
        std::fs::File::open(path)
            .wrap_err_with(|| format!("opening blob {} for upload", path.display()))?;
        retry_transient!(
            "upload",
            digest,
            self.upload_blob_once(path, digest, size, pr).await
        )
    }

    /// One upload attempt: open an upload session (attempting a cross-repo
    /// mount when a source repo is known), then transfer the bytes —
    /// monolithic `PUT` for small blobs, chunked `PATCH`es + finalizing `PUT`
    /// for large ones.
    async fn upload_blob_once(
        &mut self,
        path: &Path,
        digest: &str,
        size: u64,
        pr: &Arc<dyn SingleReport>,
    ) -> Result<UploadOutcome> {
        let had_credential = self.session.has_credential();

        // 1. Open an upload session. With mount params, a 201 means the
        // registry satisfied the blob by mounting; a 202 means "mount not
        // possible, here's a regular upload session" (the spec's fallback).
        let mut start_url = url::Url::parse(&format!(
            "{}/v2/{}/blobs/uploads/",
            self.base_url, self.repository
        ))?;
        if let Some(from) = &self.mount_from {
            start_url
                .query_pairs_mut()
                .append_pair("mount", digest)
                .append_pair("from", from);
        }
        let resp = self
            .session
            .send(|auth| {
                let mut rb = HTTP
                    .reqwest()
                    .post(start_url.as_str())
                    .header("Content-Length", "0");
                if let Some(a) = auth {
                    rb = rb.header("Authorization", a);
                }
                rb
            })
            .await
            .wrap_err_with(|| format!("POST {start_url}"))?;
        let status = resp.status();
        match status {
            StatusCode::CREATED => return Ok(UploadOutcome::Mounted),
            StatusCode::ACCEPTED => {}
            s => {
                // Let transient statuses bubble as retryable errors.
                resp.error_for_status_ref()?;
                bail!(
                    "starting blob upload failed: {} {}{}",
                    s.as_u16(),
                    start_url,
                    push_auth_hint(s, had_credential),
                );
            }
        }
        let mut location = self.resolve_location(&resp)?;
        pr.set_position(0);

        // 2. Transfer the bytes.
        if size > UPLOAD_CHUNK_SIZE {
            // Chunked: PATCH each segment, then a zero-length finalizing PUT.
            let mut offset = 0u64;
            while offset < size {
                let len = UPLOAD_CHUNK_SIZE.min(size - offset);
                let resp = self
                    .session
                    .send(|auth| {
                        build_upload_request(
                            HTTP.reqwest().patch(location.as_str()),
                            auth,
                            path,
                            offset,
                            len,
                            pr,
                        )
                        // Content-Range is inclusive on both ends.
                        .header("Content-Range", format!("{}-{}", offset, offset + len - 1))
                    })
                    .await
                    .wrap_err("PATCH blob chunk")?;
                let status = resp.status();
                if status != StatusCode::ACCEPTED {
                    resp.error_for_status_ref()?;
                    let body = resp.text().await.unwrap_or_default();
                    bail!(
                        "blob chunk upload failed: {}{}\n{}",
                        status.as_u16(),
                        push_auth_hint(status, had_credential),
                        body.trim(),
                    );
                }
                location = self.resolve_location(&resp).unwrap_or(location);
                offset += len;
            }
            // Finalize with ?digest=…
            let mut put_url = location;
            put_url.query_pairs_mut().append_pair("digest", digest);
            let resp = self
                .session
                .send(|auth| {
                    let mut rb = HTTP
                        .reqwest()
                        .put(put_url.as_str())
                        .header("Content-Length", "0");
                    if let Some(a) = auth {
                        rb = rb.header("Authorization", a);
                    }
                    rb
                })
                .await
                .wrap_err("PUT blob upload (finalize)")?;
            let status = resp.status();
            if !status.is_success() {
                resp.error_for_status_ref()?;
                let body = resp.text().await.unwrap_or_default();
                bail!(
                    "blob upload failed: {}{}\n{}",
                    status.as_u16(),
                    push_auth_hint(status, had_credential),
                    body.trim(),
                );
            }
        } else {
            // Monolithic PUT with ?digest=…
            let mut put_url = location;
            put_url.query_pairs_mut().append_pair("digest", digest);
            let resp = self
                .session
                .send(|auth| {
                    build_upload_request(
                        HTTP.reqwest().put(put_url.as_str()),
                        auth,
                        path,
                        0,
                        size,
                        pr,
                    )
                })
                .await
                .wrap_err("PUT blob upload")?;
            let status = resp.status();
            if status != StatusCode::CREATED && status != StatusCode::ACCEPTED {
                resp.error_for_status_ref()?;
                let body = resp.text().await.unwrap_or_default();
                bail!(
                    "blob upload failed: {}{}\n{}",
                    status.as_u16(),
                    push_auth_hint(status, had_credential),
                    body.trim(),
                );
            }
        }
        Ok(UploadOutcome::Uploaded)
    }

    /// Resolve the `Location` header of an upload-session response, which may
    /// be absolute or registry-relative.
    fn resolve_location(&self, resp: &reqwest::Response) -> Result<url::Url> {
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| eyre::eyre!("registry returned no Location for blob upload"))?;
        if location.starts_with("http://") || location.starts_with("https://") {
            Ok(url::Url::parse(location)?)
        } else {
            Ok(url::Url::parse(&format!("{}{}", self.base_url, location))?)
        }
    }

    async fn put_manifest(&mut self, tag: &str, media_type: &str, bytes: &[u8]) -> Result<()> {
        let had_credential = self.session.has_credential();
        let url = format!("{}/v2/{}/manifests/{tag}", self.base_url, self.repository);
        let body = bytes.to_vec();
        let resp = self
            .session
            .send(|auth| {
                let mut rb = HTTP
                    .reqwest()
                    .put(&url)
                    .header("Content-Type", media_type)
                    .body(body.clone());
                if let Some(a) = auth {
                    rb = rb.header("Authorization", a);
                }
                rb
            })
            .await
            .wrap_err_with(|| format!("PUT {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!(
                "manifest push failed: {} {url}{}\n{}",
                status.as_u16(),
                push_auth_hint(status, had_credential),
                body.trim(),
            );
        }
        Ok(())
    }
}

/// Build a PATCH/PUT upload request whose body streams `len` bytes of `path`
/// starting at `offset`, advancing `pr` as chunks are read off disk.
/// Constructed fresh on every call so the auth-retry inside
/// [`AuthSession::send`] can safely re-send the request; the progress
/// position is reset to `offset` each time so a re-send doesn't double-count.
fn build_upload_request(
    rb: reqwest::RequestBuilder,
    auth: Option<&str>,
    path: &Path,
    offset: u64,
    len: u64,
    pr: &Arc<dyn SingleReport>,
) -> reqwest::RequestBuilder {
    use futures_util::StreamExt;
    use std::io::{Seek, SeekFrom};

    let mut rb = rb
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", len);
    if let Some(a) = auth {
        rb = rb.header("Authorization", a);
    }

    let file = std::fs::File::open(path).and_then(|mut f| {
        f.seek(SeekFrom::Start(offset))?;
        Ok(f)
    });
    let file = match file {
        Ok(f) => tokio::fs::File::from_std(f),
        Err(e) => {
            // The file was validated readable before the upload began; if it
            // vanished mid-push, surface why before the registry's confusing
            // digest-mismatch error does.
            warn!("blob {} became unreadable mid-upload: {e}", path.display());
            return rb.body(Vec::new());
        }
    };
    pr.set_position(offset);
    let pr = pr.clone();
    let stream = tokio_util::io::ReaderStream::new(tokio::io::AsyncReadExt::take(file, len))
        .inspect(move |chunk| {
            if let Ok(c) = chunk {
                pr.inc(c.len() as u64);
            }
        });
    rb.body(reqwest::Body::wrap_stream(stream))
}

fn push_auth_hint(status: StatusCode, had_credential: bool) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN if !had_credential => {
            " — no credentials were found; run `docker login` (or `podman login`) for this registry"
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            " — the stored credentials were rejected or lack push permission \
             (for ghcr.io, the token needs the `write:packages` scope)"
        }
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_name() {
        let r = Reference::parse("debian").unwrap();
        assert_eq!(r.registry, "docker.io");
        assert_eq!(r.repository, "library/debian");
        assert_eq!(r.tag, "latest");
    }

    #[test]
    fn parses_tag() {
        let r = Reference::parse("debian:bookworm-slim").unwrap();
        assert_eq!(r.repository, "library/debian");
        assert_eq!(r.tag, "bookworm-slim");
    }

    #[test]
    fn parses_custom_registry() {
        let r = Reference::parse("ghcr.io/jdx/mise:v1").unwrap();
        assert_eq!(r.registry, "ghcr.io");
        assert_eq!(r.repository, "jdx/mise");
        assert_eq!(r.tag, "v1");
    }

    #[test]
    fn parses_digest_reference() {
        let digest = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let r = Reference::parse(&format!("ubuntu@{digest}")).unwrap();
        assert_eq!(r.registry, "docker.io");
        assert_eq!(r.repository, "library/ubuntu");
        assert_eq!(r.tag, digest);
    }

    #[test]
    fn registry_host_strips_port_and_brackets() {
        assert_eq!(registry_host("registry.lan:5000"), "registry.lan");
        assert_eq!(registry_host("registry.lan"), "registry.lan");
        assert_eq!(registry_host("[::1]:5000"), "::1");
        assert_eq!(registry_host("10.0.0.8:5000"), "10.0.0.8");
    }

    #[test]
    fn insecure_registry_entries_match_exact_or_bare_host() {
        let entries = vec!["registry.lan:5000".to_string(), "10.0.0.8".to_string()];
        // exact host:port entry
        assert!(is_insecure_registry_in("registry.lan:5000", &entries));
        // bare-host entry matches any port on that host
        assert!(is_insecure_registry_in("10.0.0.8:5000", &entries));
        assert!(is_insecure_registry_in("10.0.0.8", &entries));
        // a host:port entry does not cover other ports
        assert!(!is_insecure_registry_in("registry.lan:9999", &entries));
        assert!(!is_insecure_registry_in("ghcr.io", &entries));
        // loopback needs no entry
        assert!(is_insecure_registry_in("localhost:5000", &[]));
    }

    #[test]
    fn loopback_registries_use_http() {
        assert!(is_loopback_registry("localhost:5000"));
        assert!(is_loopback_registry("127.0.0.1:5000"));
        assert!(is_loopback_registry("[::1]:5000"));
        assert!(!is_loopback_registry("ghcr.io"));
        assert!(!is_loopback_registry("registry.example.com:5000"));
        assert_eq!(
            Reference::parse("localhost:5000/me/dev:v1")
                .unwrap()
                .registry_url(),
            "http://localhost:5000"
        );
        assert_eq!(
            Reference::parse("ghcr.io/me/dev:v1")
                .unwrap()
                .registry_url(),
            "https://ghcr.io"
        );
    }

    #[test]
    fn parses_bearer_challenge() {
        let www = r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io""#;
        match parse_auth_challenge(www) {
            Some(AuthChallenge::Bearer { realm, service }) => {
                assert_eq!(realm, "https://auth.docker.io/token");
                assert_eq!(service.as_deref(), Some("registry.docker.io"));
            }
            _ => panic!("expected bearer challenge"),
        }
    }

    #[test]
    fn parses_basic_challenge() {
        assert!(matches!(
            parse_auth_challenge(r#"Basic realm="registry""#),
            Some(AuthChallenge::Basic)
        ));
    }

    #[test]
    fn bearer_challenge_without_realm_is_none() {
        assert!(parse_auth_challenge("Bearer service=\"x\"").is_none());
        assert!(parse_auth_challenge("Negotiate").is_none());
    }

    #[test]
    fn bearer_challenge_preserves_commas_inside_quotes() {
        // A realm query string and an echoed scope both contain commas that a
        // naive split(',') would truncate at.
        let www = r#"Bearer realm="https://auth.example.com/token?a=1,b=2",service="reg,istry",scope="repository:me/app:pull,push""#;
        match parse_auth_challenge(www) {
            Some(AuthChallenge::Bearer { realm, service }) => {
                assert_eq!(realm, "https://auth.example.com/token?a=1,b=2");
                assert_eq!(service.as_deref(), Some("reg,istry"));
            }
            _ => panic!("expected bearer challenge"),
        }
    }

    #[test]
    fn challenge_params_handle_unquoted_and_spaced_values() {
        let params = parse_challenge_params(r#" realm=https://x/token , service="y" "#);
        assert_eq!(
            params[0],
            ("realm".to_string(), "https://x/token".to_string())
        );
        assert_eq!(params[1], ("service".to_string(), "y".to_string()));
    }

    #[test]
    fn parses_digest_reference_with_registry() {
        let digest = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let r = Reference::parse(&format!("ghcr.io/foo/bar@{digest}")).unwrap();
        assert_eq!(r.registry, "ghcr.io");
        assert_eq!(r.repository, "foo/bar");
        assert_eq!(r.tag, digest);
    }
}
