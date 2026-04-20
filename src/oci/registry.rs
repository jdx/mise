//! Anonymous OCI Distribution Spec v2 client for pulling base images.
//!
//! Supports Docker Hub / GHCR / quay.io style references and anonymous token
//! auth (no login flow). Used by `mise oci build --from <ref>` to stream a
//! base image's layers into the output layout byte-for-byte so digests match.

use eyre::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;

use crate::http::HTTP;
use crate::oci::layout::ImageLayout;
use crate::oci::manifest::{
    Descriptor, ImageManifest, MEDIA_TYPE_DOCKER_MANIFEST, MEDIA_TYPE_DOCKER_MANIFEST_LIST,
    MEDIA_TYPE_OCI_INDEX, MEDIA_TYPE_OCI_MANIFEST,
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
    pub fn parse(s: &str) -> Result<Self> {
        let (name, tag) = match s.rsplit_once(':') {
            Some((n, t)) if !t.contains('/') => (n, t),
            _ => (s, "latest"),
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
            tag: tag.to_string(),
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
        format!("https://{host}")
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
}

/// Fetch an anonymous bearer token from a registry's auth endpoint if needed.
/// Many public images require a token even for anonymous pulls (Docker Hub
/// especially).
async fn fetch_anonymous_token(www_auth: &str, repository: &str) -> Result<Option<String>> {
    // WWW-Authenticate: Bearer realm="https://auth.docker.io/token",service="registry.docker.io"
    let mut realm: Option<String> = None;
    let mut service: Option<String> = None;
    for part in www_auth.trim_start_matches("Bearer ").split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("realm=") {
            realm = Some(rest.trim_matches('"').to_string());
        } else if let Some(rest) = part.strip_prefix("service=") {
            service = Some(rest.trim_matches('"').to_string());
        }
    }
    let Some(realm) = realm else { return Ok(None) };
    let scope = format!("repository:{repository}:pull");
    let mut url = url::Url::parse(&realm)?;
    {
        let mut q = url.query_pairs_mut();
        if let Some(s) = service {
            q.append_pair("service", &s);
        }
        q.append_pair("scope", &scope);
    }
    let resp: TokenResponse = HTTP.json(url.as_str()).await?;
    Ok(resp.token.or(resp.access_token))
}

async fn get_with_token<T: serde::de::DeserializeOwned>(
    url: &str,
    token: Option<&str>,
    accept: &[&str],
) -> Result<(T, HeaderMap)> {
    let mut headers = HeaderMap::new();
    if let Some(t) = token {
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {t}"))?,
        );
    }
    if !accept.is_empty() {
        headers.insert("Accept", HeaderValue::from_str(&accept.join(", "))?);
    }
    let (body, h) = HTTP
        .json_headers_with_headers::<T, _>(url, &headers)
        .await?;
    Ok((body, h))
}

async fn get_bytes_with_token(url: &str, token: Option<&str>, accept: &[&str]) -> Result<Vec<u8>> {
    let mut headers = HeaderMap::new();
    if let Some(t) = token {
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {t}"))?,
        );
    }
    if !accept.is_empty() {
        headers.insert("Accept", HeaderValue::from_str(&accept.join(", "))?);
    }
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

    let token = fetch_token_if_needed(&manifest_url, &r.repository).await?;

    // Try OCI/Docker manifest or an index (multi-arch).
    let (body, _headers) =
        get_with_token::<serde_json::Value>(&manifest_url, token.as_deref(), &accept)
            .await
            .wrap_err_with(|| format!("fetching manifest for {reference}"))?;

    let manifest = resolve_manifest(
        body,
        &r,
        base_url.as_str(),
        token.as_deref(),
        desired_platform,
    )
    .await?;

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

/// Fetch an anonymous bearer token for registries that require one. We skip
/// the HEAD probe because the shared HTTP client errors on 401 (hiding the
/// www-authenticate header); instead we just check the host and use the
/// known-good anonymous realm. Private images and registries beyond this set
/// are explicit follow-ups (see `--from` docs).
async fn fetch_token_if_needed(manifest_url: &str, repository: &str) -> Result<Option<String>> {
    let realm = if manifest_url.contains("registry-1.docker.io") {
        "Bearer realm=\"https://auth.docker.io/token\",service=\"registry.docker.io\""
    } else if manifest_url.contains("ghcr.io") {
        "Bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\""
    } else if manifest_url.contains("quay.io") {
        // quay.io publishes an anonymous token endpoint; many public repos
        // require it even for unauthenticated pulls.
        "Bearer realm=\"https://quay.io/v2/auth\",service=\"quay.io\""
    } else {
        return Ok(None);
    };
    fetch_anonymous_token(realm, repository).await
}

/// Given a manifest body (possibly an index with multiple architectures),
/// resolve to a concrete single-image manifest and return its parsed form.
async fn resolve_manifest(
    body: serde_json::Value,
    r: &Reference,
    base_url: &str,
    token: Option<&str>,
    desired_platform: Option<(&str, &str)>,
) -> Result<ImageManifest> {
    let media_type = body
        .get("mediaType")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();

    // If this is an index / manifest list, pick the right child manifest.
    if media_type == MEDIA_TYPE_OCI_INDEX || media_type == MEDIA_TYPE_DOCKER_MANIFEST_LIST {
        let manifests = body
            .get("manifests")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();
        let (arch, os) = desired_platform.unwrap_or((std::env::consts::ARCH, std::env::consts::OS));
        let arch = normalize_arch(arch);
        let os = normalize_os(os);
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

fn normalize_arch(a: &str) -> &str {
    match a {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn normalize_os(o: &str) -> &str {
    match o {
        "macos" => "linux", // container images are linux; macOS builds still want linux base
        other => other,
    }
}
