use super::*;

/// Verify a keyless cosign bundle. The signer must satisfy `identity`, which
/// has to pin the certificate identity: a valid Fulcio chain alone only proves
/// that *some* GitHub Actions workflow signed the artifact.
pub async fn verify_cosign_signature(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
    identity: &CosignIdentity,
) -> Result<bool> {
    identity.require_pinned_signer()?;
    let content = tokio::fs::read_to_string(sig_or_bundle_path).await?;
    let artifact = tokio::fs::read(artifact_path).await?;
    let mut trust_roots = TrustRoots::default();
    if let Ok(bundle) = Bundle::from_json(&content) {
        verify_keyless_bundle(
            Artifact::from(&artifact),
            &bundle,
            identity,
            &mut trust_roots,
        )
        .await?;
        return Ok(true);
    }
    // Legacy cosign v1 bundle (`{base64Signature, cert, rekorBundle}`).
    // sigstore-verify only consumes the modern bundle shape, so we verify
    // these manually: chain-validate the embedded cert against Sigstore
    // Fulcio, then ECDSA-verify the signature over the artifact bytes.
    let trusted_root = trust_roots.sigstore_root().await?;
    verify_legacy_cosign_bundle(&artifact, &content, trusted_root, identity)?;
    Ok(true)
}

/// Chain-validate a modern keyless bundle, then require its signing
/// certificate to satisfy `identity`.
pub(crate) async fn verify_keyless_bundle<'a>(
    artifact: Artifact<'a>,
    bundle: &Bundle,
    identity: &CosignIdentity,
    trust_roots: &mut TrustRoots,
) -> Result<()> {
    verify_bundle_with_trust_roots(artifact, bundle, None, trust_roots).await?;
    let cert = bundle.signing_certificate().ok_or_else(|| {
        AttestationError::Verification("cosign bundle is missing a signer certificate".to_string())
    })?;
    verify_certificate_identity(cert.as_bytes(), identity)
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
        if matches!(
            &bundle.verification_material.content,
            VerificationMaterialContent::PublicKey { .. }
        ) {
            let artifact = tokio::fs::read(artifact_path).await?;
            verify_public_key_bundle(&artifact, &bundle, &public_key)?;
            return Ok(true);
        }

        // Bundle path: needs the trust root for tlog (Rekor) verification.
        let trusted_root = production_trusted_root().await?;
        let artifact = tokio::fs::read(artifact_path).await?;
        sigstore_verify::verify_with_key(
            artifact.as_slice(),
            &bundle,
            &public_key,
            &sigstore_verify::PublicKeyVerificationPolicy::default(),
            &trusted_root,
        )?;
        return Ok(true);
    }

    // Raw `.sig` path: only needs the local public key — no network access.
    let artifact = tokio::fs::read(artifact_path).await?;
    let signature = decode_cosign_signature(&raw_bytes);
    verify_raw_signature(&artifact, &signature, &public_key)?;
    Ok(true)
}

