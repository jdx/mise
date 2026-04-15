use std::fs;
use std::path::Path;

use base64::Engine;
use reqwest::StatusCode;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sigstore_trust_root::TrustedRoot;
use sigstore_types::{Bundle, DerPublicKey, Sha256Hash, SignatureBytes, SignatureContent};
use sigstore_verify::{VerificationPolicy, verify};

#[derive(Debug, thiserror::Error)]
pub enum AttestationError {
    #[error("no attestations found")]
    NoAttestations,
    #[error("verification failed: {0}")]
    Verification(String),
    #[error("attestation API error: {0}")]
    Api(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct GitHubAttestationsResponse {
    attestations: Vec<GitHubAttestation>,
}

#[derive(Debug, Deserialize)]
struct GitHubAttestation {
    bundle: Option<serde_json::Value>,
    bundle_url: Option<String>,
}

pub async fn has_github_attestations(
    owner: &str,
    repo: &str,
    digest: &str,
    token: Option<&str>,
) -> Result<bool, AttestationError> {
    let digest = digest.strip_prefix("sha256:").unwrap_or(digest);
    let response = match fetch_github_attestations(owner, repo, digest, token).await {
        Ok(response) => response,
        Err(AttestationError::NoAttestations) => return Ok(false),
        Err(e) => return Err(e),
    };
    Ok(!response.attestations.is_empty())
}

pub async fn verify_github_attestation(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    token: Option<&str>,
    signer_workflow: Option<&str>,
) -> Result<bool, AttestationError> {
    let digest = sha256_file_hex(artifact_path)?;
    let response = fetch_github_attestations(owner, repo, &digest, token).await?;
    if response.attestations.is_empty() {
        return Err(AttestationError::NoAttestations);
    }

    let artifact_digest =
        Sha256Hash::from_hex(&digest).map_err(|e| AttestationError::Verification(e.to_string()))?;
    let mut last_error = None;
    for attestation in response.attestations {
        match load_github_bundle(attestation, token).await {
            Ok(bundle_json) => {
                match verify_bundle_digest(artifact_digest, &bundle_json, signer_workflow, false) {
                    Ok(true) => return Ok(true),
                    Ok(false) => last_error = Some("verification returned false".to_string()),
                    Err(e) => last_error = Some(e.to_string()),
                }
            }
            Err(e) => last_error = Some(e.to_string()),
        }
    }

    Err(AttestationError::Verification(last_error.unwrap_or_else(
        || "no attestation bundle verified successfully".to_string(),
    )))
}

pub async fn verify_slsa_provenance(
    artifact_path: &Path,
    provenance_path: &Path,
    _min_level: u8,
) -> Result<bool, AttestationError> {
    let artifact = fs::read(artifact_path)?;
    let provenance = fs::read_to_string(provenance_path)?;

    for bundle_json in bundle_candidates(&provenance) {
        match verify_bundle_bytes(&artifact, &bundle_json, None) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(_) => {}
        }
    }

    Err(AttestationError::Verification(
        "file does not contain valid attestations or SLSA provenance".to_string(),
    ))
}

pub async fn verify_cosign_signature(
    artifact_path: &Path,
    bundle_path: &Path,
) -> Result<bool, AttestationError> {
    let artifact = fs::read(artifact_path)?;
    let bundle_json = fs::read_to_string(bundle_path)?;
    verify_bundle_bytes(&artifact, &bundle_json, None)
}

pub async fn verify_cosign_signature_with_key(
    artifact_path: &Path,
    signature_path: &Path,
    key_path: &Path,
) -> Result<bool, AttestationError> {
    let artifact = fs::read(artifact_path)?;
    let signature = decode_signature(&fs::read(signature_path)?);
    let key = fs::read_to_string(key_path)?;
    let public_key =
        DerPublicKey::from_pem(&key).map_err(|e| AttestationError::Verification(e.to_string()))?;
    let signature = SignatureBytes::new(signature);

    sigstore_verify::crypto::verify_signature_auto(&public_key, &signature, &artifact)
        .map_err(|e| AttestationError::Verification(e.to_string()))?;
    Ok(true)
}

async fn fetch_github_attestations(
    owner: &str,
    repo: &str,
    digest: &str,
    token: Option<&str>,
) -> Result<GitHubAttestationsResponse, AttestationError> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/attestations/sha256:{digest}");
    let client = reqwest::Client::new();
    let mut request = client
        .get(url)
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2022-11-28")
        .header("user-agent", "mise");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }

    let response = request.send().await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(AttestationError::NoAttestations);
    }
    if !response.status().is_success() {
        return Err(AttestationError::Api(format!(
            "GitHub returned {}",
            response.status()
        )));
    }
    Ok(response.json().await?)
}

async fn load_github_bundle(
    attestation: GitHubAttestation,
    token: Option<&str>,
) -> Result<String, AttestationError> {
    if let Some(bundle) = attestation.bundle {
        return Ok(serde_json::to_string(&bundle)?);
    }

    let bundle_url = attestation
        .bundle_url
        .ok_or_else(|| AttestationError::Verification("attestation has no bundle".to_string()))?;
    let client = reqwest::Client::new();
    let mut request = client.get(&bundle_url).header("user-agent", "mise");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(AttestationError::Api(format!(
            "GitHub bundle download returned {}",
            response.status()
        )));
    }
    let bytes = response.bytes().await?;
    if bundle_url.ends_with(".json.sn") {
        let decompressed = snap::raw::Decoder::new()
            .decompress_vec(&bytes)
            .map_err(|e| AttestationError::Verification(e.to_string()))?;
        return String::from_utf8(decompressed)
            .map_err(|e| AttestationError::Verification(e.to_string()));
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| AttestationError::Verification(e.to_string()))
}

