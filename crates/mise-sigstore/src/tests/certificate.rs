use super::*;

#[test]
fn select_tuf_config_default_uses_production_url() {
    // No override → canonical Sigstore public-good TUF URL (default behavior).
    assert_eq!(DEFAULT_TUF_URL, SigstoreInstance::PublicGood.tuf_url());
    assert!(format!("{:?}", select_tuf_config(None)).contains(DEFAULT_TUF_URL));
}

#[test]
fn select_tuf_config_override_uses_mirror_url() {
    // Override → the mirror URL, while still pinning the production root.
    let mirror = "https://tuf-mirror.example.com/".to_string();
    assert!(format!("{:?}", select_tuf_config(Some(mirror.clone()))).contains(&mirror));
}

#[test]
fn attestations_url_includes_predicate_type() {
    let client = AttestationClient::builder()
        .base_url("https://api.github.com")
        .build()
        .unwrap();
    let url = client
        .attestations_url(&FetchParams {
            owner: "owner".to_string(),
            repo: Some("owner/repo".to_string()),
            digest: "sha256:abc".to_string(),
            limit: 30,
            predicate_type: Some("https://slsa.dev/provenance/v1".to_string()),
        })
        .unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

    assert_eq!(url.path(), "/repos/owner/repo/attestations/sha256:abc");
    assert_eq!(query.get("per_page").map(String::as_str), Some("30"));
    assert_eq!(
        query.get("predicate_type").map(String::as_str),
        Some("https://slsa.dev/provenance/v1")
    );
}

fn fixture_bundle(json: &str) -> Bundle {
    serde_json::from_str(json).unwrap()
}

#[test]
fn build_provenance_names_its_source_repository() {
    let bundle = fixture_bundle(include_str!(
        "../../tests/fixtures/github_build_provenance_jdx_mise.json"
    ));
    assert_eq!(
        bundle_source_repository(&bundle).as_deref(),
        Some("jdx/mise")
    );
}

#[test]
fn github_release_attestation_names_its_repository() {
    let bundle = fixture_bundle(include_str!(
        "../../tests/fixtures/github_release_attestation_jdx_mise.json"
    ));
    assert_eq!(
        bundle_source_repository(&bundle).as_deref(),
        Some("jdx/mise")
    );
}

#[tokio::test]
async fn fixture_bundles_verify_against_their_artifact() {
    let _tuf_root = TUF_ROOT_LOCK.lock().await;
    // mise-v2026.9.12-linux-x64.tar.gz, which both fixtures attest.
    let digest =
        Sha256Hash::from_hex("b4058dece685259910d3aba5782445996eea79dbdb3cf952a6eb81aadf0373ff")
            .unwrap();
    let mut trust_roots = TrustRoots::default();
    for json in [
        include_str!("../../tests/fixtures/github_build_provenance_jdx_mise.json"),
        include_str!("../../tests/fixtures/github_release_attestation_jdx_mise.json"),
    ] {
        verify_bundle_with_trust_roots(
            Artifact::from(&digest),
            &fixture_bundle(json),
            None,
            &mut trust_roots,
        )
        .await
        .unwrap();
    }
}

#[test]
fn release_statements_are_only_trusted_from_the_release_attester() {
    // A workflow certificate carrying a statement that claims to be a
    // release of another repository: the certificate's own source
    // repository wins, and the statement is never read.
    let mut bundle = fixture_bundle(include_str!(
        "../../tests/fixtures/github_build_provenance_jdx_mise.json"
    ));
    let sigstore_verify::types::SignatureContent::DsseEnvelope(envelope) = &mut bundle.content
    else {
        panic!("fixture is a DSSE bundle");
    };
    let forged = serde_json::json!({
        "_type": "https://in-toto.io/Statement/v1",
        "predicateType": "https://in-toto.io/attestation/release/v0.2",
        "subject": [],
        "predicate": {"repository": "victim/repo"},
    });
    envelope.payload = serde_json::to_vec(&forged).unwrap().into();
    assert_eq!(
        bundle_source_repository(&bundle).as_deref(),
        Some("jdx/mise")
    );
}

