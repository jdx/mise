use super::*;

#[test]
fn possible_bin_names_include_version_and_platform_overrides() {
    let package = first_registry_package(
        r#"
packages:
  - repo_owner: example
    repo_name: tool
    version_constraint: "false"
    version_overrides:
      - version_constraint: semver("< 2.0.0")
        files:
          - name: old-tool
      - version_constraint: "true"
        files:
          - name: tool
          - name: tool-alias
            src: tool
            link: bin/toolctl
        overrides:
          - goos: windows
            files:
              - name: tool.exe
"#,
    );

    assert_eq!(
        package.possible_bin_names(),
        ["old-tool", "tool", "toolctl"]
    );
}

#[test]
fn possible_bin_names_fall_back_to_package_name() {
    let package = first_registry_package(
        r#"
packages:
  - name: example.com/tools/acme
    type: go_install
"#,
    );

    assert_eq!(package.possible_bin_names(), ["acme"]);
}

#[test]
fn test_registry_package_row_aliases_are_top_level_only() {
    let yml = r#"
packages:
  - name: example/canonical
    aliases:
      - name: example/alias
      - name: 123
      - other: ignored
    unsupported_field: ignored
    version_overrides:
      - aliases:
          - name: example/nested-alias
"#;
    let registry = serde_yaml::from_str::<RegistryYaml>(yml).unwrap();
    let row = registry.packages.into_iter().next().unwrap();

    assert_eq!(row.package.name.as_deref(), Some("example/canonical"));
    assert_eq!(row.aliases, vec!["example/alias"]);
    assert_eq!(row.package.version_overrides.len(), 1);
}

#[test]
fn test_registry_package_row_preserves_yaml_scalar_coercions() {
    let yml = r#"
packages:
  - replacements:
      386: i686
    vars:
      - name: enabled
        default: true
"#;
    let pkg = first_registry_package(yml);

    assert_eq!(pkg.replacements.get("386"), Some(&"i686".to_string()));
    assert_eq!(pkg.vars[0].default.as_deref(), Some("true"));
}

#[test]
fn test_registry_package_private_defaults_to_false() {
    let pkg = first_registry_package("packages:\n  - type: github_release\n");

    assert!(!pkg.private);
}

#[test]
fn test_registry_package_preserves_private() {
    let pkg = first_registry_package("packages:\n  - type: github_release\n    private: true\n");

    assert!(pkg.private);
}

#[test]
fn test_registry_package_private_survives_version_override() {
    let pkg = first_registry_package(
        r#"
packages:
  - type: github_release
    private: true
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        asset: tool.tar.gz
"#,
    )
    .with_version(&["1.0.0"], "linux", "amd64");

    assert!(pkg.private);
}

