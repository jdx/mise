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
        // docker.io is special — the distribution API is served from
        // registry-1.docker.io even though the canonical name is docker.io.
        let host = if self.registry == "docker.io" {
            "registry-1.docker.io"
        } else {
            &self.registry
        };
        // Loopback registries (localhost:5000 etc.) serve plain HTTP — the
        // same insecure-by-default convention docker applies to 127.0.0.0/8.
        let scheme = if is_loopback_registry(host) {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{host}")
    }
}

/// True when `registry` (a `host[:port]` / `[v6]:port` string) points at a
/// loopback address, in which case the distribution API is spoken over
/// plain HTTP.
fn is_loopback_registry(registry: &str) -> bool {
    let host = if let Some(rest) = registry.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        registry.rsplit_once(':').map_or(registry, |(h, _)| h)
    };
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
    let lower = www_auth.trim_start().to_ascii_lowercase();
    if lower.starts_with("basic") {
        return Some(AuthChallenge::Basic);
    }
    if !lower.starts_with("bearer") {
        return None;
    }
    // WWW-Authenticate: Bearer realm="https://auth.docker.io/token",service="registry.docker.io"
    let mut realm: Option<String> = None;
    let mut service: Option<String> = None;
    for part in www_auth.trim_start()[6..].split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("realm=") {
            realm = Some(rest.trim_matches('"').to_string());
        } else if let Some(rest) = part.strip_prefix("service=") {
            service = Some(rest.trim_matches('"').to_string());
        }
    }
    realm.map(|realm| AuthChallenge::Bearer { realm, service })
}

/// Determine the `Authorization` header to use against a registry, by probing
/// `GET /v2/` and answering its challenge.
///
///  - 200 → no auth needed (`None`)
///  - 401 + `Bearer` challenge → token fetch (anonymous or with credentials)
///  - 401 + `Basic` challenge → credentials passed through as Basic
///
/// `actions` is the scope verb list (`"pull"` or `"pull,push"`).
async fn authorization_for(
    r: &Reference,
    actions: &str,
    credential: Option<&Credential>,
) -> Result<Option<String>> {
    let probe_url = format!("{}/v2/", r.registry_url());
    let resp = HTTP
        .get_async_with_headers_allow_error_status(&probe_url, &HeaderMap::new())
        .await
        .wrap_err_with(|| format!("probing {probe_url}"))?;
    if resp.status().is_success() {
        return Ok(None);
    }
    if resp.status() != StatusCode::UNAUTHORIZED {
        bail!(
            "unexpected status {} probing {probe_url}",
            resp.status().as_u16()
        );
    }
    let www_auth = resp
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    match parse_auth_challenge(&www_auth) {
        Some(AuthChallenge::Bearer { realm, service }) => {
            let token = fetch_bearer_token(
                &realm,
                service.as_deref(),
                &r.repository,
                actions,
                credential,
            )
            .await?;
            Ok(token.map(|t| format!("Bearer {t}")))
        }
        Some(AuthChallenge::Basic) => match credential {
            Some(c) => Ok(Some(c.basic_auth_header())),
            None => bail!(
                "registry {} requires Basic auth but no credentials were found; \
                 run `docker login {}` (or `podman login`) first",
                r.registry,
                r.registry
            ),
        },
        None => bail!(
            "registry {} returned an unsupported auth challenge: {www_auth:?}",
            r.registry
        ),
    }
}