#[test]
fn release_statement_repository_requires_a_release_predicate() {
    let statement = |predicate_type: &str, repository: &str| {
        serde_json::to_vec(&serde_json::json!({
            "predicateType": predicate_type,
            "predicate": {"repository": repository},
        }))
        .unwrap()
    };
    assert_eq!(
        release_statement_repository(&statement(
            "https://in-toto.io/attestation/release/v0.2",
            "jdx/mise"
        ))
        .as_deref(),
        Some("jdx/mise")
    );
    assert_eq!(
        release_statement_repository(&statement("https://slsa.dev/provenance/v1", "jdx/mise")),
        None
    );
    assert_eq!(
        release_statement_repository(&statement(
            "https://in-toto.io/attestation/release/v0.2",
            "jdx/mise/../x"
        )),
        None
    );
}

#[test]
fn github_repository_from_uri_requires_owner_and_repo() {
    assert_eq!(
        github_repository_from_uri("https://github.com/jdx/mise").as_deref(),
        Some("jdx/mise")
    );
    assert_eq!(github_repository_from_uri("https://github.com/jdx"), None);
    assert_eq!(
        github_repository_from_uri("https://github.com/jdx/mise/x"),
        None
    );
    assert_eq!(
        github_repository_from_uri("https://gitlab.com/jdx/mise"),
        None
    );
}

#[test]
fn signer_workflow_requires_identity() {
    let err = verify_signer_workflow_identity(None, Some(".github/workflows/release.yml"))
        .unwrap_err()
        .to_string();

    assert!(err.contains("found no certificate identity"));
}

#[test]
fn signer_workflow_rejects_mismatch() {
    let err = verify_signer_workflow_identity(
        Some("https://github.com/jdx/mise/.github/workflows/ci.yml@refs/tags/v1.0.0"),
        Some(".github/workflows/release.yml"),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Workflow verification failed"));
}

#[test]
fn signer_workflow_accepts_match() {
    verify_signer_workflow_identity(
        Some("https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v1.0.0"),
        Some(".github/workflows/release.yml"),
    )
    .unwrap();
}

#[test]
fn signer_workflow_rejects_expected_containing_identity() {
    let err = verify_signer_workflow_identity(
        Some(".github/workflows/release.yml"),
        Some("https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v1.0.0"),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Workflow verification failed"));
}

#[test]
fn signer_workflow_accepts_repository_qualified_workflow() {
    verify_signer_workflow_identity(
        Some("https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v1.0.0"),
        Some("jdx/mise/.github/workflows/release.yml"),
    )
    .unwrap();
}

#[test]
fn signer_workflow_rejects_longer_workflow_file_name() {
    let err = verify_signer_workflow_identity(
        Some("https://github.com/evil/mise/.github/workflows/release.yml.evil.yml@refs/heads/main"),
        Some(".github/workflows/release.yml"),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Workflow verification failed"));
}

#[test]
fn signer_workflow_rejects_match_inside_ref() {
    let err = verify_signer_workflow_identity(
        Some(
            "https://github.com/evil/mise/.github/workflows/ci.yml@refs/heads/x/.github/workflows/release.yml",
        ),
        Some(".github/workflows/release.yml"),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Workflow verification failed"));
}

#[test]
fn signer_workflow_rejects_partial_leading_segment() {
    let err = verify_signer_workflow_identity(
        Some("https://github.com/jdx/mise/.github/workflows/prerelease.yml@refs/tags/v1.0.0"),
        Some("release.yml"),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Workflow verification failed"));
}

#[test]
fn signer_workflow_rejects_empty_expected() {
    let err = verify_signer_workflow_identity(
        Some("https://github.com/jdx/mise/.github/workflows/release.yml@refs/tags/v1.0.0"),
        Some(""),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Workflow verification failed"));
}

#[test]
fn signer_workflow_accepts_at_sign_in_workflow_file_name() {
    verify_signer_workflow_identity(
        Some("https://github.com/jdx/mise/.github/workflows/release@prod.yml@refs/tags/v1.0.0"),
        Some(".github/workflows/release@prod.yml"),
    )
    .unwrap();
}

#[test]
fn signer_workflow_accepts_at_sign_in_ref() {
    verify_signer_workflow_identity(
        Some("https://github.com/jdx/mise/.github/workflows/release.yml@refs/heads/a@b"),
        Some(".github/workflows/release.yml"),
    )
    .unwrap();
}

#[test]
fn signer_workflow_rejects_match_after_at_refs_inside_ref() {
    let err = verify_signer_workflow_identity(
        Some(
            "https://github.com/evil/mise/.github/workflows/ci.yml@refs/heads/x/.github/workflows/release.yml@refs/z",
        ),
        Some(".github/workflows/release.yml"),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Workflow verification failed"));
}