#[test]
fn test_package_type_defaults_to_github_release_when_omitted() {
    let pkg = first_registry_package("packages:\n  - name: example/tool\n");

    assert_eq!(pkg.r#type, None);
    assert_eq!(pkg.package_type(), AquaPackageType::GithubRelease);
}

#[test]
fn test_cargo_crate_survives_version_override() {
    let pkg = first_registry_package(
        r#"
packages:
  - type: github_release
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        type: cargo
        crate: example-crate
"#,
    )
    .with_version(&["1.0.0"], "linux", "amd64");

    assert_eq!(pkg.package_type(), AquaPackageType::Cargo);
    assert_eq!(pkg.crate_name.as_deref(), Some("example-crate"));
}

#[test]
fn test_version_override_can_explicitly_select_github_release() {
    let pkg = first_registry_package(
        r#"
packages:
  - type: http
    repo_owner: anthropics
    repo_name: claude-code
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        type: github_release
        asset: claude-{{.OS}}-{{.Arch}}
        format: tar.gz
        replacements:
          amd64: x64
"#,
    )
    .with_version(&["2.1.226"], "linux", "amd64");

    assert_eq!(pkg.r#type, Some(AquaPackageType::GithubRelease));
    assert_eq!(pkg.package_type(), AquaPackageType::GithubRelease);
    assert_eq!(pkg.format("2.1.226", "linux", "amd64").unwrap(), "tar.gz");
    assert_eq!(
        pkg.asset("2.1.226", "linux", "amd64").unwrap(),
        "claude-linux-x64.tar.gz"
    );
}

#[test]
fn test_omitted_version_override_type_preserves_http() {
    let pkg = first_registry_package(
        r#"
packages:
  - type: http
    url: https://example.com/tool
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        format: raw
"#,
    )
    .with_version(&["1.0.0"], "linux", "amd64");

    assert_eq!(pkg.r#type, Some(AquaPackageType::Http));
    assert_eq!(pkg.package_type(), AquaPackageType::Http);
}

#[test]
fn test_version_override_can_select_http() {
    let pkg = first_registry_package(
        r#"
packages:
  - type: github_release
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        type: http
        url: https://example.com/tool
"#,
    )
    .with_version(&["1.0.0"], "linux", "amd64");

    assert_eq!(pkg.r#type, Some(AquaPackageType::Http));
    assert_eq!(pkg.package_type(), AquaPackageType::Http);
}

#[test]
fn test_platform_override_can_explicitly_select_github_release() {
    let pkg = first_registry_package(
        r#"
packages:
  - type: http
    url: https://example.com/tool
    overrides:
      - goos: linux
        type: github_release
        asset: tool-{{.OS}}-{{.Arch}}
"#,
    );

    let linux = pkg.clone().with_version(&["1.0.0"], "linux", "amd64");
    let darwin = pkg.with_version(&["1.0.0"], "darwin", "arm64");

    assert_eq!(linux.r#type, Some(AquaPackageType::GithubRelease));
    assert_eq!(linux.package_type(), AquaPackageType::GithubRelease);
    assert_eq!(darwin.r#type, Some(AquaPackageType::Http));
    assert_eq!(darwin.package_type(), AquaPackageType::Http);
}

#[test]
fn test_emulation_flags_can_be_disabled_by_version_override() {
    let pkg = first_registry_package(
        r#"
packages:
  - asset: tool-{{.OS}}-{{.Arch}}
    rosetta2: true
    windows_arm_emulation: true
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        rosetta2: false
        windows_arm_emulation: false
"#,
    )
    .with_version(&["1.0.0"], "darwin", "arm64");

    assert_eq!(pkg.rosetta2, Some(false));
    assert_eq!(pkg.windows_arm_emulation, Some(false));
    assert_eq!(
        pkg.asset("1.0.0", "darwin", "arm64").unwrap(),
        "tool-darwin-arm64"
    );
    assert_eq!(
        pkg.asset("1.0.0", "windows", "arm64").unwrap(),
        "tool-windows-arm64.exe"
    );
}

#[test]
fn test_emulation_flags_can_be_disabled_by_platform_override() {
    let pkg = first_registry_package(
        r#"
packages:
  - asset: tool-{{.OS}}-{{.Arch}}
    rosetta2: true
    overrides:
      - goos: darwin
        rosetta2: false
"#,
    )
    .with_version(&["1.0.0"], "darwin", "arm64");

    assert_eq!(pkg.rosetta2, Some(false));
    assert_eq!(
        pkg.asset("1.0.0", "darwin", "arm64").unwrap(),
        "tool-darwin-arm64"
    );
}

#[test]
fn test_no_asset_can_be_disabled_by_version_override() {
    let pkg = first_registry_package(
        r#"
packages:
  - no_asset: true
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        no_asset: false
"#,
    )
    .with_version(&["1.0.0"], "linux", "amd64");

    assert_eq!(pkg.no_asset, Some(false));
}

#[test]
fn test_omitted_emulation_flags_preserve_base_values() {
    let pkg = first_registry_package(
        r#"
packages:
  - asset: tool-{{.OS}}-{{.Arch}}
    rosetta2: true
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        format: raw
"#,
    )
    .with_version(&["1.0.0"], "darwin", "arm64");

    assert_eq!(pkg.rosetta2, Some(true));
    assert_eq!(
        pkg.asset("1.0.0", "darwin", "arm64").unwrap(),
        "tool-darwin-amd64"
    );
}

#[test]
fn test_omitted_no_asset_preserves_base_value() {
    let pkg = first_registry_package(
        r#"
packages:
  - no_asset: true
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        format: raw
"#,
    )
    .with_version(&["1.0.0"], "linux", "amd64");

    assert_eq!(pkg.no_asset, Some(true));
}
