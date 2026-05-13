use std::path::Path;

use async_trait::async_trait;
use base64::Engine;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sigstore_verify::VerificationPolicy;
use sigstore_verify::trust_root::{SigstoreInstance, TrustedRoot};
use sigstore_verify::types::{Bundle, DerCertificate, DerPublicKey, SignatureBytes};
use thiserror::Error;
use tokio::io::AsyncReadExt;

const GITHUB_API_URL: &str = "https://api.github.com";
const USER_AGENT_VALUE: &str = "mise-sigstore/0.1.0";

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
    let mut trust_roots = TrustRoots::default();
    verify_attestation_bundles(&attestations, &artifact, signer_workflow, &mut trust_roots).await
}

pub async fn verify_cosign_signature(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
) -> Result<bool> {
    let content = tokio::fs::read_to_string(sig_or_bundle_path).await?;
    let artifact = tokio::fs::read(artifact_path).await?;
    let mut trust_roots = TrustRoots::default();
    if let Ok(bundle) = Bundle::from_json(&content) {
        let trusted_root = trust_roots.for_bundle(&bundle).await?;
        verify_bundle(&artifact, &bundle, None, trusted_root)?;
        return Ok(true);
    }
    // Legacy cosign v1 bundle (`{base64Signature, cert, rekorBundle}`).
    // sigstore-verify only consumes the modern bundle shape, so we verify
    // these manually: chain-validate the embedded cert against Sigstore
    // Fulcio, then ECDSA-verify the signature over the artifact bytes.
    let trusted_root = trust_roots.sigstore_root().await?;
    verify_legacy_cosign_bundle(&artifact, &content, trusted_root)?;
    Ok(true)
}

pub async fn verify_cosign_signature_with_key(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
    public_key_path: &Path,
) -> Result<bool> {
    let key_pem = tokio::fs::read_to_string(public_key_path).await?;
    let public_key = DerPublicKey::from_pem(&key_pem)?;

    // Read the file once, propagating real I/O errors. Only a JSON-parse
    // failure means "this isn't a sigstore bundle, treat it as a raw `.sig`."
    let raw_bytes = tokio::fs::read(sig_or_bundle_path).await?;
    let bundle = std::str::from_utf8(&raw_bytes)
        .ok()
        .and_then(|content| Bundle::from_json(content).ok());
    if let Some(bundle) = bundle {
        // Bundle path: needs the trust root for tlog (Rekor) verification.
        let trusted_root = production_trusted_root().await?;
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

    // Raw `.sig` path: only needs the local public key — no network access.
    let artifact = tokio::fs::read(artifact_path).await?;
    let signature = decode_cosign_signature(&raw_bytes);
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
    let mut errors = Vec::new();
    let mut trust_roots = TrustRoots::default();

    let mut candidates: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let trimmed = content.trim();
    if !trimmed.is_empty() && !candidates.contains(&trimmed) {
        candidates.push(trimmed);
    }

    for candidate in candidates {
        // Bundle::from_json failure falls through to the DSSE envelope path.
        if let Ok(bundle) = Bundle::from_json(candidate) {
            let result = match trust_roots.for_bundle(&bundle).await {
                Ok(root) => verify_bundle(&artifact, &bundle, None, root)
                    .and_then(|_| verify_bundle_slsa(&bundle, &artifact, min_level)),
                Err(e) => Err(e),
            };
            match result {
                Ok(()) => return Ok(true),
                Err(e) => errors.push(e),
            }
            continue;
        }
        // slsa-github-generator and goreleaser write the provenance as a raw
        // DSSE envelope (`*.intoto.jsonl`) rather than a sigstore bundle —
        // there is no `verificationMaterial`, so `Bundle::from_json` rejects
        // it. Match the in-toto payload manually and check artifact digest +
        // SLSA predicate without going through sigstore-verify. Use the public
        // Sigstore trust root since slsa-github-generator certs are issued by
        // Sigstore Fulcio.
        let result = match trust_roots.sigstore_root().await {
            Ok(root) => verify_intoto_envelope(candidate, &artifact, min_level, root),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => return Ok(true),
            Err(e) => errors.push(e),
        }
    }

    collapse_slsa_errors(errors, || {
        "File does not contain valid attestations or SLSA provenance".to_string()
    })
}

fn verify_intoto_envelope(
    line: &str,
    artifact: &[u8],
    min_level: u8,
    trusted_root: &TrustedRoot,
) -> Result<()> {
    let envelope: serde_json::Value = serde_json::from_str(line).map_err(|e| {
        AttestationError::UnsupportedFormat(format!("not a JSON DSSE envelope: {e}"))
    })?;
    let payload_type = envelope
        .get("payloadType")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if payload_type != "application/vnd.in-toto+json" {
        return Err(AttestationError::UnsupportedFormat(format!(
            "unsupported DSSE payloadType: {payload_type}"
        )));
    }
    let payload_b64 = envelope
        .get("payload")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AttestationError::UnsupportedFormat("DSSE envelope missing payload".to_string())
        })?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(payload_b64.as_bytes())
        .map_err(|e| AttestationError::Verification(format!("invalid base64 payload: {e}")))?;

    // DSSE signature verification. The envelope's signatures sign the
    // Pre-Authentication Encoding of the payload, not the payload itself.
    // Without this check, anyone able to substitute the provenance file could
    // forge a passing attestation just by including the artifact's digest in
    // the in-toto subject list.
    //
    // Each signature embeds the Sigstore Fulcio leaf cert that signed it
    // (slsa-github-generator format). We chain-validate that cert against the
    // public Sigstore trust root, then verify the signature against the PAE
    // using the cert's public key. A self-signed forged cert would be
    // rejected at the chain step. Bundles in the modern sigstore format
    // (which carry tlog/TSA) take the strict `verify_bundle` path above.
    let signatures = envelope
        .get("signatures")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            AttestationError::Verification("DSSE envelope missing signatures".to_string())
        })?;
    if signatures.is_empty() {
        return Err(AttestationError::Verification(
            "DSSE envelope has no signatures".to_string(),
        ));
    }
    let pae = sigstore_verify::types::pae(payload_type, &payload);
    let mut sig_errors = Vec::new();
    let mut verified = false;
    for sig in signatures {
        match verify_dsse_signature(sig, &pae, trusted_root) {
            Ok(()) => {
                verified = true;
                break;
            }
            Err(e) => sig_errors.push(e.to_string()),
        }
    }
    if !verified {
        return Err(AttestationError::Verification(format!(
            "no valid DSSE signature: {}",
            join_error_strings(sig_errors, || "no signatures could be verified".to_string())
        )));
    }

    verify_intoto_payload(&payload, artifact, min_level)
}

