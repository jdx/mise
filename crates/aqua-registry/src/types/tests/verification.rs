use super::*;

#[test]
fn test_github_artifact_attestations_predicate_type() {
    let pkg = first_registry_package(
        r#"
packages:
  - github_artifact_attestations:
      enabled: true
      predicate_type: https://slsa.dev/provenance/v1
      signer_workflow: canonical-workflow.yml
"#,
    );

    let attestations = pkg.github_artifact_attestations.unwrap();
    assert_eq!(attestations.enabled, Some(true));
    assert_eq!(
        attestations.predicate_type.as_deref(),
        Some("https://slsa.dev/provenance/v1")
    );
    assert_eq!(
        attestations.signer_workflow.as_deref(),
        Some("canonical-workflow.yml")
    );
}

#[test]
fn test_checksum_nested_verification_fields_are_deserialized() {
    let pkg = first_registry_package(
        r#"
packages:
  - checksum:
      type: github_release
      asset: checksums.txt
      algorithm: sha256
      replacements:
        darwin: mac
      minisign:
        type: github_release
        asset: checksums.txt.minisig
        public_key: minisign-public-key
      github_artifact_attestations:
        enabled: true
        predicate_type: https://slsa.dev/provenance/v1
        signer_workflow: checksum-workflow.yml
"#,
    );

    let checksum = pkg.checksum.unwrap();
    assert_eq!(
        checksum.replacements.as_ref().and_then(|r| r.get("darwin")),
        Some(&"mac".to_string())
    );
    assert_eq!(
        checksum
            .minisign
            .as_ref()
            .and_then(|m| m.public_key.as_deref()),
        Some("minisign-public-key")
    );
    let attestations = checksum.github_artifact_attestations.unwrap();
    assert_eq!(attestations.enabled, Some(true));
    assert_eq!(
        attestations.predicate_type.as_deref(),
        Some("https://slsa.dev/provenance/v1")
    );
    assert_eq!(
        attestations.signer_workflow.as_deref(),
        Some("checksum-workflow.yml")
    );
}

#[test]
fn test_checksum_replacements_override_package_replacements() {
    let pkg = first_registry_package(
        r#"
packages:
  - type: http
    url: https://example.com/tool-{{.OS}}-{{.Arch}}.tar.gz
    replacements:
      darwin: macOS
      arm64: aarch64
    checksum:
      type: http
      url: "{{.AssetURL}}.{{.OS}}-{{.Arch}}.checksums"
      replacements:
        darwin: mac
        arm64: arm64
"#,
    );

    assert_eq!(
        pkg.url("1.0.0", "darwin", "arm64").unwrap(),
        "https://example.com/tool-macOS-aarch64.tar.gz"
    );
    assert_eq!(
        pkg.checksum
            .as_ref()
            .unwrap()
            .url(&pkg, "1.0.0", "darwin", "arm64")
            .unwrap(),
        "https://example.com/tool-macOS-aarch64.tar.gz.mac-arm64.checksums"
    );
}

#[test]
fn test_checksum_replacements_merge_on_version_override() {
    let mut checksum = first_registry_package(
        r#"
packages:
  - checksum:
      type: http
      url: https://example.com/{{.OS}}.checksums
      replacements:
        darwin: mac
"#,
    )
    .checksum
    .unwrap();
    let override_checksum = first_registry_package(
        r#"
packages:
  - checksum:
      replacements:
        arm64: aarch64
"#,
    )
    .checksum
    .unwrap();
    checksum.merge(override_checksum);
    assert_eq!(
        checksum.replacements,
        Some(HashMap::from([
            ("darwin".to_string(), "mac".to_string()),
            ("arm64".to_string(), "aarch64".to_string()),
        ]))
    );
}

#[test]
fn test_checksum_replacements_null_deserializes_to_none() {
    let pkg = first_registry_package(
        r#"
packages:
  - checksum:
      type: github_release
      asset: checksums.txt
      algorithm: sha256
      replacements: null
"#,
    );
    assert_eq!(pkg.checksum.unwrap().replacements, None);
}

#[test]
fn test_checksum_url_applies_replacements_to_goos_and_goarch() {
    let pkg = first_registry_package(
        r#"
packages:
  - type: http
    url: https://example.com/tool.tar.gz
    checksum:
      type: http
      url: https://example.com/{{.GOOS}}-{{.GOARCH}}.checksums
      replacements:
        darwin: mac
        arm64: aarch64
"#,
    );
    assert_eq!(
        pkg.checksum
            .as_ref()
            .unwrap()
            .url(&pkg, "1.0.0", "darwin", "arm64")
            .unwrap(),
        "https://example.com/mac-aarch64.checksums"
    );
}
