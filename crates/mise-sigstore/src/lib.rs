use std::path::Path;

use async_trait::async_trait;
use base64::Engine;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sigstore_verify::VerificationPolicy;
use sigstore_verify::trust_root::{DEFAULT_TUF_URL, TrustedRoot, TufConfig};
use sigstore_verify::types::{Bundle, DerPublicKey, SignatureBytes};
use thiserror::Error;
use tokio::io::AsyncReadExt;

const GITHUB_API_URL: &str = "https://api.github.com";
const USER_AGENT_VALUE: &str = "mise-sigstore/0.1.0";

// Embedded Sigstore TUF root used to bootstrap trust. We bundle a recent root
// (v12) instead of relying on `sigstore_verify::trust_root::TrustedRoot::production()`,
// because the upstream embedded v1 root (still shipped as of
// sigstore-trust-root 0.6.6) fails signature verification under `tough` — the
// timezone-offset `expires` field is normalized to UTC `Z` on re-serialization,
// so the canonical JSON the signature was computed over no longer matches.
// Bundling v12 lets the TUF chain walk forward to whatever root version the
// CDN currently serves.
const EMBEDDED_TUF_ROOT: &[u8] = include_bytes!("../data/tuf_root.json");

#[derive(Debug, Error)]
pub enum AttestationError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Verification failed: {0}")]
    Verification(String),
    #[error("Unsupported attestation format: {0}")]
    UnsupportedFormat(String),
    #[error("No attestations found")]
    NoAttestations,
    #[error("Invalid digest format: {0}")]
    InvalidDigest(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Sigstore error: {0}")]
    Sigstore(String),
}

impl From<sigstore_verify::Error> for AttestationError {
    fn from(err: sigstore_verify::Error) -> Self {
        AttestationError::Sigstore(err.to_string())
    }
}

impl From<sigstore_verify::types::Error> for AttestationError {
    fn from(err: sigstore_verify::types::Error) -> Self {
        AttestationError::Sigstore(err.to_string())
    }
}

