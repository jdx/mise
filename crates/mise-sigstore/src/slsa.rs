use super::*;
use crate::cosign::verify_raw_signature;

pub async fn verify_slsa_provenance(
    artifact_path: &Path,
    provenance_path: &Path,
    min_level: u8,
    signer: SlsaSignerIdentity<'_>,
) -> Result<bool> {
    let artifact = tokio::fs::read(artifact_path).await?;
    verify_slsa_provenance_artifacts(
        provenance_path,
        &[SlsaArtifact::from_bytes(String::new(), &artifact)],
        min_level,
        signer,
    )
    .await
}

pub async fn verify_slsa_provenance_artifacts(
    provenance_path: &Path,
    artifacts: &[SlsaArtifact],
    min_level: u8,
    signer: SlsaSignerIdentity<'_>,
) -> Result<bool> {
    if signer.identity.is_empty() || signer.issuer.is_empty() {
        return Err(AttestationError::Verification(
            "SLSA signer identity and OIDC issuer must be set".to_string(),
        ));
    }
    if artifacts.is_empty() {
        return Err(AttestationError::SubjectMismatch(
            "no artifacts supplied for SLSA subject verification".to_string(),
        ));
    }

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
            let result =
                match verify_bundle_for_any_artifact(artifacts, &bundle, signer, &mut trust_roots)
                    .await
                {
                    Ok(()) => verify_bundle_slsa_subjects(&bundle, artifacts, min_level),
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
            Ok(root) => {
                verify_intoto_envelope_subjects(candidate, artifacts, min_level, signer, root)
            }
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

#[cfg(test)]
pub(crate) fn verify_intoto_envelope(
    line: &str,
    artifact: &[u8],
    min_level: u8,
    signer: SlsaSignerIdentity<'_>,
    trusted_root: &TrustedRoot,
) -> Result<()> {
    verify_intoto_envelope_subjects(
        line,
        &[SlsaArtifact::from_bytes(String::new(), artifact)],
        min_level,
        signer,
        trusted_root,
    )
}

pub(crate) fn verify_intoto_envelope_subjects(
    line: &str,
    artifacts: &[SlsaArtifact],
    min_level: u8,
    signer: SlsaSignerIdentity<'_>,
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
        match verify_dsse_signature(sig, &pae, signer, trusted_root) {
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

    verify_intoto_payload_subjects(&payload, artifacts, min_level)
}

pub(crate) fn verify_dsse_signature(
    sig: &serde_json::Value,
    pae: &[u8],
    signer: SlsaSignerIdentity<'_>,
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
    verify_slsa_signer_certificate(cert.as_bytes(), signer)?;
    let spki_der = extract_spki_der(cert.as_bytes())?;
    let public_key = DerPublicKey::new(spki_der);
    verify_raw_signature(pae, &sig_bytes, &public_key)
}

pub(crate) fn verify_intoto_payload_subjects(
    payload: &[u8],
    artifacts: &[SlsaArtifact],
    min_level: u8,
) -> Result<()> {
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
    let subjects = statement
        .get("subject")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            AttestationError::Verification("SLSA statement missing subject array".to_string())
        })?;

    let subject_digests = subjects
        .iter()
        .filter_map(|subject| {
            subject
                .get("digest")
                .and_then(|d| d.get("sha256"))
                .and_then(|v| v.as_str())
                .map(|sha| sha.to_ascii_lowercase())
        })
        .collect::<std::collections::HashSet<_>>();
    let named_subjects = subjects
        .iter()
        .filter_map(|subject| {
            let name = subject.get("name")?.as_str()?;
            let sha = subject
                .get("digest")
                .and_then(|d| d.get("sha256"))
                .and_then(|v| v.as_str())?;
            Some((name.to_string(), sha.to_ascii_lowercase()))
        })
        .collect::<std::collections::HashSet<_>>();

    let mut missing = Vec::new();
    for artifact in artifacts {
        let artifact_digest = artifact.sha256.to_ascii_lowercase();
        let matches_subject = if artifact.name.is_empty() {
            subject_digests.contains(&artifact_digest)
        } else {
            named_subjects.contains(&(artifact.name.clone(), artifact_digest.clone()))
        };
        if !matches_subject {
            if artifact.name.is_empty() {
                missing.push(artifact_digest);
            } else {
                missing.push(format!("{} ({artifact_digest})", artifact.name));
            }
        }
    }
    if !missing.is_empty() {
        return Err(AttestationError::SubjectMismatch(format!(
            "artifact subjects not found in SLSA statement subjects: {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

pub(crate) fn collapse_slsa_errors(
    errors: Vec<AttestationError>,
    default: impl FnOnce() -> String,
) -> Result<bool> {
    let unsupported_format = errors
        .iter()
        .all(|error| matches!(error, AttestationError::UnsupportedFormat(_)));
    let subject_mismatch = errors.iter().any(is_slsa_subject_mismatch);
    let message = join_error_strings(
        errors.into_iter().map(|error| error.to_string()).collect(),
        default,
    );
    Err(if unsupported_format {
        AttestationError::UnsupportedFormat(message)
    } else if subject_mismatch {
        AttestationError::SubjectMismatch(message)
    } else {
        AttestationError::Verification(message)
    })
}

pub fn is_slsa_subject_mismatch(error: &AttestationError) -> bool {
    match error {
        AttestationError::SubjectMismatch(_) => true,
        AttestationError::Verification(msg) | AttestationError::Sigstore(msg) => {
            is_subject_mismatch_message(msg)
        }
        _ => false,
    }
}

pub(crate) fn is_subject_mismatch_message(message: &str) -> bool {
    message.contains("artifact hash does not match any subject in attestation")
        || message.contains("not found in SLSA statement subjects")
        || message.contains("artifact subjects not found in SLSA statement subjects")
}

pub(crate) fn join_error_strings(errors: Vec<String>, default: impl FnOnce() -> String) -> String {
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

/// SLSA-specific checks once `verify_bundle` has cryptographically verified
/// the bundle: the DSSE payload is an SLSA provenance statement, the policy
/// level is supported, and the artifact's SHA-256 appears in the statement's
/// `subject` array. The subject check is the load-bearing part — without it,
/// a valid SLSA bundle signed for *some* artifact would accept *any* artifact.
pub(crate) async fn verify_bundle_for_any_artifact(
    artifacts: &[SlsaArtifact],
    bundle: &Bundle,
    signer: SlsaSignerIdentity<'_>,
    trust_roots: &mut TrustRoots,
) -> Result<()> {
    let artifact = artifacts.first().ok_or_else(|| {
        AttestationError::SubjectMismatch(
            "no artifacts supplied for SLSA subject verification".to_string(),
        )
    })?;
    let digest = Sha256Hash::from_hex(&artifact.sha256).map_err(|e| {
        AttestationError::Verification(format!("invalid artifact sha256 digest: {e}"))
    })?;
    match verify_bundle_with_slsa_signer(Artifact::from(&digest), bundle, signer, trust_roots).await
    {
        Ok(()) => Ok(()),
        Err(e) if is_slsa_subject_mismatch(&e) => {
            Err(AttestationError::SubjectMismatch(e.to_string()))
        }
        Err(e) => Err(e),
    }
}

pub(crate) fn verify_bundle_slsa_subjects(
    bundle: &Bundle,
    artifacts: &[SlsaArtifact],
    min_level: u8,
) -> Result<()> {
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
    verify_intoto_payload_subjects(&payload, artifacts, min_level)
}