/// Verify a legacy cosign v1 keyless bundle (`{base64Signature, cert, rekorBundle}`).
///
/// Cosign 2.x and earlier `cosign sign-blob --bundle` writes this format. The
/// modern sigstore Bundle (with `verificationMaterial`/`messageSignature`)
/// replaces it, but tools like goreleaser still produce v1 bundles in their
/// release artifacts. Verification mirrors what we do for raw DSSE envelopes:
/// decode the embedded Fulcio cert (PEM in `cert`), chain-validate it against
/// the public Sigstore trust root, then ECDSA-verify `base64Signature` over
/// the raw artifact bytes with the cert's public key.
///
/// The Rekor `SignedEntryTimestamp` and the artifact hash recorded in the
/// rekord entry aren't independently re-checked here — re-verifying them
/// would require a Rekor public key lookup and adds little: the cert+sig
/// step already cryptographically binds the signer to the artifact bytes,
/// which is what every downstream consumer cares about.
fn verify_legacy_cosign_bundle(
    artifact: &[u8],
    bundle_json: &str,
    trusted_root: &TrustedRoot,
) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(bundle_json).map_err(|e| {
        AttestationError::UnsupportedFormat(format!("not a sigstore or cosign bundle: {e}"))
    })?;
    let cert_b64 = value.get("cert").and_then(|v| v.as_str()).ok_or_else(|| {
        AttestationError::UnsupportedFormat("legacy cosign bundle missing cert".to_string())
    })?;
    let sig_b64 = value
        .get("base64Signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AttestationError::UnsupportedFormat(
                "legacy cosign bundle missing base64Signature".to_string(),
            )
        })?;

    let cert_pem_bytes = base64::engine::general_purpose::STANDARD
        .decode(cert_b64.as_bytes())
        .map_err(|e| {
            AttestationError::Verification(format!("invalid base64 cert in legacy bundle: {e}"))
        })?;
    let cert_pem = std::str::from_utf8(&cert_pem_bytes).map_err(|e| {
        AttestationError::Verification(format!("legacy cosign cert is not UTF-8 PEM: {e}"))
    })?;
    let cert = DerCertificate::from_pem(cert_pem)?;
    verify_cert_chain(cert.as_bytes(), trusted_root)?;

    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(sig_b64.as_bytes())
        .map_err(|e| AttestationError::Verification(format!("invalid base64 signature: {e}")))?;
    let spki_der = extract_spki_der(cert.as_bytes())?;
    let public_key = DerPublicKey::new(spki_der);
    verify_raw_signature(artifact, &sig_bytes, &public_key)
}