impl From<sigstore_verify::trust_root::Error> for AttestationError {
    fn from(err: sigstore_verify::trust_root::Error) -> Self {
        AttestationError::Sigstore(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AttestationError>;

#[derive(Debug, Clone)]
pub struct ArtifactRef {
    digest: String,
}

impl ArtifactRef {
    pub fn from_digest(digest: &str) -> Self {
        if digest.contains(':') {
            Self {
                digest: digest.to_string(),
            }
        } else {
            Self {
                digest: format!("sha256:{digest}"),
            }
        }
    }
}

#[async_trait]
pub trait AttestationSource {
    async fn fetch_attestations(&self, artifact: &ArtifactRef) -> Result<Vec<Attestation>>;
}

pub mod sources {
    pub use crate::{ArtifactRef, AttestationSource};

    pub mod github {
        pub use crate::GitHubSource;
    }
}

#[derive(Debug, Clone)]
pub struct GitHubSource {
    client: AttestationClient,
    owner: String,
    repo: String,
}

impl GitHubSource {
    pub fn new(owner: &str, repo: &str, token: Option<&str>) -> Result<Self> {
        let mut builder = AttestationClient::builder();
        if let Some(token) = token {
            builder = builder.github_token(token);
        }
        Ok(Self {
            client: builder.build()?,
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    pub fn with_base_url(
        owner: &str,
        repo: &str,
        token: Option<&str>,
        base_url: &str,
    ) -> Result<Self> {
        let mut builder = AttestationClient::builder().base_url(base_url);
        if let Some(token) = token {
            builder = builder.github_token(token);
        }
        Ok(Self {
            client: builder.build()?,
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }
}

#[async_trait]
impl AttestationSource for GitHubSource {
    async fn fetch_attestations(&self, artifact: &ArtifactRef) -> Result<Vec<Attestation>> {
        self.client
            .fetch_attestations(FetchParams {
                owner: self.owner.clone(),
                repo: Some(format!("{}/{}", self.owner, self.repo)),
                digest: artifact.digest.clone(),
                limit: 30,
                predicate_type: None,
            })
            .await
    }
}

#[derive(Debug, Clone)]
pub struct AttestationClient {
    client: reqwest::Client,
    base_url: String,
    github_token: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AttestationClientBuilder {
    base_url: Option<String>,
    github_token: Option<String>,
}

impl AttestationClientBuilder {
    pub fn base_url(mut self, url: &str) -> Self {
        self.base_url = Some(url.trim_end_matches('/').to_string());
        self
    }

    pub fn github_token(mut self, token: &str) -> Self {
        self.github_token = Some(token.to_string());
        self
    }

    pub fn build(self) -> Result<AttestationClient> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(AttestationClient {
            client,
            base_url: self.base_url.unwrap_or_else(|| GITHUB_API_URL.to_string()),
            github_token: self.github_token,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct FetchParams {
    pub owner: String,
    pub repo: Option<String>,
    pub digest: String,
    pub limit: usize,
    pub predicate_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttestationsResponse {
    attestations: Vec<Attestation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Attestation {
    bundle: Option<serde_json::Value>,
    bundle_url: Option<String>,
}

impl AttestationClient {
    pub fn builder() -> AttestationClientBuilder {
        AttestationClientBuilder::default()
    }

    fn github_headers(&self, url: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let base_with_slash = format!("{}/", self.base_url);
        if url == self.base_url || url.starts_with(&base_with_slash) {
            if let Some(token) = &self.github_token {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {token}"))
                        .map_err(|e| AttestationError::Api(e.to_string()))?,
                );
            }
            headers.insert(
                "x-github-api-version",
                HeaderValue::from_static("2022-11-28"),
            );
        }
        Ok(headers)
    }

    pub async fn fetch_attestations(&self, params: FetchParams) -> Result<Vec<Attestation>> {
        let url = if let Some(repo) = &params.repo {
            format!(
                "{}/repos/{repo}/attestations/{}",
                self.base_url, params.digest
            )
        } else {
            format!(
                "{}/orgs/{}/attestations/{}",
                self.base_url, params.owner, params.digest
            )
        };

        let mut query_params = vec![("per_page", params.limit.to_string())];
        if let Some(predicate_type) = &params.predicate_type {
            query_params.push(("predicate_type", predicate_type.clone()));
        }
        let url = reqwest::Url::parse_with_params(&url, query_params)
            .map_err(|e| AttestationError::Api(format!("Invalid GitHub attestations URL: {e}")))?;

        let response = self
            .client
            .get(url.clone())
            .headers(self.github_headers(url.as_str())?)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(vec![]);
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AttestationError::Api(format!(
                "GitHub API returned {status}: {body}"
            )));
        }

        let response: AttestationsResponse = response.json().await?;
        let mut attestations = Vec::new();
        for attestation in response.attestations {
            if attestation.bundle.is_some() {
                attestations.push(attestation);
            } else if let Some(bundle_url) = &attestation.bundle_url {
                let bundle = self.fetch_bundle_url(bundle_url).await?;
                attestations.push(Attestation {
                    bundle: Some(bundle),
                    bundle_url: Some(bundle_url.clone()),
                });
            }
        }
        Ok(attestations)
    }

    async fn fetch_bundle_url(&self, bundle_url: &str) -> Result<serde_json::Value> {
        let response = self
            .client
            .get(bundle_url)
            .headers(self.github_headers(bundle_url)?)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AttestationError::Api(format!(
                "bundle URL returned {}",
                response.status()
            )));
        }
        if is_snappy_content_type(&response) {
            let bytes = response.bytes().await?;
            let decompressed = snap::raw::Decoder::new()
                .decompress_vec(&bytes)
                .map_err(|e| AttestationError::Api(format!("Snappy decompression failed: {e}")))?;
            serde_json::from_slice(&decompressed).map_err(AttestationError::Json)
        } else {
            response.json().await.map_err(AttestationError::Http)
        }
    }
}

pub async fn verify_github_attestation(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    token: Option<&str>,
    signer_workflow: Option<&str>,
) -> Result<bool> {
    verify_github_attestation_inner(artifact_path, owner, repo, token, signer_workflow, None).await
}

pub async fn verify_github_attestation_with_base_url(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    token: Option<&str>,
    signer_workflow: Option<&str>,
    base_url: &str,
) -> Result<bool> {
    verify_github_attestation_inner(
        artifact_path,
        owner,
        repo,
        token,
        signer_workflow,
        Some(base_url),
    )
    .await
}

async fn verify_github_attestation_inner(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    token: Option<&str>,
    signer_workflow: Option<&str>,
    base_url: Option<&str>,
) -> Result<bool> {
    let mut builder = AttestationClient::builder();
    if let Some(token) = token {
        builder = builder.github_token(token);
    }
    if let Some(base_url) = base_url {
        builder = builder.base_url(base_url);
    }
    let client = builder.build()?;
    let digest = calculate_file_digest_async(artifact_path).await?;
    let attestations = client
        .fetch_attestations(FetchParams {
            owner: owner.to_string(),
            repo: Some(format!("{owner}/{repo}")),
            digest: format!("sha256:{digest}"),
            limit: 30,
            predicate_type: None,
        })
        .await?;

    if attestations.is_empty() {
        return Err(AttestationError::NoAttestations);
    }

    let artifact = tokio::fs::read(artifact_path).await?;
    let trusted_root = production_trusted_root().await?;
    verify_attestation_bundles(&attestations, &artifact, signer_workflow, &trusted_root)
}

pub async fn verify_cosign_signature(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
) -> Result<bool> {
    let content = tokio::fs::read_to_string(sig_or_bundle_path).await?;
    let bundle = Bundle::from_json(&content)?;
    let artifact = tokio::fs::read(artifact_path).await?;
    let trusted_root = production_trusted_root().await?;
    verify_bundle(&artifact, &bundle, None, true, &trusted_root)?;
    Ok(true)
}

pub async fn verify_cosign_signature_with_key(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
    public_key_path: &Path,
) -> Result<bool> {
    let key_pem = tokio::fs::read_to_string(public_key_path).await?;
    let public_key = DerPublicKey::from_pem(&key_pem)?;
    let trusted_root = production_trusted_root().await?;

    let bundle = tokio::fs::read_to_string(sig_or_bundle_path)
        .await
        .ok()
        .and_then(|content| Bundle::from_json(&content).ok());
    if let Some(bundle) = bundle {
        let artifact = tokio::fs::read(artifact_path).await?;
        let result = sigstore_verify::verify_with_key(
            artifact.as_slice(),
            &bundle,
            &public_key,
            &trusted_root,
        )?;
        if !result.success {
            return Err(AttestationError::Verification(
                "sigstore verification returned false".to_string(),
            ));
        }
        return Ok(true);
    }

    let artifact = tokio::fs::read(artifact_path).await?;
    let signature = read_cosign_signature(sig_or_bundle_path).await?;
    verify_raw_signature(&artifact, &signature, &public_key)?;
    Ok(true)
}

pub async fn verify_slsa_provenance(
    artifact_path: &Path,
    provenance_path: &Path,
    min_level: u8,
) -> Result<bool> {
    let artifact = tokio::fs::read(artifact_path).await?;
    let content = tokio::fs::read_to_string(provenance_path).await?;
    let trusted_root = production_trusted_root().await?;
    let mut errors = Vec::new();

    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        match Bundle::from_json(line) {
            Ok(bundle) => match verify_bundle(&artifact, &bundle, None, true, &trusted_root) {
                Ok(()) => match verify_min_slsa_level(&bundle, min_level) {
                    Ok(()) => return Ok(true),
                    Err(e) => errors.push(e),
                },
                Err(e) => errors.push(e),
            },
            Err(e) => errors.push(AttestationError::UnsupportedFormat(e.to_string())),
        }
    }

    if content.trim_start().starts_with('{') {
        match Bundle::from_json(content.trim()) {
            Ok(bundle) => {
                match verify_bundle(&artifact, &bundle, None, true, &trusted_root)
                    .and_then(|_| verify_min_slsa_level(&bundle, min_level))
                {
                    Ok(()) => return Ok(true),
                    Err(e) => errors.push(e),
                }
            }
            Err(e) => errors.push(AttestationError::UnsupportedFormat(e.to_string())),
        }
    }

    collapse_slsa_errors(errors, || {
        "File does not contain valid attestations or SLSA provenance".to_string()
    })
}

fn collapse_slsa_errors(
    errors: Vec<AttestationError>,
    default: impl FnOnce() -> String,
) -> Result<bool> {
    let unsupported_format = errors
        .iter()
        .all(|error| matches!(error, AttestationError::UnsupportedFormat(_)));
    let message = join_error_strings(
        errors.into_iter().map(|error| error.to_string()).collect(),
        default,
    );
    Err(if unsupported_format {
        AttestationError::UnsupportedFormat(message)
    } else {
        AttestationError::Verification(message)
    })
}

fn join_error_strings(errors: Vec<String>, default: impl FnOnce() -> String) -> String {
    let mut errors = errors
        .into_iter()
        .filter(|error| !error.trim().is_empty())
        .collect::<Vec<_>>();
    errors.dedup();
    if errors.is_empty() {
        default()
    } else {
        errors.join("; ")
    }
}

fn verify_attestation_bundles(
    attestations: &[Attestation],
    artifact: &[u8],
    signer_workflow: Option<&str>,
    trusted_root: &TrustedRoot,
) -> Result<bool> {
    let mut errors = Vec::new();
    for attestation in attestations {
        let Some(bundle_value) = &attestation.bundle else {
            continue;
        };
        let bundle = match serde_json::from_value::<Bundle>(bundle_value.clone()) {
            Ok(bundle) => bundle,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };
        match verify_bundle(artifact, &bundle, signer_workflow, true, trusted_root) {
            Ok(()) => return Ok(true),
            Err(e) => errors.push(e.to_string()),
        }
    }

    Err(AttestationError::Verification(join_error_strings(
        errors,
        || "No valid attestations found".to_string(),
    )))
}

fn is_snappy_content_type(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|content_type| content_type.split(';').next())
        .is_some_and(|content_type| content_type.trim() == "application/x-snappy")
}

fn verify_bundle(
    artifact: &[u8],
    bundle: &Bundle,
    signer_workflow: Option<&str>,
    skip_tlog: bool,
    trusted_root: &TrustedRoot,
) -> Result<()> {
    let mut policy = VerificationPolicy::default();
    if skip_tlog {
        policy = policy.skip_tlog();
    }
    let result = sigstore_verify::verify(artifact, bundle, &policy, trusted_root)?;
    if !result.success {
        return Err(AttestationError::Verification(
            "sigstore verification returned false".to_string(),
        ));
    }

    verify_signer_workflow_identity(result.identity.as_deref(), signer_workflow)?;

    Ok(())
}

async fn production_trusted_root() -> Result<TrustedRoot> {
    let config = TufConfig::custom(DEFAULT_TUF_URL, EMBEDDED_TUF_ROOT);
    Ok(TrustedRoot::from_tuf(config).await?)
}

fn verify_signer_workflow_identity(
    identity: Option<&str>,
    signer_workflow: Option<&str>,
) -> Result<()> {
    let Some(expected) = signer_workflow else {
        return Ok(());
    };
    let Some(identity) = identity.filter(|identity| !identity.is_empty()) else {
        return Err(AttestationError::Verification(format!(
            "Workflow verification failed: expected '{expected}', found no certificate identity"
        )));
    };
    if !identity.contains(expected) {
        return Err(AttestationError::Verification(format!(
            "Workflow verification failed: expected '{expected}', found certificate identity: {identity:?}"
        )));
    }
    Ok(())
}

fn verify_min_slsa_level(bundle: &Bundle, min_level: u8) -> Result<()> {
    let payload = match &bundle.content {
        sigstore_verify::types::SignatureContent::DsseEnvelope(envelope) => {
            envelope.decode_payload()
        }
        _ => {
            return Err(AttestationError::UnsupportedFormat(
                "SLSA provenance must be a DSSE envelope".to_string(),
            ));
        }
    };
    let statement: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
        AttestationError::Verification(format!("Failed to parse SLSA payload: {e}"))
    })?;
    let predicate_type = statement
        .get("predicateType")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !predicate_type.starts_with("https://slsa.dev/provenance/") {
        return Err(AttestationError::UnsupportedFormat(format!(
            "Not an SLSA provenance predicate: {predicate_type}"
        )));
    }
    if min_level > 1 {
        return Err(AttestationError::Verification(format!(
            "SLSA level {min_level} verification is not supported by the native adapter"
        )));
    }
    Ok(())
}

async fn read_cosign_signature(sig_path: &Path) -> Result<Vec<u8>> {
    let bytes = tokio::fs::read(sig_path).await?;
    let trimmed = String::from_utf8_lossy(&bytes).trim().to_string();
    if let Some(decoded) = base64::engine::general_purpose::STANDARD
        .decode(trimmed.as_bytes())
        .ok()
        .filter(|_| !trimmed.is_empty())
    {
        return Ok(decoded);
    }
    Ok(bytes)
}

fn verify_raw_signature(
    artifact: &[u8],
    signature: &[u8],
    public_key: &DerPublicKey,
) -> Result<()> {
    use sigstore_verify::crypto::{KeyType, SigningScheme, detect_key_type, verify_signature};

    let scheme = match detect_key_type(public_key) {
        KeyType::Ed25519 => SigningScheme::Ed25519,
        KeyType::EcdsaP256 => SigningScheme::EcdsaP256Sha256,
        KeyType::Unknown => {
            return Err(AttestationError::Verification(
                "unsupported or unrecognized public key type".to_string(),
            ));
        }
    };
    let signature = SignatureBytes::from_bytes(signature);
    verify_signature(public_key, artifact, &signature, scheme)
        .map_err(|e| AttestationError::Verification(format!("signature verification failed: {e}")))
}

async fn calculate_file_digest_async(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signer_workflow_requires_identity() {
        let err = verify_signer_workflow_identity(None, Some(".github/workflows/release.yml"))
            .unwrap_err()
            .to_string();

        assert!(err.contains("found no certificate identity"));
    }

    #[test]
    fn signer_workflow_rejects_mismatch() {
        let err = verify_signer_workflow_identity(
            Some("https://github.com/jdx/mise/.github/workflows/ci.yml@refs/tags/v1.0.0"),
            Some(".github/workflows/release.yml"),
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("Workflow verification failed"));
    }

    #[test]
    fn signer_workflow_accepts_match() {
        verify_signer_workflow_identity(
            Some("https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v1.0.0"),
            Some(".github/workflows/release.yml"),
        )
        .unwrap();
    }

    #[test]
    fn signer_workflow_rejects_expected_containing_identity() {
        let err = verify_signer_workflow_identity(
            Some(".github/workflows/release.yml"),
            Some("https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v1.0.0"),
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("Workflow verification failed"));
    }
}
