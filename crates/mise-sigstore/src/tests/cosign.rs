use super::*;
use crate::cosign::{
    verify_dsse_artifact_subject, verify_keyless_bundle, verify_public_key_bundle,
};

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

fn opts(opts: &[&str]) -> Vec<String> {
    opts.iter().map(|s| s.to_string()).collect()
}

#[test]
fn cosign_identity_parses_registry_opts() {
    let identity = CosignIdentity::from_opts(&opts(&[
        "--certificate-identity-regexp",
        "^https://github.com/o/r/",
        "--certificate-oidc-issuer=https://token.actions.githubusercontent.com",
        "--certificate-github-workflow-repository",
        "o/r",
        "--certificate-github-workflow-ref",
        "refs/tags/v1",
        "--key",
        "https://example.com/key.pub",
    ]))
    .unwrap();
    assert_eq!(
        identity.identity_regexp.as_deref(),
        Some("^https://github.com/o/r/")
    );
    assert_eq!(
        identity.oidc_issuer.as_deref(),
        Some("https://token.actions.githubusercontent.com")
    );
    assert_eq!(identity.github_workflow_repository.as_deref(), Some("o/r"));
    assert_eq!(
        identity.github_workflow_ref.as_deref(),
        Some("refs/tags/v1")
    );
    assert!(identity.pins_signer());
}

#[test]
fn cosign_identity_rejects_unknown_or_valueless_certificate_flags() {
    assert!(CosignIdentity::from_opts(&opts(&["--certificate-unknown", "x"])).is_err());
    assert!(CosignIdentity::from_opts(&opts(&["--certificate-identity"])).is_err());
    // The next option is not a value.
    assert!(
        CosignIdentity::from_opts(&opts(&[
            "--certificate-identity-regexp",
            "--certificate-oidc-issuer",
            "https://token.actions.githubusercontent.com",
        ]))
        .is_err()
    );
    // Inline values are taken verbatim.
    let identity = CosignIdentity::from_opts(&opts(&["--certificate-identity=--odd"])).unwrap();
    assert_eq!(identity.identity.as_deref(), Some("--odd"));
}

#[test]
fn cosign_identity_without_signer_is_not_pinned() {
    let identity = CosignIdentity::from_opts(&opts(&[
        "--certificate-oidc-issuer",
        "https://token.actions.githubusercontent.com",
    ]))
    .unwrap();
    assert!(!identity.pins_signer());
    let err = identity.require_pinned_signer().unwrap_err().to_string();
    assert!(err.contains("requires a certificate identity"), "{err}");
}

fn fixture_cert_der() -> Vec<u8> {
    let bundle = Bundle::from_json(include_str!(
        "../../tests/fixtures/github_build_provenance_jdx_mise.json"
    ))
    .unwrap();
    bundle.signing_certificate().unwrap().as_bytes().to_vec()
}

const FIXTURE_IDENTITY: &str =
    "https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v2026.9.12";
const FIXTURE_ISSUER: &str = "https://token.actions.githubusercontent.com";

#[test]
fn certificate_identity_accepts_the_real_signer() {
    let der = fixture_cert_der();
    for identity in [
        CosignIdentity {
            identity: Some(FIXTURE_IDENTITY.to_string()),
            oidc_issuer: Some(FIXTURE_ISSUER.to_string()),
            ..Default::default()
        },
        CosignIdentity {
            identity_regexp: Some(r"^https://github\.com/jdx/mise/\.github/workflows/.+$".into()),
            github_workflow_repository: Some("jdx/mise".to_string()),
            github_workflow_ref: Some("refs/tags/v2026.9.12".to_string()),
            ..Default::default()
        },
    ] {
        verify_certificate_identity(&der, &identity).unwrap();
    }
}

#[test]
fn certificate_identity_regexp_supports_re2_quoted_literals() {
    // The aqua registry pins a tag with `\Q{{.Version}}\E`, which is RE2 syntax.
    let der = fixture_cert_der();
    let regexp = |pattern: &str| CosignIdentity {
        identity_regexp: Some(pattern.to_string()),
        ..Default::default()
    };
    verify_certificate_identity(
        &der,
        &regexp(
            r"^https://github\.com/jdx/mise/\.github/workflows/.+\.ya?ml@refs/tags/\Qv2026.9.12\E$",
        ),
    )
    .unwrap();
    for rejected in [
        // The quoted dots are literal, not wildcards.
        r"^https://github\.com/jdx/mise/\.github/workflows/.+@refs/tags/\Qv2026X9X12\E$",
        // Another tag is not this tag.
        r"^https://github\.com/jdx/mise/\.github/workflows/.+@refs/tags/\Qv2026.9.1\E$",
        // An unterminated \Q quotes through the end, metacharacters included.
        r"^https://github\.com/jdx/mise/\.github/workflows/.+@refs/tags/\Qv2026.9.12$",
    ] {
        assert!(
            verify_certificate_identity(&der, &regexp(rejected)).is_err(),
            "{rejected} should be rejected"
        );
    }
}

