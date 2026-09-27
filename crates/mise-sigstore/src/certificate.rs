use super::*;

/// Return the X.509 Issuer's `O` (organizationName) attribute, if present.
///
/// Used to dispatch verification policy: certs issued by GitHub's internal
/// Fulcio (`O=GitHub, Inc.`) need a separate trust root and a relaxed policy.
/// Parses the cert with x509-cert rather than byte-searching the DER, so we
/// only match the actual issuer organization field — not arbitrary substrings
/// elsewhere in the certificate.
/// Fulcio's Source Repository URI: the repository whose workflow run was
/// signed, even when a reusable workflow from another repository signed it
/// (that one is the certificate's SAN). A DER UTF8String.
pub(crate) const FULCIO_SOURCE_REPOSITORY_URI_OID: &str = "1.3.6.1.4.1.57264.1.12";
/// Fulcio's deprecated GitHub Workflow Repository (`owner/repo`), found on
/// certificates issued before the one above existed. Raw bytes, not DER.
pub(crate) const FULCIO_GITHUB_WORKFLOW_REPOSITORY_OID: &str = "1.3.6.1.4.1.57264.1.5";
/// The identity GitHub signs its own release attestations with.
pub(crate) const GITHUB_RELEASE_ATTESTER_IDENTITY: &str = "https://dotcom.releases.github.com";
pub(crate) const IN_TOTO_RELEASE_PREDICATE_PREFIX: &str = "https://in-toto.io/attestation/release/";

/// The repository, as `owner/repo`, that a bundle's signature vouches for.
/// Only meaningful for a bundle that already verified, so its certificate is
/// trusted.
///
/// - GitHub Actions certificates name it in a Fulcio extension.
/// - GitHub's release attestations (made for every immutable release) are
///   signed by GitHub itself, and name it in the signed statement. The
///   statement is only trusted when GitHub's release attester signed it:
///   any workflow can sign a statement that claims to be a release.
pub(crate) fn bundle_source_repository(bundle: &Bundle) -> Option<String> {
    use x509_cert::Certificate;
    use x509_cert::der::Decode;
    let cert = Certificate::from_der(bundle.signing_certificate()?.as_bytes()).ok()?;
    if let Some(repository) = certificate_source_repository(&cert) {
        return Some(repository);
    }
    // The release attester is GitHub's own signer, so its certificate must
    // come from GitHub's CA, not the public Sigstore one.
    if !is_github_internal_certificate(bundle)
        || !certificate_uri_sans(&cert)
            .iter()
            .any(|uri| uri == GITHUB_RELEASE_ATTESTER_IDENTITY)
    {
        return None;
    }
    let sigstore_verify::types::SignatureContent::DsseEnvelope(envelope) = &bundle.content else {
        return None;
    };
    release_statement_repository(&envelope.decode_payload())
}

pub(crate) fn certificate_source_repository(cert: &x509_cert::Certificate) -> Option<String> {
    use x509_cert::der::Decode;
    use x509_cert::der::asn1::Utf8StringRef;
    let extensions = cert.tbs_certificate().extensions()?;
    let extension = |oid: &str| {
        extensions
            .iter()
            .find(|ext| ext.extn_id.to_string() == oid)
            .map(|ext| ext.extn_value.as_bytes())
    };
    if let Some(value) = extension(FULCIO_SOURCE_REPOSITORY_URI_OID) {
        let uri = Utf8StringRef::from_der(value).ok()?;
        return github_repository_from_uri(uri.as_str());
    }
    let value = extension(FULCIO_GITHUB_WORKFLOW_REPOSITORY_OID)?;
    let repository = std::str::from_utf8(value).ok()?;
    valid_github_repository(repository).then(|| repository.to_string())
}

pub(crate) fn certificate_uri_sans(cert: &x509_cert::Certificate) -> Vec<String> {
    use x509_cert::ext::pkix::SubjectAltName;
    use x509_cert::ext::pkix::name::GeneralName;
    let Ok(Some(san)) = cert.tbs_certificate().get_extension::<SubjectAltName>() else {
        return Vec::new();
    };
    san.1
        .0
        .iter()
        .filter_map(|name| match name {
            GeneralName::UniformResourceIdentifier(uri) => Some(uri.to_string()),
            _ => None,
        })
        .collect()
}

