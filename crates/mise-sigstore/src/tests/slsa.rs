use super::*;

const GENUINE_SIGNER: SlsaSignerIdentity<'static> = SlsaSignerIdentity {
    identity: "https://github.com/slsa-framework/slsa-github-generator/.github/workflows/generator_generic_slsa3.yml@refs/tags/v2.0.0",
    issuer: "https://token.actions.githubusercontent.com",
    source_repository: Some("getsops/sops"),
};

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
    let err = verify_intoto_envelope(&tampered, b"any artifact bytes", 1, GENUINE_SIGNER, &root)
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
    let err = verify_intoto_envelope(&stripped, b"any artifact bytes", 1, GENUINE_SIGNER, &root)
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
        GENUINE_SIGNER,
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
fn intoto_envelope_requires_exact_signer_identity_and_issuer() {
    let root = embedded_sigstore_root();
    for signer in [
        SlsaSignerIdentity {
            identity: "https://github.com/other/repo/.github/workflows/build.yml@refs/tags/v1",
            ..GENUINE_SIGNER
        },
        SlsaSignerIdentity {
            issuer: "https://accounts.google.com",
            ..GENUINE_SIGNER
        },
    ] {
        let err = verify_intoto_envelope(
            GENUINE_INTOTO_ENVELOPE.trim(),
            b"different artifact contents",
            1,
            signer,
            &root,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("signer identity") || err.contains("OIDC issuer"),
            "{err}"
        );
    }
}

#[test]
fn intoto_envelope_requires_the_tool_repository_for_a_reusable_workflow() {
    // slsa-github-generator signs with the same identity and issuer for every
    // repository that calls it, so only the certificate's source repository
    // says whose release this provenance is for.
    let root = embedded_sigstore_root();
    let verify = |source_repository| {
        verify_intoto_envelope(
            GENUINE_INTOTO_ENVELOPE.trim(),
            b"different artifact contents",
            1,
            SlsaSignerIdentity {
                source_repository,
                ..GENUINE_SIGNER
            },
            &root,
        )
        .unwrap_err()
        .to_string()
    };
    for accepted in [
        Some("getsops/sops"),
        Some("GetSops/SOPS"),
        Some("github.com/getsops/sops"),
        Some("https://github.com/getsops/sops"),
    ] {
        // The signer passed; the artifact is the only mismatch left.
        let err = verify(accepted);
        assert!(
            err.contains("not found in SLSA statement subjects"),
            "{err}"
        );
    }
    for rejected in [
        None,
        Some("example-org/example-tool"),
        Some("getsops/sops2"),
    ] {
        let err = verify(rejected);
        assert!(
            err.contains("SLSA provenance was built in getsops/sops"),
            "{err}"
        );
    }
}

#[tokio::test]
async fn unaccepted_source_repositories_name_the_signing_run() {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/sops_v3_9_0.intoto.jsonl"
    ));
    let unaccepted = async |signer| {
        slsa_provenance_unaccepted_source_repositories(path, signer)
            .await
            .unwrap()
    };
    // Expecting the name the repository had before it moved to getsops.
    assert_eq!(
        unaccepted(SlsaSignerIdentity {
            source_repository: Some("mozilla/sops"),
            ..GENUINE_SIGNER
        })
        .await,
        ["getsops/sops"]
    );
    // Nothing to look up when the source is accepted, or the signer differs.
    assert!(unaccepted(GENUINE_SIGNER).await.is_empty());
    assert!(
        unaccepted(SlsaSignerIdentity {
            issuer: "https://accounts.google.com",
            source_repository: Some("mozilla/sops"),
            ..GENUINE_SIGNER
        })
        .await
        .is_empty()
    );
}

#[test]
fn github_workflow_repository_of_identity() {
    assert_eq!(
        github_workflow_repository(
            "https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v2026.9.12"
        ),
        Some("jdx/mise")
    );
    for identity in [
        "https://github.com/jdx/.github/workflows/release.yml@refs/tags/v1",
        "https://gitlab.com/jdx/mise/.github/workflows/release.yml@refs/tags/v1",
        "https://github.com/jdx/mise",
        "user@example.com",
    ] {
        assert_eq!(github_workflow_repository(identity), None, "{identity}");
    }
}

#[test]
fn normalize_github_repository_forms() {
    for repository in [
        "jdx/mise",
        "github.com/jdx/mise",
        "https://github.com/jdx/mise",
        "https://github.com/jdx/mise/",
        "https://github.com/jdx/mise.git",
    ] {
        assert_eq!(
            normalize_github_repository(repository),
            Some("jdx/mise"),
            "{repository}"
        );
    }
    for repository in ["jdx", "https://gitlab.com/jdx/mise", "jdx/mise/extra", ""] {
        assert_eq!(
            normalize_github_repository(repository),
            None,
            "{repository}"
        );
    }
}

#[tokio::test]
async fn bundle_requires_exact_signer_identity_and_issuer() {
    let _tuf_root = TUF_ROOT_LOCK.lock().await;
    let bundle = Bundle::from_json(include_str!(
        "../../tests/fixtures/github_build_provenance_jdx_mise.json"
    ))
    .unwrap();
    let digest =
        Sha256Hash::from_hex("b4058dece685259910d3aba5782445996eea79dbdb3cf952a6eb81aadf0373ff")
            .unwrap();
    let signer = SlsaSignerIdentity {
        identity: "https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v2026.9.12",
        issuer: "https://token.actions.githubusercontent.com",
        source_repository: None,
    };
    let mut roots = TrustRoots::default();
    verify_bundle_with_slsa_signer(Artifact::from(&digest), &bundle, signer, &mut roots)
        .await
        .unwrap();

    for rejected in [
        SlsaSignerIdentity {
            identity: "https://github.com/other/repo/.github/workflows/release.yml@refs/tags/v2026.9.12",
            ..signer
        },
        SlsaSignerIdentity {
            issuer: "https://accounts.google.com",
            ..signer
        },
    ] {
        let err =
            verify_bundle_with_slsa_signer(Artifact::from(&digest), &bundle, rejected, &mut roots)
                .await
                .unwrap_err()
                .to_string();
        assert!(
            err.contains("signer identity") || err.contains("OIDC issuer"),
            "{err}"
        );
    }
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
    let err = verify_intoto_envelope(&forged, b"any artifact bytes", 1, GENUINE_SIGNER, &root)
        .unwrap_err()
        .to_string();
    assert!(
        err.to_lowercase().contains("chain")
            || err.to_lowercase().contains("trust")
            || err.to_lowercase().contains("invalid"),
        "expected chain validation failure, got {err}"
    );
}
