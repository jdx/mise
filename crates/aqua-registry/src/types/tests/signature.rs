use super::*;

#[test]
fn test_top_level_cosign_is_deserialized() {
    let yml = r#"
packages:
  - cosign:
      bundle:
        type: github_release
        asset: "{{.Asset}}.sigstore.json"
"#;
    let pkg = first_registry_package(yml);
    assert!(pkg.cosign.is_some());
    assert!(pkg.checksum.is_none());
}

#[test]
fn test_top_level_cosign_is_merged_from_version_override() {
    let yml = r#"
packages:
  - asset: tool-{{.Version}}-{{.OS}}-{{.Arch}}
    format: raw
    cosign:
      bundle:
        type: github_release
        asset: "{{.Asset}}.sigstore.json"
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        cosign:
          key:
            type: github_release
            asset: cosign.pub
"#;
    let pkg = first_registry_package(yml).with_version(&["v1.0.0"], "linux", "amd64");
    let cosign = pkg.cosign.unwrap();
    assert!(cosign.bundle.is_some());
    assert!(cosign.key.is_some());
}

#[test]
fn test_minisign_asset_template_uses_package_asset() {
    let yml = r#"
packages:
  - asset: minisign-{{.Version}}-{{.OS}}.tar.gz
    minisign:
      type: github_release
      asset: "{{.Asset}}.minisig"
      public_key: RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3
"#;
    let pkg = first_registry_package(yml);
    let package_asset = pkg.asset("0.12", "linux", "amd64").unwrap();
    let minisign_asset = pkg
        .minisign
        .as_ref()
        .unwrap()
        .asset(&pkg, &package_asset, "0.12", "linux", "amd64")
        .unwrap();

    assert_eq!(package_asset, "minisign-0.12-linux.tar.gz");
    assert_eq!(minisign_asset, "minisign-0.12-linux.tar.gz.minisig");
}