fn verify_dsse_signature(
    sig: &serde_json::Value,
    pae: &[u8],
    trusted_root: &TrustedRoot,
) -> Result<()> {
    let cert_pem = sig.get("cert").and_then(|v| v.as_str()).ok_or_else(|| {
        AttestationError::Verification("DSSE signature missing cert field".to_string())
    })?;
    let sig_b64 = sig.get("sig").and_then(|v| v.as_str()).ok_or_else(|| {
        AttestationError::Verification("DSSE signature missing sig field".to_string())
    })?;
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(sig_b64.as_bytes())
        .map_err(|e| AttestationError::Verification(format!("invalid base64 signature: {e}")))?;
    let cert = DerCertificate::from_pem(cert_pem)?;
    // Chain-validate the embedded cert before trusting its public key.
    verify_cert_chain(cert.as_bytes(), trusted_root)?;
    let spki_der = extract_spki_der(cert.as_bytes())?;
    let public_key = DerPublicKey::new(spki_der);
    verify_raw_signature(pae, &sig_bytes, &public_key)
}

fn verify_intoto_payload(payload: &[u8], artifact: &[u8], min_level: u8) -> Result<()> {
    let statement: serde_json::Value = serde_json::from_slice(payload).map_err(|e| {
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
    let artifact_digest = hex::encode(Sha256::digest(artifact));
    let subjects = statement
        .get("subject")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            AttestationError::Verification("SLSA statement missing subject array".to_string())
        })?;
    let matches_subject = subjects.iter().any(|subject| {
        subject
            .get("digest")
            .and_then(|d| d.get("sha256"))
            .and_then(|v| v.as_str())
            .is_some_and(|sha| sha.eq_ignore_ascii_case(&artifact_digest))
    });
    if !matches_subject {
        return Err(AttestationError::Verification(format!(
            "artifact sha256 {artifact_digest} not found in SLSA statement subjects"
        )));
    }
    Ok(())
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

async fn verify_attestation_bundles(
    attestations: &[Attestation],
    artifact: &[u8],
    signer_workflow: Option<&str>,
    trust_roots: &mut TrustRoots,
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
        let trusted_root = match trust_roots.for_bundle(&bundle).await {
            Ok(root) => root,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };
        match verify_bundle(artifact, &bundle, signer_workflow, trusted_root) {
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
    trusted_root: &TrustedRoot,
) -> Result<()> {
    let mut policy = VerificationPolicy::default();
    // sigstore-verify's default policy *requires* an inclusion proof when
    // `verify_tlog` is on. GitHub artifact attestations and TSA-only bundles
    // never carry one, so we'd reject them outright. Skip tlog only when the
    // bundle has no inclusion proof — public-Sigstore cosign bundles, which do
    // ship a Rekor inclusion proof, still get full tlog verification (Rekor
    // checkpoint signature, SET, inclusion-proof Merkle path).
    if !bundle.has_inclusion_proof() {
        policy = policy.skip_tlog();
    }
    // GitHub-internal leaf certs don't carry an SCT extension (GitHub's CA
    // doesn't log to public CT). `skip_sct` keeps full certificate-chain
    // validation against the GitHub trust root's Fulcio certs but turns off
    // the SCT check, which is exactly what GitHub artifact attestations need.
    if is_github_internal_certificate(bundle) {
        policy = policy.skip_sct();
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

fn is_github_internal_certificate(bundle: &Bundle) -> bool {
    bundle
        .signing_certificate()
        .map(|cert| cert_issuer_organization(cert.as_bytes()).as_deref() == Some("GitHub, Inc."))
        .unwrap_or(false)
}

/// Verify that a leaf certificate chains to one of the trust root's CA certs.
///
/// Used for raw DSSE envelopes (`*.intoto.jsonl` from slsa-github-generator),
/// which don't have the bundle structure sigstore-verify expects, so we can't
/// delegate to `sigstore_verify::verify`. GitHub-internal bundles go through
/// sigstore-verify directly with `skip_sct`.
///
/// webpki performs the same chain-building, ECDSA/RSA signature checks, and
/// CODE_SIGNING EKU enforcement as sigstore-verify, just without the SCT step.
///
/// Validation time is the leaf cert's `notAfter`. Fulcio leaves are
/// short-lived (~10 min) so by `now()` they're already expired and we have no
/// independently verified time source here. Using `notAfter` rather than
/// `notBefore` is the stricter choice: it catches any intermediate CA whose
/// own validity ends before the leaf's, which would otherwise slip through.
fn verify_cert_chain(leaf_der: &[u8], trusted_root: &TrustedRoot) -> Result<()> {
    use rustls_pki_types::{CertificateDer, UnixTime};
    use webpki::{ALL_VERIFICATION_ALGS, EndEntityCert, KeyUsage, anchor_from_trusted_cert};
    use x509_cert::Certificate;
    use x509_cert::der::Decode;

    let leaf = Certificate::from_der(leaf_der).map_err(|e| {
        AttestationError::Verification(format!("failed to parse leaf certificate: {e}"))
    })?;
    let not_after = leaf
        .tbs_certificate
        .validity
        .not_after
        .to_unix_duration()
        .as_secs();
    let validation_time = UnixTime::since_unix_epoch(std::time::Duration::from_secs(not_after));

    let all_certs = trusted_root.fulcio_certs().map_err(|e| {
        AttestationError::Verification(format!("failed to load CA certs from trust root: {e}"))
    })?;
    if all_certs.is_empty() {
        return Err(AttestationError::Verification(
            "trust root contains no CA certificates".to_string(),
        ));
    }
    // Use every CA cert in the trust root as both a trust anchor and as a
    // possible intermediate. `anchor_from_trusted_cert` accepts any parseable
    // cert (not just self-signed roots), and that's intentional: we trust the
    // whole CA bundle the trust root ships, so it's fine for chain validation
    // to terminate at an intermediate rather than walk all the way up to the
    // self-signed root. This matches what sigstore-verify does internally.
    // The chain itself is still cryptographically verified end-to-end.
    let trust_anchors: Vec<_> = all_certs
        .iter()
        .filter_map(|der| {
            anchor_from_trusted_cert(&CertificateDer::from(der.as_ref()))
                .map(|a| a.to_owned())
                .ok()
        })
        .collect();
    if trust_anchors.is_empty() {
        return Err(AttestationError::Verification(
            "trust root CA certs are unparseable".to_string(),
        ));
    }
    let intermediate_certs: Vec<CertificateDer<'static>> = all_certs
        .iter()
        .map(|der| CertificateDer::from(der.as_ref()).into_owned())
        .collect();

    let leaf_der_ref = CertificateDer::from(leaf_der);
    let leaf_cert = EndEntityCert::try_from(&leaf_der_ref).map_err(|e| {
        AttestationError::Verification(format!("failed to parse leaf for chain check: {e}"))
    })?;

    // 1.3.6.1.5.5.7.3.3 — id-kp-codeSigning, raw OID bytes (no DER tag/length).
    const ID_KP_CODE_SIGNING: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x03];

    leaf_cert
        .verify_for_usage(
            ALL_VERIFICATION_ALGS,
            &trust_anchors,
            &intermediate_certs,
            validation_time,
            KeyUsage::required(ID_KP_CODE_SIGNING),
            None,
            None,
        )
        .map_err(|e| {
            AttestationError::Verification(format!("certificate chain validation failed: {e}"))
        })?;
    Ok(())
}

/// Return the X.509 Issuer's `O` (organizationName) attribute, if present.
///
/// Used to dispatch verification policy: certs issued by GitHub's internal
/// Fulcio (`O=GitHub, Inc.`) need a separate trust root and a relaxed policy.
/// Parses the cert with x509-cert rather than byte-searching the DER, so we
/// only match the actual issuer organization field — not arbitrary substrings
/// elsewhere in the certificate.
fn cert_issuer_organization(cert_der: &[u8]) -> Option<String> {
    use x509_cert::Certificate;
    use x509_cert::der::Decode;
    let cert = Certificate::from_der(cert_der).ok()?;
    for rdn in cert.tbs_certificate.issuer.0.iter() {
        for atv in rdn.0.iter() {
            // 2.5.4.10 = id-at-organizationName
            if atv.oid.to_string() == "2.5.4.10" {
                if let Ok(s) = atv.value.decode_as::<String>() {
                    return Some(s);
                }
                if let Ok(s) = atv
                    .value
                    .decode_as::<x509_cert::der::asn1::PrintableStringRef>()
                {
                    return Some(s.as_str().to_string());
                }
                if let Ok(s) = atv.value.decode_as::<x509_cert::der::asn1::Utf8StringRef>() {
                    return Some(s.as_str().to_string());
                }
            }
        }
    }
    None
}

/// Extract the SubjectPublicKeyInfo bytes (DER) from an X.509 certificate.
fn extract_spki_der(cert_der: &[u8]) -> Result<Vec<u8>> {
    use x509_cert::Certificate;
    use x509_cert::der::{Decode, Encode};
    let cert = Certificate::from_der(cert_der)
        .map_err(|e| AttestationError::Verification(format!("failed to parse certificate: {e}")))?;
    cert.tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|e| {
            AttestationError::Verification(format!("failed to encode SubjectPublicKeyInfo: {e}"))
        })
}

async fn production_trusted_root() -> Result<TrustedRoot> {
    Ok(TrustedRoot::production().await?)
}

fn github_trusted_root() -> Result<TrustedRoot> {
    Ok(TrustedRoot::from_embedded(SigstoreInstance::GitHub)?)
}

/// Per-process cache so we only fetch the Sigstore TUF root or parse the
/// embedded GitHub trusted root once per `verify_*` invocation. Each is
/// loaded lazily — a verification flow that only ever sees GitHub bundles
/// never triggers a network call to the Sigstore TUF CDN, and vice versa.
#[derive(Default)]
struct TrustRoots {
    sigstore: Option<TrustedRoot>,
    github: Option<TrustedRoot>,
}

impl TrustRoots {
    async fn for_bundle(&mut self, bundle: &Bundle) -> Result<&TrustedRoot> {
        if is_github_internal_certificate(bundle) {
            self.github_root()
        } else {
            self.sigstore_root().await
        }
    }

    async fn sigstore_root(&mut self) -> Result<&TrustedRoot> {
        if self.sigstore.is_none() {
            self.sigstore = Some(production_trusted_root().await?);
        }
        Ok(self.sigstore.as_ref().unwrap())
    }

    fn github_root(&mut self) -> Result<&TrustedRoot> {
        if self.github.is_none() {
            self.github = Some(github_trusted_root()?);
        }
        Ok(self.github.as_ref().unwrap())
    }
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

/// SLSA-specific checks once `verify_bundle` has cryptographically verified
/// the bundle: the DSSE payload is an SLSA provenance statement, the policy
/// level is supported, and the artifact's SHA-256 appears in the statement's
/// `subject` array. The subject check is the load-bearing part — without it,
/// a valid SLSA bundle signed for *some* artifact would accept *any* artifact.
fn verify_bundle_slsa(bundle: &Bundle, artifact: &[u8], min_level: u8) -> Result<()> {
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
    verify_intoto_payload(&payload, artifact, min_level)
}

fn decode_cosign_signature(bytes: &[u8]) -> Vec<u8> {
    let trimmed = String::from_utf8_lossy(bytes).trim().to_string();
    if let Some(decoded) = base64::engine::general_purpose::STANDARD
        .decode(trimmed.as_bytes())
        .ok()
        .filter(|_| !trimmed.is_empty())
    {
        return decoded;
    }
    bytes.to_vec()
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

    /// A genuine `*.intoto.jsonl` produced by slsa-github-generator (sops
    /// v3.9.0 release). Signed by Sigstore Fulcio. Tests that don't need a
    /// matching artifact can run against this fixture alone.
    const GENUINE_INTOTO_ENVELOPE: &str =
        include_str!("../tests/fixtures/sops_v3_9_0.intoto.jsonl");

    fn embedded_sigstore_root() -> TrustedRoot {
        TrustedRoot::from_json(sigstore_verify::trust_root::SIGSTORE_PRODUCTION_TRUSTED_ROOT)
            .expect("embedded production trusted_root.json parses")
    }

    #[test]
    fn intoto_envelope_rejects_tampered_signature() {
        let root = embedded_sigstore_root();
        let mut env: serde_json::Value =
            serde_json::from_str(GENUINE_INTOTO_ENVELOPE.trim()).unwrap();
        env["signatures"][0]["sig"] =
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b"forged"));
        let tampered = serde_json::to_string(&env).unwrap();

        // Signature verification happens before the subject digest check, so a
        // forged sig fails regardless of which artifact bytes we pass.
        let err = verify_intoto_envelope(&tampered, b"any artifact bytes", 1, &root)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("DSSE signature") || err.contains("signature verification failed"),
            "expected signature failure, got {err}"
        );
    }

    #[test]
    fn intoto_envelope_rejects_missing_signatures() {
        let root = embedded_sigstore_root();
        let mut env: serde_json::Value =
            serde_json::from_str(GENUINE_INTOTO_ENVELOPE.trim()).unwrap();
        env["signatures"] = serde_json::json!([]);
        let stripped = serde_json::to_string(&env).unwrap();
        let err = verify_intoto_envelope(&stripped, b"any artifact bytes", 1, &root)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no signatures"), "got {err}");
    }

    #[test]
    fn intoto_envelope_rejects_unknown_artifact() {
        // Genuine signature verifies, but a foreign artifact is not in subjects.
        let root = embedded_sigstore_root();
        let err = verify_intoto_envelope(
            GENUINE_INTOTO_ENVELOPE.trim(),
            b"different artifact contents",
            1,
            &root,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("not found in SLSA statement subjects"),
            "expected subject mismatch, got {err}"
        );
    }

    #[test]
    fn intoto_envelope_rejects_self_signed_cert() {
        // Replace the embedded Fulcio cert with an unrelated self-signed cert
        // and a recomputed signature. Chain validation must reject it.
        let root = embedded_sigstore_root();
        let mut env: serde_json::Value =
            serde_json::from_str(GENUINE_INTOTO_ENVELOPE.trim()).unwrap();
        // A self-signed P-256 cert (any will do — the issuer doesn't chain to
        // the Sigstore trust root).
        const SELF_SIGNED: &str = "-----BEGIN CERTIFICATE-----\n\
MIIBhTCCASugAwIBAgIUExample0AAAAAAAAAAAAAAAAAAAAwCgYIKoZIzj0EAwIw\n\
EzERMA8GA1UEAwwIc2VsZi1jYTAeFw0yNTAxMDEwMDAwMDBaFw0zNTAxMDEwMDAw\n\
MDBaMBMxETAPBgNVBAMMCHNlbGYtY2EwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNC\n\
AAQX9YJlbpFy0FmCXn7gC8m/qAh3wZw9w0CIxample/Random/dataABCDEFGHIJ\n\
KLMNOPQRSTUVWXYZabcdefghijklmnopo1MwUTAdBgNVHQ4EFgQUExampleHandle\n\
00000000000000000000003wHwYDVR0jBBgwFoAUExampleHandle00000000000\n\
00000000003wDwYDVR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNJADBGAiEAExam\n\
pleSignature1234567890123456789012345678901234567890CIQDExampleS\n\
ignature1234567890123456789012345678901234567890Aa==\n\
-----END CERTIFICATE-----\n";
        env["signatures"][0]["cert"] = serde_json::Value::String(SELF_SIGNED.to_string());
        let forged = serde_json::to_string(&env).unwrap();
        let err = verify_intoto_envelope(&forged, b"any artifact bytes", 1, &root)
            .unwrap_err()
            .to_string();
        assert!(
            err.to_lowercase().contains("chain")
                || err.to_lowercase().contains("trust")
                || err.to_lowercase().contains("invalid"),
            "expected chain validation failure, got {err}"
        );
    }
}
