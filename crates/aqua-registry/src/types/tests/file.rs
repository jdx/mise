use super::*;

#[test]
fn test_aqua_file_src_gradle() {
    // Test the gradle package src template: {{.AssetWithoutExt | trimSuffix "-bin"}}/bin/gradle
    let pkg = AquaPackage {
        repo_owner: "gradle".to_string(),
        repo_name: "gradle-distributions".to_string(),
        asset: "gradle-{{trimV .Version}}-bin.zip".to_string(),
        ..Default::default()
    };
    let file = AquaFile {
        name: "gradle".to_string(),
        src: Some("{{.AssetWithoutExt | trimSuffix \"-bin\"}}/bin/gradle".to_string()),
        ..Default::default()
    };

    let result = file.src(&pkg, "8.14.3", "darwin", "arm64").unwrap();
    assert_eq!(result, Some("gradle-8.14.3/bin/gradle".to_string()));
}

#[test]
fn test_aqua_file_src_asset_without_ext_strips_zst() {
    let pkg = AquaPackage {
        repo_owner: "openai".to_string(),
        repo_name: "codex".to_string(),
        asset: "codex-{{.Arch}}-{{.OS}}.exe.{{.Format}}".to_string(),
        format: "zst".to_string(),
        replacements: HashMap::from([
            ("amd64".to_string(), "x86_64".to_string()),
            ("windows".to_string(), "pc-windows-msvc".to_string()),
        ]),
        ..Default::default()
    };
    let file = AquaFile {
        name: "codex".to_string(),
        src: Some("{{.AssetWithoutExt}}".to_string()),
        ..Default::default()
    };

    let result = file.src(&pkg, "0.133.0", "windows", "amd64").unwrap();

    assert_eq!(result, Some("codex-x86_64-pc-windows-msvc.exe".to_string()));
}

#[test]
fn test_asset_without_ext_uses_aqua_asset_formats() {
    assert_eq!(
        asset_without_ext("tfcmt_linux_amd64.tar.gz"),
        "tfcmt_linux_amd64"
    );
    assert_eq!(
        asset_without_ext("tfcmt_linux_amd64.tgz"),
        "tfcmt_linux_amd64"
    );
    assert_eq!(
        asset_without_ext("tfcmt_linux_amd64.tbz"),
        "tfcmt_linux_amd64"
    );
    assert_eq!(asset_without_ext("tool.tar.br"), "tool");
    assert_eq!(
        asset_without_ext("codex-x86_64-pc-windows-msvc.exe.zst"),
        "codex-x86_64-pc-windows-msvc.exe"
    );
    assert_eq!(asset_without_ext("tfcmt.js"), "tfcmt.js");
    assert_eq!(
        asset_without_ext("tfcmt_windows_amd64.exe"),
        "tfcmt_windows_amd64.exe"
    );
}

#[test]
fn test_aqua_file_src_empty_asset_produces_absolute_path() {
    // When a linked version name like "brew" matches a wrong version_override
    // that has no asset field, the package ends up with an empty asset.
    // The src template "{{.AssetWithoutExt}}/name" then renders to "/name"
    // which is an absolute path — this caused a StripPrefixError panic
    // in the aqua backend's list_bin_paths.
    let pkg = AquaPackage {
        repo_owner: "mozilla".to_string(),
        repo_name: "sccache".to_string(),
        asset: String::new(),
        ..Default::default()
    };
    let file = AquaFile {
        name: "sccache".to_string(),
        src: Some("{{.AssetWithoutExt}}/sccache".to_string()),
        ..Default::default()
    };

    let result = file.src(&pkg, "brew", "darwin", "arm64").unwrap();
    assert_eq!(result, Some("/sccache".to_string()));
}