/// Check the signer named by a verified Fulcio certificate. Chain and
/// signature validation must run before this policy check.
pub(crate) fn verify_slsa_signer_certificate(
    cert_der: &[u8],
    expected: SlsaSignerIdentity<'_>,
) -> Result<()> {
    use x509_cert::Certificate;
    use x509_cert::der::Decode;

    let cert = Certificate::from_der(cert_der).map_err(|e| {
        AttestationError::Verification(format!("failed to parse SLSA signer certificate: {e}"))
    })?;
    if !certificate_uri_sans(&cert)
        .iter()
        .any(|identity| identity == expected.identity)
    {
        return Err(AttestationError::WorkflowMismatch(format!(
            "SLSA signer identity does not match {:?}",
            expected.identity
        )));
    }

    // Fulcio's OIDC issuer extension is a raw UTF-8 URL in older and current
    // GitHub Actions certificates.
    const FULCIO_OIDC_ISSUER_OID: &str = "1.3.6.1.4.1.57264.1.1";
    let issuer = cert
        .tbs_certificate()
        .extensions()
        .and_then(|extensions| {
            extensions
                .iter()
                .find(|ext| ext.extn_id.to_string() == FULCIO_OIDC_ISSUER_OID)
        })
        .and_then(|ext| std::str::from_utf8(ext.extn_value.as_bytes()).ok());
    if issuer != Some(expected.issuer) {
        return Err(AttestationError::WorkflowMismatch(format!(
            "SLSA OIDC issuer does not match {:?}",
            expected.issuer
        )));
    }
    Ok(())
}

pub(crate) fn release_statement_repository(payload: &[u8]) -> Option<String> {
    let statement: serde_json::Value = serde_json::from_slice(payload).ok()?;
    if !statement
        .get("predicateType")?
        .as_str()?
        .starts_with(IN_TOTO_RELEASE_PREDICATE_PREFIX)
    {
        return None;
    }
    let repository = statement.get("predicate")?.get("repository")?.as_str()?;
    valid_github_repository(repository).then(|| repository.to_string())
}

pub(crate) fn github_repository_from_uri(uri: &str) -> Option<String> {
    let repository = uri.strip_prefix("https://github.com/")?;
    valid_github_repository(repository).then(|| repository.to_string())
}

pub(crate) fn valid_github_repository(repository: &str) -> bool {
    let mut parts = repository.split('/');
    let valid_part = |part: Option<&str>| {
        part.is_some_and(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
    };
    valid_part(parts.next()) && valid_part(parts.next()) && parts.next().is_none()
}

pub(crate) fn cert_issuer_organization(cert_der: &[u8]) -> Option<String> {
    use x509_cert::Certificate;
    use x509_cert::der::Decode;
    let cert = Certificate::from_der(cert_der).ok()?;
    for rdn in cert.tbs_certificate().issuer().iter_rdn() {
        for atv in rdn.iter() {
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
pub(crate) fn extract_spki_der(cert_der: &[u8]) -> Result<Vec<u8>> {
    use x509_cert::Certificate;
    use x509_cert::der::{Decode, Encode};
    let cert = Certificate::from_der(cert_der)
        .map_err(|e| AttestationError::Verification(format!("failed to parse certificate: {e}")))?;
    cert.tbs_certificate()
        .subject_public_key_info()
        .to_der()
        .map_err(|e| {
            AttestationError::Verification(format!("failed to encode SubjectPublicKeyInfo: {e}"))
        })
}

/// Process-global override for the Sigstore public-good TUF repository URL.
///
/// Set by the embedding crate from `settings.url_replacements` so the TUF root
/// fetch follows the same mirror/proxy as the rest of mise's HTTP traffic.
/// `None` means "use the crate default" (unchanged behavior).
pub(crate) static TUF_URL_OVERRIDE: std::sync::RwLock<Option<String>> =
    std::sync::RwLock::new(None);