/// Fetch a bearer token from a registry's token endpoint. Anonymous when
/// `credential` is `None` (public pulls); authenticated via Basic auth on the
/// token request otherwise. Docker Hub identity tokens (from Docker Desktop
/// logins) use the OAuth2 refresh-token POST flow instead.
async fn fetch_bearer_token(
    realm: &str,
    service: Option<&str>,
    repository: &str,
    actions: &str,
    credential: Option<&Credential>,
) -> Result<Option<String>> {
    let scope = format!("repository:{repository}:{actions}");

    if let Some(c) = credential
        && c.username == "<token>"
    {
        // OAuth2 identity-token flow.
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
        q.append_pair("scope", &scope);
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

fn auth_headers(authorization: Option<&str>, accept: &[&str]) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if let Some(a) = authorization {
        headers.insert("Authorization", HeaderValue::from_str(a)?);
    }
    if !accept.is_empty() {
        headers.insert("Accept", HeaderValue::from_str(&accept.join(", "))?);
    }
    Ok(headers)
}

async fn get_with_token<T: serde::de::DeserializeOwned>(
    url: &str,
    authorization: Option<&str>,
    accept: &[&str],
) -> Result<(T, HeaderMap)> {
    let headers = auth_headers(authorization, accept)?;
    let (body, h) = HTTP
        .json_headers_with_headers::<T, _>(url, &headers)
        .await?;
    Ok((body, h))
}

async fn get_bytes_with_token(
    url: &str,
    authorization: Option<&str>,
    accept: &[&str],
) -> Result<Vec<u8>> {
    let headers = auth_headers(authorization, accept)?;
    let bytes = HTTP.get_bytes_with_headers(url, &headers).await?;
    Ok(bytes.as_ref().to_vec())
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

    // Use stored credentials when the user has them (private base images);
    // fall back to anonymous tokens otherwise.
    let credential = crate::oci::auth::resolve_credential(&r.registry)?;
    let token = authorization_for(&r, "pull", credential.as_ref()).await?;

    // Try OCI/Docker manifest or an index (multi-arch).
    let (body, headers) =
        get_with_token::<serde_json::Value>(&manifest_url, token.as_deref(), &accept)
            .await
            .wrap_err_with(|| format!("fetching manifest for {reference}"))?;

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let manifest = resolve_manifest(
        body,
        &r,
        base_url.as_str(),
        token.as_deref(),
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
    let config_bytes = get_bytes_with_token(&config_url, token.as_deref(), &[]).await?;
    // Preserve the byte-level digest by writing under the exact digest name.
    layout.write_blob_with_digest(&manifest.config.digest, &config_bytes)?;

    for layer in &manifest.layers {
        let layer_url = format!("{base_url}/v2/{}/blobs/{}", r.repository, layer.digest);
        let blob_path = layout.blob_path(&layer.digest);
        if blob_path.exists() {
            continue;
        }
        let bytes = get_bytes_with_token(&layer_url, token.as_deref(), &[]).await?;
        layout.write_blob_with_digest(&layer.digest, &bytes)?;
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
    token: Option<&str>,
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
        let (body, _h) = get_with_token::<serde_json::Value>(&manifest_url, token, &accept).await?;
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
}

/// Push an OCI image layout directory to a registry reference.
///
/// Uploads only blobs the registry doesn't already have (HEAD check per
/// blob), then PUTs the manifest under the reference's tag (or digest).
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

    let credential = crate::oci::auth::resolve_credential(&r.registry)?;
    if credential.is_none() {
        // Not fatal — local registries accept anonymous pushes — but worth
        // surfacing before a 401 does.
        warn!(
            "no registry credentials found for {} — pushing anonymously. \
             Run `docker login {}` (or `podman login`) if the push is rejected.",
            r.registry, r.registry
        );
    }
    let authorization = authorization_for(&r, "pull,push", credential.as_ref()).await?;
    let pusher = Pusher {
        base_url: r.registry_url(),
        repository: r.repository.clone(),
        authorization,
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

    let mut uploaded = 0;
    let mut skipped = 0;
    for desc in blobs {
        crate::oci::layout::validate_sha256_digest(&desc.digest)?;
        if pusher.blob_exists(&desc.digest).await? {
            debug!("blob {} already present, skipping", desc.digest);
            skipped += 1;
            continue;
        }
        info!(
            "uploading {} ({:.1} MiB)",
            desc.digest,
            desc.size as f64 / (1024.0 * 1024.0)
        );
        pusher
            .upload_blob(&layout.blob_path(&desc.digest), &desc.digest, desc.size)
            .await
            .wrap_err_with(|| format!("uploading blob {}", desc.digest))?;
        uploaded += 1;
    }

    pusher
        .put_manifest(&r.tag, &manifest_desc.media_type, &manifest_bytes)
        .await
        .wrap_err_with(|| format!("pushing manifest to {reference}"))?;

    Ok(PushSummary {
        manifest_digest: manifest_desc.digest.clone(),
        uploaded,
        skipped,
    })
}

struct Pusher {
    base_url: String,
    repository: String,
    authorization: Option<String>,
}

impl Pusher {
    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.authorization {
            Some(a) => req.header("Authorization", a),
            None => req,
        }
    }

    async fn blob_exists(&self, digest: &str) -> Result<bool> {
        let url = format!("{}/v2/{}/blobs/{digest}", self.base_url, self.repository);
        let resp = self
            .apply_auth(HTTP.reqwest().head(&url))
            .send()
            .await
            .wrap_err_with(|| format!("HEAD {url}"))?;
        match resp.status() {
            StatusCode::OK => Ok(true),
            // Treat auth failures on HEAD as "not there" — the subsequent
            // upload will surface a clearer 401/403 with context.
            StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Ok(false),
            s => bail!("unexpected status {s} from HEAD {url}"),
        }
    }

    async fn upload_blob(&self, path: &Path, digest: &str, size: u64) -> Result<()> {
        // 1. Open an upload session.
        let start_url = format!("{}/v2/{}/blobs/uploads/", self.base_url, self.repository);
        let resp = self
            .apply_auth(HTTP.reqwest().post(&start_url))
            .header("Content-Length", "0")
            .send()
            .await
            .wrap_err_with(|| format!("POST {start_url}"))?;
        let status = resp.status();
        if status != StatusCode::ACCEPTED {
            bail!(
                "starting blob upload failed: {} {}{}",
                status.as_u16(),
                start_url,
                push_auth_hint(status, self.authorization.is_some()),
            );
        }
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| eyre::eyre!("registry returned no Location for blob upload"))?;
        let mut upload_url = if location.starts_with("http://") || location.starts_with("https://")
        {
            url::Url::parse(location)?
        } else {
            url::Url::parse(&format!("{}{}", self.base_url, location))?
        };
        // 2. Monolithic PUT with ?digest=…
        upload_url.query_pairs_mut().append_pair("digest", digest);
        let file = tokio::fs::File::open(path)
            .await
            .wrap_err_with(|| format!("opening blob {}", path.display()))?;
        let body = reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(file));
        let resp = self
            .apply_auth(HTTP.reqwest().put(upload_url.as_str()))
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", size)
            .body(body)
            .send()
            .await
            .wrap_err("PUT blob upload")?;
        let status = resp.status();
        if status != StatusCode::CREATED && status != StatusCode::ACCEPTED {
            let body = resp.text().await.unwrap_or_default();
            bail!(
                "blob upload failed: {}{}\n{}",
                status.as_u16(),
                push_auth_hint(status, self.authorization.is_some()),
                body.trim(),
            );
        }
        Ok(())
    }

    async fn put_manifest(&self, tag: &str, media_type: &str, bytes: &[u8]) -> Result<()> {
        let url = format!("{}/v2/{}/manifests/{tag}", self.base_url, self.repository);
        let resp = self
            .apply_auth(HTTP.reqwest().put(&url))
            .header("Content-Type", media_type)
            .body(bytes.to_vec())
            .send()
            .await
            .wrap_err_with(|| format!("PUT {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!(
                "manifest push failed: {} {url}{}\n{}",
                status.as_u16(),
                push_auth_hint(status, self.authorization.is_some()),
                body.trim(),
            );
        }
        Ok(())
    }
}

fn push_auth_hint(status: StatusCode, had_authorization: bool) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN if !had_authorization => {
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
    fn parses_digest_reference_with_registry() {
        let digest = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let r = Reference::parse(&format!("ghcr.io/foo/bar@{digest}")).unwrap();
        assert_eq!(r.registry, "ghcr.io");
        assert_eq!(r.repository, "foo/bar");
        assert_eq!(r.tag, digest);
    }
}
