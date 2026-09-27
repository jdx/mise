use super::*;

pub async fn verify_cosign_signature(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
) -> Result<bool> {
    let content = tokio::fs::read_to_string(sig_or_bundle_path).await?;
    let artifact = tokio::fs::read(artifact_path).await?;
    let mut trust_roots = TrustRoots::default();
    if let Ok(bundle) = Bundle::from_json(&content) {
        verify_bundle_with_trust_roots(Artifact::from(&artifact), &bundle, None, &mut trust_roots)
            .await?;
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
        sigstore_verify::verify_with_key(artifact.as_slice(), &bundle, &public_key, &trusted_root)?;
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
        KeyType, SigningScheme, detect_key_type, verify_signature, verify_signature_prehashed,
    };

    validate_bundle_with_options(
        bundle,
        &ValidationOptions {
            require_inclusion_proof: true,
            require_timestamp: false,
        },
    )
    .map_err(|e| AttestationError::Verification(format!("bundle validation failed: {e}")))?;

    let scheme = match detect_key_type(public_key) {
        KeyType::Ed25519 => SigningScheme::Ed25519,
        KeyType::EcdsaP256 => SigningScheme::EcdsaP256Sha256,
        KeyType::Unknown => {
            return Err(AttestationError::Verification(
                "unsupported or unrecognized public key type".to_string(),
            ));
        }
    };

    match &bundle.content {
        SignatureContent::MessageSignature(msg_sig) => {
            let artifact_hash = Sha256Hash::try_from_slice(&Sha256::digest(artifact))?;
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
            let payload = envelope.decode_payload();
            let pae = sigstore_verify::types::pae(&envelope.payload_type, &payload);
            if !envelope
                .signatures
                .iter()
                .any(|sig| verify_signature(public_key, &pae, &sig.sig, scheme).is_ok())
            {
                return Err(AttestationError::Verification(
                    "DSSE signature verification failed: no valid signatures found".to_string(),
                ));
            }
            verify_dsse_artifact_subject(&payload, artifact)?;
        }
    }

    Ok(())
}

fn verify_dsse_artifact_subject(payload: &[u8], artifact: &[u8]) -> Result<()> {
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
