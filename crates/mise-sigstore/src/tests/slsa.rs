use super::*;

fn slsa_statement(subjects: serde_json::Value) -> Vec<u8> {
    serde_json::json!({
        "predicateType": "https://slsa.dev/provenance/v1",
        "subject": subjects,
    })
    .to_string()
    .into_bytes()
}

#[test]
fn intoto_payload_accepts_complete_content_subjects() {
    let artifact = SlsaArtifact::from_bytes("pixi".to_string(), b"binary");
    let payload = slsa_statement(serde_json::json!([
        {"name": "pixi", "digest": {"sha256": artifact.sha256.clone()}},
    ]));

    verify_intoto_payload_subjects(&payload, &[artifact], 1).unwrap();
}

#[test]
fn intoto_payload_rejects_partial_content_subjects() {
    let covered = SlsaArtifact::from_bytes("bin/tool".to_string(), b"tool");
    let uncovered = SlsaArtifact::from_bytes("README.md".to_string(), b"docs");
    let payload = slsa_statement(serde_json::json!([
        {"name": "bin/tool", "digest": {"sha256": covered.sha256.clone()}},
    ]));

    let err = verify_intoto_payload_subjects(&payload, &[covered, uncovered], 1)
        .unwrap_err()
        .to_string();
    assert!(err.contains("README.md"));
    assert!(err.contains("not found in SLSA statement subjects"));
}

#[test]
fn intoto_envelope_rejects_tampered_signature() {
    let root = embedded_sigstore_root();
    let mut env: serde_json::Value = serde_json::from_str(GENUINE_INTOTO_ENVELOPE.trim()).unwrap();
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
    let mut env: serde_json::Value = serde_json::from_str(GENUINE_INTOTO_ENVELOPE.trim()).unwrap();
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
    let mut env: serde_json::Value = serde_json::from_str(GENUINE_INTOTO_ENVELOPE.trim()).unwrap();
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