fn verify_bundle_digest(
    artifact_digest: Sha256Hash,
    bundle_json: &str,
    signer_workflow: Option<&str>,
    verify_tlog: bool,
) -> Result<bool, AttestationError> {
    let bundle = Bundle::from_json(bundle_json)
        .map_err(|e| AttestationError::Verification(e.to_string()))?;
    let root =
        TrustedRoot::production().map_err(|e| AttestationError::Verification(e.to_string()))?;
    let mut policy = VerificationPolicy::default();
    if !verify_tlog {
        policy = policy.skip_tlog();
    }
    let result = match verify(artifact_digest, &bundle, &policy, &root) {
        Ok(result) => result,
        Err(e) if !verify_tlog && e.to_string().contains("tlog") => {
            return verify_github_dsse_bundle(artifact_digest, &bundle, signer_workflow);
        }
        Err(e) => return Err(AttestationError::Verification(e.to_string())),
    };
    verify_workflow_identity(&result.identity, signer_workflow)?;
    Ok(result.success)
}

fn verify_github_dsse_bundle(
    artifact_digest: Sha256Hash,
    bundle: &Bundle,
    signer_workflow: Option<&str>,
) -> Result<bool, AttestationError> {
    let cert = bundle
        .signing_certificate()
        .ok_or_else(|| AttestationError::Verification("bundle has no certificate".to_string()))?;
    let cert_info = sigstore_verify::crypto::parse_certificate_info(cert.as_bytes())
        .map_err(|e| AttestationError::Verification(e.to_string()))?;

    let SignatureContent::DsseEnvelope(envelope) = &bundle.content else {
        return Err(AttestationError::Verification(
            "GitHub attestation bundle is not a DSSE envelope".to_string(),
        ));
    };

    let pae = envelope.pae();
    if !envelope.signatures.iter().any(|sig| {
        sigstore_verify::crypto::verify_signature(
            &cert_info.public_key,
            &pae,
            &sig.sig,
            cert_info.signing_scheme,
        )
        .is_ok()
    }) {
        return Err(AttestationError::Verification(
            "DSSE signature verification failed".to_string(),
        ));
    }

    let payload = envelope.decode_payload();
    let payload =
        std::str::from_utf8(&payload).map_err(|e| AttestationError::Verification(e.to_string()))?;
    if !statement_matches_sha256(payload, &artifact_digest.to_hex())? {
        return Err(AttestationError::Verification(
            "artifact hash does not match any subject in attestation".to_string(),
        ));
    }

    verify_workflow_identity(&cert_info.identity, signer_workflow)?;
    Ok(true)
}

fn statement_matches_sha256(payload: &str, expected: &str) -> Result<bool, AttestationError> {
    let statement: serde_json::Value = serde_json::from_str(payload)?;
    let subjects = statement
        .get("subject")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AttestationError::Verification("attestation has no subjects".to_string()))?;
    Ok(subjects.iter().any(|subject| {
        subject
            .get("digest")
            .and_then(|digest| digest.get("sha256"))
            .and_then(|sha256| sha256.as_str())
            .is_some_and(|sha256| sha256.eq_ignore_ascii_case(expected))
    }))
}

fn verify_bundle_bytes(
    artifact: &[u8],
    bundle_json: &str,
    signer_workflow: Option<&str>,
) -> Result<bool, AttestationError> {
    let bundle = Bundle::from_json(bundle_json)
        .map_err(|e| AttestationError::Verification(e.to_string()))?;
    let root =
        TrustedRoot::production().map_err(|e| AttestationError::Verification(e.to_string()))?;
    let policy = VerificationPolicy::default();
    let result = verify(artifact, &bundle, &policy, &root)
        .map_err(|e| AttestationError::Verification(e.to_string()))?;
    verify_workflow_identity(&result.identity, signer_workflow)?;
    Ok(result.success)
}

fn verify_workflow_identity(
    identity: &Option<String>,
    signer_workflow: Option<&str>,
) -> Result<(), AttestationError> {
    let Some(expected) = signer_workflow else {
        return Ok(());
    };
    let Some(actual) = identity else {
        return Err(AttestationError::Verification(
            "signing certificate did not include an identity".to_string(),
        ));
    };
    if expected.is_empty() || actual.is_empty() {
        return Err(AttestationError::Verification(
            "signing certificate identity or expected workflow is empty".to_string(),
        ));
    }
    if actual == expected || actual.contains(expected) {
        Ok(())
    } else {
        Err(AttestationError::Verification(format!(
            "identity mismatch: expected workflow {expected}, got {actual}"
        )))
    }
}

fn bundle_candidates(input: &str) -> Vec<String> {
    let trimmed = input.trim();
    if trimmed.starts_with('{') {
        return vec![trimmed.to_string()];
    }
    trimmed
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('{'))
        .map(ToString::to_string)
        .collect()
}

fn sha256_file_hex(path: &Path) -> Result<String, AttestationError> {
    let data = fs::read(path)?;
    Ok(hex::encode(Sha256::digest(&data)))
}

fn decode_signature(bytes: &[u8]) -> Vec<u8> {
    let trimmed = std::str::from_utf8(bytes)
        .map(str::trim)
        .unwrap_or_default();
    if !trimmed.is_empty()
        && let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(trimmed)
    {
        return decoded;
    }
    bytes.to_vec()
}
