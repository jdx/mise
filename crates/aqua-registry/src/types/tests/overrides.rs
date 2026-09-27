use super::*;

#[test]
fn test_override_variants_match_linux_libc() {
    let yml = r#"
packages:
  - url: https://example.com/tool-{{.Version}}-{{.OS}}-{{.Arch}}-gnu
    format: raw
    overrides:
      - goos: linux
        variants:
          - key: libc
            value: gnu
      - goos: linux
        url: https://example.com/tool-{{.Version}}-{{.OS}}-{{.Arch}}-musl
        variants:
          - key: libc
            value: musl
"#;
    let pkg = first_registry_package(yml);

    let gnu = pkg
        .clone()
        .with_version_libc(&["1.0.0"], "linux", "amd64", Some("gnu"));
    let musl = pkg.with_version_libc(&["1.0.0"], "linux", "amd64", Some("musl"));

    assert_eq!(
        gnu.url("1.0.0", "linux", "amd64").unwrap(),
        "https://example.com/tool-1.0.0-linux-amd64-gnu"
    );
    assert_eq!(
        musl.url("1.0.0", "linux", "amd64").unwrap(),
        "https://example.com/tool-1.0.0-linux-amd64-musl"
    );
}

#[test]
fn test_unconditional_override_resolves_package_type_without_version() {
    let yml = r#"
packages:
  - type: github_release
    version_constraint: "false"
    version_overrides:
      - version_constraint: Version == "v1.0.0"
        type: cargo
        crate: historical-tool
      - version_constraint: "true"
        type: go_build
"#;
    let (pkg, root_package) = first_registry_package(yml).with_unconditional_version_override();

    assert_eq!(pkg.package_type(), AquaPackageType::GoBuild);
    assert_eq!(pkg.crate_name, None);
    assert!(!root_package);
}

#[test]
fn test_unconditional_root_does_not_apply_version_fallback() {
    let yml = r#"
packages:
  - type: github_release
    version_overrides:
      - version_constraint: "true"
        type: cargo
        crate: tool
"#;
    let (pkg, root_package) = first_registry_package(yml).with_unconditional_version_override();

    assert_eq!(pkg.package_type(), AquaPackageType::GithubRelease);
    assert_eq!(pkg.crate_name, None);
    assert!(root_package);
}

#[test]
fn test_conditional_root_does_not_apply_version_fallback_without_version() {
    let yml = r#"
packages:
  - type: go_install
    version_constraint: semver(">= 1.2.0")
    version_overrides:
      - version_constraint: "true"
        type: github_release
"#;
    let (pkg, root_package) = first_registry_package(yml).with_unconditional_version_override();

    assert_eq!(pkg.package_type(), AquaPackageType::GoInstall);
    assert!(root_package);
}

#[test]
fn test_platform_overrides_expose_normalized_libc_selector() {
    let yml = r#"
packages:
  - type: github_release
    overrides:
      - goos: linux
        variants:
          - key: libc
            value: glibc
        type: cargo
        crate: platform-tool
"#;
    let pkg = first_registry_package(yml);
    let package_override = pkg.platform_overrides().into_iter().next().unwrap();

    assert_eq!(package_override.goos.as_deref(), Some("linux"));
    assert_eq!(package_override.libc.as_deref(), Some("gnu"));
    assert_eq!(
        package_override.package.package_type(),
        AquaPackageType::Cargo
    );
    assert_eq!(
        package_override.package.crate_name.as_deref(),
        Some("platform-tool")
    );
}

#[test]
fn test_unconditional_override_applies_matching_platform_type() {
    let yml = r#"
packages:
  - type: github_release
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        overrides:
          - envs:
              - darwin
              - windows
            type: cargo
            crate: platform-tool
"#;
    let (effective, _) = first_registry_package(yml).with_unconditional_version_override();

    assert_eq!(effective.package_type(), AquaPackageType::GithubRelease);
    let package_override = effective.platform_overrides().into_iter().next().unwrap();
    assert_eq!(
        package_override.envs,
        vec!["darwin".to_string(), "windows".to_string()]
    );
    assert_eq!(
        package_override.package.package_type(),
        AquaPackageType::Cargo
    );
    assert_eq!(
        package_override.package.crate_name.as_deref(),
        Some("platform-tool")
    );
}

#[test]
fn test_override_envs_match_os() {
    let yml = r#"
packages:
  - files:
      - name: catalina.sh
        src: apache-tomcat-{{.SemVer}}/bin/catalina.sh
    overrides:
      - envs:
          - windows
        files:
          - name: catalina.bat
            src: apache-tomcat-{{.SemVer}}/bin/catalina.bat
"#;
    let pkg = first_registry_package(yml);

    let linux = pkg.clone().with_version(&["10.1.0"], "linux", "amd64");
    let windows = pkg.with_version(&["10.1.0"], "windows", "amd64");

    assert_eq!(linux.files[0].name, "catalina.sh");
    assert_eq!(windows.files[0].name, "catalina.bat");
}