#[test]
fn certificate_identity_rejects_other_signers() {
    let der = fixture_cert_der();
    let base = CosignIdentity {
        identity: Some(FIXTURE_IDENTITY.to_string()),
        ..Default::default()
    };
    for rejected in [
        CosignIdentity {
            identity: Some(FIXTURE_IDENTITY.replace("jdx/mise", "attacker/mise")),
            ..Default::default()
        },
        // A substring of the real identity is not the identity.
        CosignIdentity {
            identity: Some("jdx/mise/.github/workflows/release.yml".to_string()),
            ..Default::default()
        },
        CosignIdentity {
            identity_regexp: Some(r"^https://github\.com/other/".to_string()),
            ..Default::default()
        },
        CosignIdentity {
            oidc_issuer: Some("https://accounts.google.com".to_string()),
            ..base.clone()
        },
        CosignIdentity {
            github_workflow_repository: Some("attacker/mise".to_string()),
            ..base.clone()
        },
        CosignIdentity {
            github_workflow_ref: Some("refs/heads/main".to_string()),
            ..base.clone()
        },
        CosignIdentity {
            identity_regexp: Some("(".to_string()),
            ..Default::default()
        },
    ] {
        assert!(
            verify_certificate_identity(&der, &rejected).is_err(),
            "{rejected:?} should be rejected"
        );
    }
}

#[tokio::test]
async fn keyless_cosign_requires_a_pinned_signer() {
    let dir = std::env::temp_dir().join(format!("mise-cosign-identity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let artifact = dir.join("artifact");
    let bundle = dir.join("artifact.bundle");
    std::fs::write(&artifact, b"artifact").unwrap();
    std::fs::write(&bundle, b"{}").unwrap();
    let err = verify_cosign_signature(&artifact, &bundle, &CosignIdentity::default())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("requires a certificate identity"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn trivial_identity_patterns_do_not_pin_the_signer() {
    for identity in [
        CosignIdentity {
            identity: Some(String::new()),
            ..Default::default()
        },
        CosignIdentity {
            identity_regexp: Some(String::new()),
            ..Default::default()
        },
        CosignIdentity {
            identity_regexp: Some(".*".to_string()),
            ..Default::default()
        },
        CosignIdentity {
            identity_regexp: Some("^".to_string()),
            ..Default::default()
        },
    ] {
        assert!(!identity.pins_signer(), "{identity:?} should not pin");
    }
    let from_empty_opt =
        CosignIdentity::from_opts(&opts(&["--certificate-identity-regexp="])).unwrap();
    assert!(!from_empty_opt.pins_signer());
}

#[tokio::test]
async fn keyless_bundle_verification_enforces_signer_identity() {
    // mise-v2026.9.12-linux-x64.tar.gz, attested by the fixture bundle.
    let digest =
        Sha256Hash::from_hex("b4058dece685259910d3aba5782445996eea79dbdb3cf952a6eb81aadf0373ff")
            .unwrap();
    let bundle = Bundle::from_json(include_str!(
        "../../tests/fixtures/github_build_provenance_jdx_mise.json"
    ))
    .unwrap();
    let mut roots = TrustRoots::default();

    let real_signer = CosignIdentity {
        identity: Some(FIXTURE_IDENTITY.to_string()),
        oidc_issuer: Some(FIXTURE_ISSUER.to_string()),
        ..Default::default()
    };
    verify_keyless_bundle(Artifact::from(&digest), &bundle, &real_signer, &mut roots)
        .await
        .unwrap();

    // The bundle is validly signed, but not by the pinned signer.
    let other_signer = CosignIdentity {
        identity_regexp: Some(r"^https://github\.com/attacker/".to_string()),
        ..Default::default()
    };
    let err = verify_keyless_bundle(Artifact::from(&digest), &bundle, &other_signer, &mut roots)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AttestationError::WorkflowMismatch(_)),
        "{err}"
    );
}
