use super::*;
use crate::github::{log_attestation_method_failure, warn_if_trust_root_unreachable};

pub(crate) async fn verify_attestation_bundles(
    attestations: &[Attestation],
    artifact: &[u8],
    signer_workflow: Option<&str>,
    trust_roots: &mut TrustRoots,
) -> Result<bool> {
    verify_attestation_bundles_for_artifact(
        Artifact::from(artifact),
        attestations,
        signer_workflow,
        trust_roots,
    )
    .await
}

pub(crate) async fn verify_attestation_bundles_for_artifact(
    artifact: Artifact<'_>,
    attestations: &[Attestation],
    signer_workflow: Option<&str>,
    trust_roots: &mut TrustRoots,
) -> Result<bool> {
    let mut errors = Vec::new();
    let mut trust_root_failures = 0;
    for attestation in attestations {
        let Some(bundle_value) = &attestation.bundle else {
            continue;
        };
        let bundle = match serde_json::from_value::<Bundle>(bundle_value.clone()) {
            Ok(bundle) => bundle,
            Err(e) => {
                let error_message = e.to_string();
                log_attestation_method_failure(&AttestationError::Json(e));
                errors.push(error_message);
                continue;
            }
        };
        match verify_bundle_with_trust_roots(
            artifact.clone(),
            &bundle,
            signer_workflow,
            trust_roots,
        )
        .await
        {
            Ok(()) => {
                warn_if_trust_root_unreachable(trust_root_failures);
                return Ok(true);
            }
            Err(e) => {
                log_attestation_method_failure(&e);
                if matches!(e, AttestationError::TrustRoot(_)) {
                    trust_root_failures += 1;
                }
                errors.push(e.to_string());
            }
        }
    }

    Err(AttestationError::Verification(join_error_strings(
        errors,
        || "No valid attestations found".to_string(),
    )))
}

pub(crate) fn is_snappy_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|content_type| content_type.split(';').next())
        .is_some_and(|content_type| content_type.trim() == "application/x-snappy")
}

pub(crate) fn verify_bundle<'a>(
    artifact: impl Into<Artifact<'a>>,
    bundle: &Bundle,
    signer_workflow: Option<&str>,
    trusted_root: &TrustedRoot,
) -> Result<()> {
    let mut policy = VerificationPolicy::any_identity();
    // sigstore-verify's default policy *requires* an inclusion proof when
    // `verify_tlog` is on. GitHub artifact attestations and TSA-only bundles
    // never carry one, so we'd reject them outright. Skip tlog only when the
    // bundle has no inclusion proof — public-Sigstore cosign bundles, which do
    // ship a Rekor inclusion proof, still get full tlog verification (Rekor
    // checkpoint signature, SET, inclusion-proof Merkle path).
    if !bundle.has_inclusion_proof() {
        policy = policy.skip_tlog_unsafe();
    }
    // GitHub-internal leaf certs don't carry an SCT extension (GitHub's CA
    // doesn't log to public CT). `skip_sct` keeps full certificate-chain
    // validation against the GitHub trust root's Fulcio certs but turns off
    // the SCT check, which is exactly what GitHub artifact attestations need.
    if is_github_internal_certificate(bundle) {
        policy = policy.skip_sct();
    }
    let result = sigstore_verify::verify(artifact, bundle, &policy, trusted_root)?;

    verify_signer_workflow_identity(
        result.identity().map(|identity| identity.as_str()),
        signer_workflow,
    )?;

    Ok(())
}

pub(crate) async fn verify_bundle_with_trust_roots<'a>(
    artifact: Artifact<'a>,
    bundle: &Bundle,
    signer_workflow: Option<&str>,
    trust_roots: &mut TrustRoots,
) -> Result<()> {
    if is_github_internal_certificate(bundle) {
        return verify_github_bundle_with_tuf_retry(artifact, bundle, signer_workflow, trust_roots)
            .await;
    }

    let trusted_root = trust_roots.sigstore_root().await?;
    verify_bundle(artifact, bundle, signer_workflow, trusted_root)
}

pub(crate) async fn verify_bundle_with_slsa_signer<'a>(
    artifact: Artifact<'a>,
    bundle: &Bundle,
    signer: SlsaSignerIdentity<'_>,
    trust_roots: &mut TrustRoots,
) -> Result<()> {
    verify_bundle_with_trust_roots(artifact, bundle, None, trust_roots).await?;
    let cert = bundle.signing_certificate().ok_or_else(|| {
        AttestationError::Verification("SLSA bundle is missing a signer certificate".to_string())
    })?;
    verify_slsa_signer_certificate(cert.as_bytes(), signer)
}

pub(crate) async fn verify_github_bundle_with_tuf_retry<'a>(
    artifact: Artifact<'a>,
    bundle: &Bundle,
    signer_workflow: Option<&str>,
    trust_roots: &mut TrustRoots,
) -> Result<()> {
    let embedded_result = match trust_roots.github_embedded_root() {
        Ok(trusted_root) => verify_bundle(artifact.clone(), bundle, signer_workflow, trusted_root),
        Err(e) => Err(e),
    };
    verify_github_bundle_with_tuf_retry_after_embedded_result(embedded_result, || async {
        let trusted_root = trust_roots.github_tuf_root().await?;
        verify_bundle(artifact, bundle, signer_workflow, trusted_root)
    })
    .await
}

pub(crate) async fn verify_github_bundle_with_tuf_retry_after_embedded_result<F, Fut>(
    embedded_result: Result<()>,
    verify_with_tuf: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    match embedded_result {
        Ok(()) => Ok(()),
        Err(e) if is_signer_workflow_mismatch(&e) => Err(e),
        Err(embedded_err) => match verify_with_tuf().await {
            Ok(()) => Ok(()),
            Err(tuf_err) if is_signer_workflow_mismatch(&tuf_err) => Err(tuf_err),
            Err(tuf_err) => {
                let message = format!(
                    "GitHub attestation verification failed with embedded trusted root: \
                     {embedded_err}; GitHub TUF trusted root retry also failed: {tuf_err}"
                );
                // Keep the category: a trust root that could not be loaded is
                // not a signature failure, and callers count it to warn.
                Err(match tuf_err {
                    AttestationError::TrustRoot(_) => AttestationError::TrustRoot(message),
                    _ => AttestationError::Verification(message),
                })
            }
        },
    }
}

pub(crate) fn is_signer_workflow_mismatch(err: &AttestationError) -> bool {
    matches!(err, AttestationError::WorkflowMismatch(_))
}

pub(crate) fn is_github_internal_certificate(bundle: &Bundle) -> bool {
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
pub(crate) fn verify_cert_chain(leaf_der: &[u8], trusted_root: &TrustedRoot) -> Result<()> {
    use rustls_pki_types::{CertificateDer, UnixTime};
    use webpki::{ALL_VERIFICATION_ALGS, EndEntityCert, KeyUsage, anchor_from_trusted_cert};
    use x509_cert::Certificate;
    use x509_cert::der::Decode;

    let leaf = Certificate::from_der(leaf_der).map_err(|e| {
        AttestationError::Verification(format!("failed to parse leaf certificate: {e}"))
    })?;
    let not_after = leaf
        .tbs_certificate()
        .validity()
        .not_after
        .to_unix_duration()
        .as_secs();
    let validation_time = UnixTime::since_unix_epoch(std::time::Duration::from_secs(not_after));

    let all_certs = trusted_root.fulcio_certs();
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