#[test]
fn test_override_envs_match_os_arch() {
    let yml = r#"
packages:
  - url: https://example.com/tool-default
    format: raw
    complete_windows_ext: false
    overrides:
      - envs:
          - darwin
          - windows/arm64
        url: https://example.com/tool-cargo
"#;
    let pkg = first_registry_package(yml);

    let darwin = pkg.clone().with_version(&["1.0.0"], "darwin", "amd64");
    let windows_arm64 = pkg.clone().with_version(&["1.0.0"], "windows", "arm64");
    let windows_amd64 = pkg.with_version(&["1.0.0"], "windows", "amd64");

    assert_eq!(
        darwin.url("1.0.0", "darwin", "amd64").unwrap(),
        "https://example.com/tool-cargo"
    );
    assert_eq!(
        windows_arm64.url("1.0.0", "windows", "arm64").unwrap(),
        "https://example.com/tool-cargo"
    );
    assert_eq!(
        windows_amd64.url("1.0.0", "windows", "amd64").unwrap(),
        "https://example.com/tool-default"
    );
}

#[test]
fn test_override_envs_all_matches_any_platform() {
    let yml = r#"
packages:
  - url: https://example.com/tool-default
    format: raw
    overrides:
      - envs:
          - all
        url: https://example.com/tool-all
"#;
    let pkg = first_registry_package(yml);

    // What is under test is that the `envs: [all]` override supplied the URL on every
    // platform. A raw-format URL additionally picks up the Windows executable extension,
    // which `test_url_adds_exe_when_missing` covers on its own.
    for (os, arch, expected) in [
        ("linux", "amd64", "https://example.com/tool-all"),
        ("darwin", "arm64", "https://example.com/tool-all"),
        ("windows", "amd64", "https://example.com/tool-all.exe"),
    ] {
        let resolved = pkg.clone().with_version(&["1.0.0"], os, arch);

        assert_eq!(resolved.url("1.0.0", os, arch).unwrap(), expected);
    }
}

#[test]
fn test_override_envs_combine_with_goarch() {
    let yml = r#"
packages:
  - url: https://example.com/tool-default
    format: raw
    overrides:
      - goarch: arm64
        envs:
          - linux
        url: https://example.com/tool-linux-arm64
"#;
    let pkg = first_registry_package(yml);

    let linux_amd64 = pkg.clone().with_version(&["1.0.0"], "linux", "amd64");
    let linux_arm64 = pkg.clone().with_version(&["1.0.0"], "linux", "arm64");
    let darwin_arm64 = pkg.with_version(&["1.0.0"], "darwin", "arm64");

    assert_eq!(
        linux_amd64.url("1.0.0", "linux", "amd64").unwrap(),
        "https://example.com/tool-default"
    );
    assert_eq!(
        linux_arm64.url("1.0.0", "linux", "arm64").unwrap(),
        "https://example.com/tool-linux-arm64"
    );
    assert_eq!(
        darwin_arm64.url("1.0.0", "darwin", "arm64").unwrap(),
        "https://example.com/tool-default"
    );
}

#[test]
fn test_format_overrides_apply_by_os_before_rendering() {
    let yml = r#"
packages:
  - asset: tool-{{.Version}}-{{.OS}}-{{.Arch}}.{{.Format}}
    format: tar.gz
    format_overrides:
      - goos: windows
        format: zip
"#;
    let pkg = first_registry_package(yml);

    let linux = pkg.clone().with_version(&["1.0.0"], "linux", "amd64");
    let windows = pkg.with_version(&["1.0.0"], "windows", "amd64");

    assert_eq!(linux.format("1.0.0", "linux", "amd64").unwrap(), "tar.gz");
    assert_eq!(
        linux.asset("1.0.0", "linux", "amd64").unwrap(),
        "tool-1.0.0-linux-amd64.tar.gz"
    );
    assert_eq!(windows.format("1.0.0", "windows", "amd64").unwrap(), "zip");
    assert_eq!(
        windows.asset("1.0.0", "windows", "amd64").unwrap(),
        "tool-1.0.0-windows-amd64.zip"
    );
}

#[test]
fn test_platform_override_can_override_format_override() {
    let yml = r#"
packages:
  - asset: tool-{{.Version}}-{{.OS}}-{{.Arch}}.{{.Format}}
    format: tar.gz
    format_overrides:
      - goos: windows
        format: zip
    overrides:
      - goos: windows
        asset: tool.exe
        format: raw
"#;
    let pkg = first_registry_package(yml).with_version(&["1.0.0"], "windows", "amd64");

    assert_eq!(pkg.format("1.0.0", "windows", "amd64").unwrap(), "raw");
    assert_eq!(pkg.asset("1.0.0", "windows", "amd64").unwrap(), "tool.exe");
}

#[test]
fn test_override_variants_skip_unknown_keys() {
    let yml = r#"
packages:
  - url: https://example.com/tool-default
    format: raw
    overrides:
      - goos: linux
        url: https://example.com/tool-avx2
        variants:
          - key: cpu
            value: avx2
      - goos: linux
        url: https://example.com/tool-musl
        variants:
          - key: libc
            value: musl
"#;
    let pkg =
        first_registry_package(yml).with_version_libc(&["1.0.0"], "linux", "amd64", Some("musl"));

    assert_eq!(
        pkg.url("1.0.0", "linux", "amd64").unwrap(),
        "https://example.com/tool-musl"
    );
}

#[test]
fn test_override_variants_do_not_match_without_runtime_libc() {
    let yml = r#"
packages:
  - url: https://example.com/tool-default
    format: raw
    overrides:
      - goos: linux
        url: https://example.com/tool-musl
        variants:
          - key: libc
            value: musl
"#;
    let pkg = first_registry_package(yml).with_version(&["1.0.0"], "linux", "amd64");

    assert_eq!(
        pkg.url("1.0.0", "linux", "amd64").unwrap(),
        "https://example.com/tool-default"
    );
}
