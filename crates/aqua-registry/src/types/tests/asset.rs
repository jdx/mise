use super::*;

#[test]
fn test_url_no_double_exe_extension() {
    // When a Windows override URL already ends in .exe, complete_windows_ext
    // should not append another .exe (which would produce .exe.exe).
    let pkg = AquaPackage {
        url: "https://example.com/tool/{{.Version}}/tool.exe".to_string(),
        format: "raw".to_string(),
        complete_windows_ext: Some(true),
        ..Default::default()
    };

    let url = pkg.url("1.0.0", "windows", "amd64").unwrap();
    assert!(
        !url.ends_with(".exe.exe"),
        "URL should not have double .exe extension, got: {url}"
    );
    assert!(url.ends_with(".exe"));
}

#[test]
fn test_url_adds_exe_when_missing() {
    // When a Windows URL does not end in .exe and format is raw,
    // complete_windows_ext should append .exe.
    let pkg = AquaPackage {
        url: "https://example.com/tool/{{.Version}}/tool".to_string(),
        format: "raw".to_string(),
        complete_windows_ext: Some(true),
        ..Default::default()
    };

    let url = pkg.url("1.0.0", "windows", "amd64").unwrap();
    assert!(
        url.ends_with(".exe"),
        "URL should end with .exe, got: {url}"
    );
}