pub(crate) fn verify_public_key_bundle(
    artifact: &[u8],
    bundle: &Bundle,
    public_key: &DerPublicKey,
) -> Result<()> {
    use sigstore_verify::bundle::{ValidationOptions, validate_bundle_with_options};
    use sigstore_verify::crypto::{
        KeyAlgorithm, SigningScheme, verify_signature, verify_signature_prehashed,
    };

    validate_bundle_with_options(
        bundle,
        &ValidationOptions::new().with_require_inclusion_proof(true),
    )
    .map_err(|e| AttestationError::Verification(format!("bundle validation failed: {e}")))?;

    let scheme = match KeyAlgorithm::from_spki(public_key) {
        Ok(KeyAlgorithm::Ed25519) => SigningScheme::Ed25519,
        Ok(KeyAlgorithm::EcdsaP256) => SigningScheme::EcdsaP256Sha256,
        _ => {
            return Err(AttestationError::Verification(
                "unsupported or unrecognized public key type".to_string(),
            ));
        }
    };

    match &bundle.content {
        SignatureContent::MessageSignature(msg_sig) => {
            let artifact_hash = Sha256Hash::try_from(Sha256::digest(artifact).as_slice())?;
            if let Some(digest) = &msg_sig.message_digest {
                if digest.algorithm != HashAlgorithm::Sha2256 {
                    return Err(AttestationError::Verification(format!(
                        "unsupported message digest algorithm {}",
                        digest.algorithm
                    )));
                }
                if digest.digest != artifact_hash {
                    return Err(AttestationError::Verification(
                        "message digest in bundle does not match artifact hash".to_string(),
                    ));
                }
            }

            if scheme.supports_prehashed() {
                verify_signature_prehashed(
                    public_key,
                    artifact_hash.as_bytes(),
                    &msg_sig.signature,
                    scheme,
                )
            } else {
                verify_signature(public_key, artifact, &msg_sig.signature, scheme)
            }
            .map_err(|e| {
                AttestationError::Verification(format!("signature verification failed: {e}"))
            })?;
        }
        SignatureContent::DsseEnvelope(envelope) => {
            let payload = envelope.payload.as_bytes();
            let pae = sigstore_verify::types::pae(&envelope.payload_type, payload);
            if verify_signature(public_key, &pae, &envelope.signature.sig, scheme).is_err() {
                return Err(AttestationError::Verification(
                    "DSSE signature verification failed: no valid signatures found".to_string(),
                ));
            }
            verify_dsse_artifact_subject(payload, artifact)?;
        }
        _ => {
            return Err(AttestationError::UnsupportedFormat(
                "unsupported bundle signature content".to_string(),
            ));
        }
    }

    Ok(())
}

pub(crate) fn verify_dsse_artifact_subject(payload: &[u8], artifact: &[u8]) -> Result<()> {
    let statement: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|e| AttestationError::Verification(format!("invalid DSSE payload: {e}")))?;
    let artifact_digest = hex::encode(Sha256::digest(artifact));
    let matches = statement
        .get("subject")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|subjects| {
            subjects.iter().any(|subject| {
                subject
                    .get("digest")
                    .and_then(|digest| digest.get("sha256"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|digest| digest.eq_ignore_ascii_case(&artifact_digest))
            })
        });
    if !matches {
        return Err(AttestationError::Verification(
            "DSSE subject digest does not match artifact".to_string(),
        ));
    }
    Ok(())
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
pub(crate) fn verify_legacy_cosign_bundle(
    artifact: &[u8],
    bundle_json: &str,
    trusted_root: &TrustedRoot,
    identity: &CosignIdentity,
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
    verify_certificate_identity(cert.as_bytes(), identity)?;

    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(sig_b64.as_bytes())
        .map_err(|e| AttestationError::Verification(format!("invalid base64 signature: {e}")))?;
    let spki_der = extract_spki_der(cert.as_bytes())?;
    let public_key = DerPublicKey::new(spki_der);
    verify_raw_signature(artifact, &sig_bytes, &public_key)
}

pub(crate) fn decode_cosign_signature(bytes: &[u8]) -> Vec<u8> {
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

pub(crate) fn verify_raw_signature(
    artifact: &[u8],
    signature: &[u8],
    public_key: &DerPublicKey,
) -> Result<()> {
    use sigstore_verify::crypto::{KeyAlgorithm, SigningScheme, verify_signature};

    let scheme = match KeyAlgorithm::from_spki(public_key) {
        Ok(KeyAlgorithm::Ed25519) => SigningScheme::Ed25519,
        Ok(KeyAlgorithm::EcdsaP256) => SigningScheme::EcdsaP256Sha256,
        _ => {
            return Err(AttestationError::Verification(
                "unsupported or unrecognized public key type".to_string(),
            ));
        }
    };
    let signature = SignatureBytes::from_bytes(signature);
    verify_signature(public_key, artifact, &signature, scheme)
        .map_err(|e| AttestationError::Verification(format!("signature verification failed: {e}")))
}
