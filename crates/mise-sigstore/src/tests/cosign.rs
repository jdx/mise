use super::*;
use crate::cosign::{verify_dsse_artifact_subject, verify_public_key_bundle};

#[test]
fn public_key_dsse_subject_must_match_artifact() {
    let digest = hex::encode(Sha256::digest(b"artifact"));
    let payload = serde_json::to_vec(&serde_json::json!({
        "subject": [{"digest": {"sha256": digest.to_uppercase()}}]
    }))
    .unwrap();
    verify_dsse_artifact_subject(&payload, b"artifact").unwrap();

    let err = verify_dsse_artifact_subject(&payload, b"different artifact").unwrap_err();
    assert!(err.to_string().contains("does not match artifact"));
}

#[test]
fn public_key_dsse_rejects_missing_or_invalid_subjects() {
    for payload in [br#"{}"#.as_slice(), br#"{"subject":[]}"#, br#"not json"#] {
        assert!(verify_dsse_artifact_subject(payload, b"artifact").is_err());
    }
}

#[test]
fn signed_public_key_dsse_bundle_binds_artifact() {
    // The signed payload and public key are test fixtures. Reuse a real
    // inclusion proof for structural bundle validation; this path checks
    // the pinned key's signature, not the proof's cryptographic validity.
    let mut fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/github_build_provenance_jdx_mise.json"
    ))
    .unwrap();
    let material = fixture["verificationMaterial"].as_object_mut().unwrap();
    material.remove("certificate");
    material.insert("publicKey".to_string(), serde_json::json!({"hint": "test"}));
    fixture["dsseEnvelope"] = serde_json::json!({
        "payloadType": "application/vnd.in-toto+json",
        "payload": "eyJfdHlwZSI6Imh0dHBzOi8vaW4tdG90by5pby9TdGF0ZW1lbnQvdjEiLCJzdWJqZWN0IjpbeyJuYW1lIjoiYXJ0aWZhY3QiLCJkaWdlc3QiOnsic2hhMjU2IjoiYzdjNWMxZDcwYzVkZWM0NDE2YWI2MTU4YWZkMGIyMjNlZjQwYzI5YjFkYzFmOTdlZDk0MjhiOTRkNGNhZGIxYyJ9fV19",
        "signatures": [{"sig": "/A10dkD75B7Iem/dSefYsil3K6CM1Pm6Dz4KBapWRO8HuxwOzBTvDX5w3tsD3/5jcxF+KRD/egbHkno4wr8ZBA=="}]
    });
    let bundle = Bundle::from_json(&fixture.to_string()).unwrap();
    let key = DerPublicKey::from_pem(
            "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAenAN5yDEkIVZOtHA1FJ9zb2Jy2hZAXAnayUfH4goans=\n-----END PUBLIC KEY-----\n",
        )
        .unwrap();

    verify_public_key_bundle(b"artifact", &bundle, &key).unwrap();
    let err = verify_public_key_bundle(b"different artifact", &bundle, &key).unwrap_err();
    assert!(
        err.to_string()
            .contains("DSSE subject digest does not match artifact")
    );
}