#[test]
fn test_url_preserves_jar_when_completing_windows_ext() {
    let pkg = AquaPackage {
        url: "https://example.com/tool/{{.Version}}/tool.jar".to_string(),
        format: "raw".to_string(),
        complete_windows_ext: Some(true),
        ..Default::default()
    };

    let url = pkg.url("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(url, "https://example.com/tool/1.0.0/tool.jar");
}

#[test]
fn test_asset_appends_format_ext_by_default() {
    let pkg = AquaPackage {
        asset: "tool-{{.Version}}-{{.OS}}-{{.Arch}}".to_string(),
        format: "tar.gz".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "linux", "amd64").unwrap();

    assert_eq!(asset, "tool-1.0.0-linux-amd64.tar.gz");
}

#[test]
fn test_asset_respects_append_ext_false() {
    let pkg = AquaPackage {
        asset: "tool-{{.Version}}-{{.OS}}-{{.Arch}}".to_string(),
        format: "tar.gz".to_string(),
        append_ext: Some(false),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "linux", "amd64").unwrap();

    assert_eq!(asset, "tool-1.0.0-linux-amd64");
}

#[test]
fn test_asset_uses_custom_windows_ext() {
    let pkg = AquaPackage {
        asset: "tool-{{.Version}}-{{.OS}}-{{.Arch}}".to_string(),
        format: "raw".to_string(),
        windows_ext: ".bat".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(asset, "tool-1.0.0-windows-amd64.bat");
}

#[test]
fn test_asset_normalizes_custom_windows_ext() {
    let pkg = AquaPackage {
        asset: "tool-{{.Version}}-{{.OS}}-{{.Arch}}".to_string(),
        format: "raw".to_string(),
        windows_ext: "bat".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(asset, "tool-1.0.0-windows-amd64.bat");
}

#[test]
fn test_asset_omitted_format_preserves_existing_extension() {
    let pkg = AquaPackage {
        asset: "tool-{{.Version}}.ps1".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(asset, "tool-1.0.0.ps1");
}

#[test]
fn test_format_detects_7z_asset() {
    let pkg = AquaPackage {
        asset: "tool-{{.Version}}-{{.OS}}-{{.Arch}}.7z".to_string(),
        ..Default::default()
    };

    let format = pkg.format("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(format, "7z");
}

#[test]
fn test_format_detects_literal_aqua_aliases() {
    let cases = [
        ("tool-{{.Version}}.tgz", "tgz"),
        ("tool-{{.Version}}.txz", "txz"),
        ("tool-{{.Version}}.tbz", "tbz"),
        ("tool-{{.Version}}.tbz2", "tbz2"),
    ];

    for (asset, expected) in cases {
        let pkg = AquaPackage {
            asset: asset.to_string(),
            ..Default::default()
        };

        let format = pkg.format("1.0.0", "linux", "amd64").unwrap();

        assert_eq!(format, expected);
    }
}

#[test]
fn test_format_preserves_explicit_aqua_aliases() {
    for format in ["tgz", "txz", "tbz", "tbz2"] {
        let pkg = AquaPackage {
            format: format.to_string(),
            ..Default::default()
        };

        let detected = pkg.format("1.0.0", "linux", "amd64").unwrap();

        assert_eq!(detected, format);
    }
}

#[test]
fn test_asset_completion_strips_version_from_filename_only() {
    let pkg = AquaPackage {
        asset: "https://example.com/1.0.0/tool-{{.Version}}".to_string(),
        format: "raw".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(asset, "https://example.com/1.0.0/tool-1.0.0.exe");
}

#[test]
fn test_asset_completion_preserves_prefix_before_non_boundary_version() {
    let pkg = AquaPackage {
        asset: "x1.8atool_{{.Version}}_win".to_string(),
        format: "raw".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.8", "windows", "amd64").unwrap();

    assert_eq!(asset, "x1.8atool_1.8_win.exe");
}

#[test]
fn test_asset_completion_treats_version_dot_as_empty_extension() {
    let pkg = AquaPackage {
        asset: "tool.{{.Version}}".to_string(),
        format: "raw".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(asset, "tool.1.0.0.exe");
}

#[test]
fn test_asset_completion_does_not_corrupt_version_prefixes() {
    let pkg = AquaPackage {
        asset: "tool-1.1.1".to_string(),
        format: "raw".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.1", "windows", "amd64").unwrap();

    assert_eq!(asset, "tool-1.1.1.exe");
}

#[test]
fn test_github_content_does_not_complete_windows_ext_by_default() {
    let pkg = AquaPackage {
        r#type: Some(AquaPackageType::GithubContent),
        path: Some("install".to_string()),
        asset: "install".to_string(),
        format: "raw".to_string(),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(asset, "install");
}

#[test]
fn test_github_content_complete_windows_ext_defaults_to_sh() {
    let pkg = AquaPackage {
        r#type: Some(AquaPackageType::GithubContent),
        path: Some("install".to_string()),
        asset: "install".to_string(),
        format: "raw".to_string(),
        complete_windows_ext: Some(true),
        ..Default::default()
    };

    let asset = pkg.asset("1.0.0", "windows", "amd64").unwrap();

    assert_eq!(asset, "install.sh");
}

#[test]
fn test_file_src_and_dst_complete_windows_ext() {
    let pkg = AquaPackage::default();

    assert_eq!(
        pkg.complete_windows_ext_to_file_src("tool_1.0.0", "v1.0.0", "windows"),
        "tool_1.0.0.exe"
    );
    assert_eq!(
        pkg.complete_windows_ext_to_file_src("tool_1.0.0.bat", "v1.0.0", "windows"),
        "tool_1.0.0.bat"
    );
    assert_eq!(
        pkg.complete_windows_ext_to_file_dst("tool_1.0.0.bat", "tool_1.0.0", "v1.0.0", "windows"),
        "tool_1.0.0.bat"
    );
    assert_eq!(
        pkg.complete_windows_ext_to_file_dst("tool_1.0.0", "tool_1.0.0", "v1.0.0", "windows"),
        "tool_1.0.0.exe"
    );
}

#[test]
fn test_asset_strs_no_double_exe_extension() {
    // asset_strs should also not double .exe when asset already ends in .exe.
    let pkg = AquaPackage {
        asset: "tool.exe".to_string(),
        format: "raw".to_string(),
        complete_windows_ext: Some(true),
        ..Default::default()
    };

    let strs = pkg.asset_strs("1.0.0", "windows", "amd64").unwrap();
    for s in &strs {
        assert!(
            !s.ends_with(".exe.exe"),
            "Asset string should not have double .exe, got: {s}"
        );
    }
}

#[test]
fn test_windows_arm64_fallback_applies_amd64_replacement() {
    let pkg = AquaPackage {
        asset: "tool-{{.OS}}-{{.Arch}}-{{.GOARCH}}.zip".to_string(),
        format: "zip".to_string(),
        replacements: HashMap::from([
            ("windows".to_string(), "win32".to_string()),
            ("amd64".to_string(), "x64".to_string()),
        ]),
        ..Default::default()
    };

    let strs = pkg.asset_strs("1.0.0", "windows", "arm64").unwrap();

    assert!(strs.contains("tool-win32-arm64-arm64.zip"));
    assert!(strs.contains("tool-win32-x64-x64.zip"));
    assert!(!strs.iter().any(|asset| asset.contains("amd64")));
}

#[test]
fn test_aqua_file_link_template() {
    let pkg = AquaPackage {
        repo_owner: "example".to_string(),
        repo_name: "tool".to_string(),
        asset: "tool-{{.Version}}.tar.gz".to_string(),
        ..Default::default()
    };
    let file = AquaFile {
        name: "tool".to_string(),
        link: Some("{{.FileName}}-alias".to_string()),
        ..Default::default()
    };

    let result = file.link(&pkg, "1.0.0", "linux", "amd64").unwrap();
    assert_eq!(result, Some("tool-alias".to_string()));
}
