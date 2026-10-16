use super::*;
use aqua_registry::types::AquaSlsaProvenance;
use aqua_registry::{AquaFile, AquaVar, ParsedRegistry};

/// The regex filter [`aqua_fuzzy_match`] replaced, kept to show the two agree.
fn aqua_fuzzy_match_by_regex(
    versions: Vec<String>,
    query: &str,
    filter_prereleases: bool,
) -> Vec<String> {
    let escaped_query = regex::escape(query);
    let query = if query == "latest" {
        "\\D*[0-9].*"
    } else {
        &escaped_query
    };
    let query_regex = Regex::new(&format!("^{query}([-.].+)?$")).unwrap();
    versions
        .into_iter()
        .filter(|v| {
            if query == v {
                return true;
            }
            if filter_prereleases && VERSION_REGEX.is_match(v) {
                return false;
            }
            query_regex.is_match(v)
        })
        .collect()
}

#[test]
fn test_aqua_fuzzy_match_agrees_with_the_regex_it_replaced() {
    let queries = [
        "latest",
        "1",
        "1.2",
        "1.2.3",
        "v1",
        "v1.2",
        "go1.2",
        "jq-1",
        "jq-",
        "",
        "1.2-rc1",
        "1+x",
        "a.b",
        "é",
        "1\n",
        r"\D*[0-9].*",
    ];
    let versions: Vec<String> = [
        "",
        "1",
        "1.2",
        "1.2.3",
        "1.20",
        "1.2-rc1",
        "1.2-dev.5",
        "1.2+build",
        "1.2.",
        "1.2-",
        "1.2.\n",
        "v1.2",
        "v1.2.3",
        "go1.2.3",
        "jq-1.7",
        "jq-1.7-rc1",
        "latest",
        "x",
        "é-1",
        "٣1.0",
        "a٣1",
        "\n1",
        "1\n",
        "1.2+b.4",
        r"\D*[0-9].*",
        "a.b.c",
    ]
    .map(String::from)
    .to_vec();
    for query in queries {
        for filter_prereleases in [true, false] {
            assert_eq!(
                aqua_fuzzy_match(versions.clone(), query, filter_prereleases),
                aqua_fuzzy_match_by_regex(versions.clone(), query, filter_prereleases),
                "query {query:?} filter_prereleases {filter_prereleases}"
            );
        }
    }
}

#[test]
fn aqua_uses_go_arch_name_for_32_bit_arm() {
    assert_eq!(to_aqua_arch("arm"), "arm");
    assert_eq!(to_aqua_arch("x64"), "amd64");
    assert_eq!(to_aqua_arch("arm64"), "arm64");
}

// `--dry-run` reaches `validate` through `resolve_validated_package`, and this is the sentence
// it has to be able to produce before the install starts. Pinned here because
// `e2e/cli/test_install_dry_run_feasibility` greps for it: aqua marks specific versions
// `no_asset` regardless of platform, which is what makes that e2e runnable on any runner.
#[test]
fn validate_refuses_a_package_the_registry_marks_as_having_no_asset() {
    // `validate` reads the package it is handed; selecting the right `version_overrides` block
    // happens upstream in `package_with_options_for_pkg`. So the two cases are two packages
    // here, not two versions of one -- writing `no_asset` under `version_overrides` would
    // leave the base package unmarked and assert nothing.
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: github_release
    repo_owner: example
    repo_name: unreleased
    no_asset: true
  - type: github_release
    repo_owner: example
    repo_name: released
    asset: tool.tar.gz
"#,
    )
    .unwrap();

    let unreleased = registry.package("example/unreleased").unwrap();
    assert_eq!(
        validate(&unreleased, "v1.0.0").unwrap_err().to_string(),
        "no asset released"
    );

    // The control: an otherwise identical package that does ship an asset passes. Without it
    // this test would be satisfied by a `validate` that rejected everything.
    let released = registry.package("example/released").unwrap();
    assert!(validate(&released, "v1.0.0").is_ok());
}

#[test]
fn cargo_warning_uses_crate_name() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: cargo
    repo_owner: example
    repo_name: tool
    crate: example-crate
"#,
    )
    .unwrap();
    let pkg = registry.package("example/tool").unwrap();

    let error = validate(&pkg, "v1.0.0").unwrap_err().to_string();

    assert!(error.ends_with("Use the cargo backend instead: cargo:example-crate."));
}

#[test]
fn cargo_warning_falls_back_to_crates_io_name() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: cargo
    repo_owner: example
    repo_name: tool
    name: crates.io/example-crate
    crate: ""
"#,
    )
    .unwrap();
    let pkg = registry.package("crates.io/example-crate").unwrap();

    let error = validate(&pkg, "v1.0.0").unwrap_err().to_string();

    assert!(error.ends_with("Use the cargo backend instead: cargo:example-crate."));
}

#[test]
fn cargo_warning_escapes_terminal_control_characters() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: cargo
    repo_owner: example
    repo_name: tool
    crate: "example\u001b[2J\ncrate"
"#,
    )
    .unwrap();
    let pkg = registry.package("example/tool").unwrap();

    let error = validate(&pkg, "v1.0.0").unwrap_err().to_string();

    assert!(!error.contains('\u{1b}'));
    assert!(!error.contains('\n'));
    assert!(error.ends_with(r"Use the cargo backend instead: cargo:example\u{1b}[2J\ncrate."));
}

#[test]
fn go_install_warning_renders_versioned_path() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: go_install
    repo_owner: example
    repo_name: tool
    path: github.com/example/tool/v{{(semver .Version).Major}}/cmd/tool
"#,
    )
    .unwrap();
    let pkg = registry.package("example/tool").unwrap();

    let error = validate(&pkg, "v2.3.1").unwrap_err().to_string();

    assert!(error.ends_with("Use the go backend instead: go:github.com/example/tool/v2/cmd/tool."));
}

#[test]
fn go_install_warning_uses_repository_when_path_is_missing() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: go_install
    repo_owner: example
    repo_name: tool
"#,
    )
    .unwrap();
    let pkg = registry.package("example/tool").unwrap();

    let error = validate(&pkg, "v1.0.0").unwrap_err().to_string();

    assert!(error.ends_with("Use the go backend instead: go:github.com/example/tool."));
}

fn aqua_var(name: &str, required: bool) -> AquaVar {
    AquaVar {
        name: name.to_string(),
        default: None,
        required,
    }
}

#[test]
fn an_ordinary_cache_path_gets_no_length_note() {
    // The note is only worth printing when length is a plausible explanation, so on a normal
    // machine it must stay out of the way. On unix it never appears at all.
    let root = PathBuf::from(r"C:\Users\u\AppData\Local\Temp\mise\lock-provenance");
    assert_eq!(AquaBackend::scratch_length_hint_for(&root), "");
}

#[test]
fn symlink_bin_dir_is_not_created_without_existing_bins() {
    let temp = tempfile::tempdir().unwrap();
    let symlink_dir = temp.path().join(MISE_BINS_DIR);
    let bin = temp.path().join("missing-bin");
    let srcs = [AquaFileLink {
        src: bin.clone(),
        dst: bin.clone(),
        hard: false,
        explicit_link: false,
    }];

    AquaBackend::create_symlink_bin_dir_at(&symlink_dir, &srcs).unwrap();
    assert!(!symlink_dir.exists());

    fs::write(&bin, b"binary").unwrap();
    AquaBackend::create_symlink_bin_dir_at(&symlink_dir, &srcs).unwrap();
    assert!(symlink_dir.join("missing-bin").exists());
}

#[test]
#[cfg(windows)]
fn a_deep_cache_path_says_which_variable_to_change() {
    // Measured: at a 200-character cache, `mise install` and `mise lock` both still exit 0, so
    // a failure here comes with no other signal about why.
    let root = PathBuf::from(format!(r"C:\{}", "d".repeat(200)));
    let hint = AquaBackend::scratch_length_hint_for(&root);
    assert!(hint.contains("MISE_CACHE_DIR"), "{hint}");
    assert!(hint.contains("260"), "{hint}");
}

#[test]
fn the_lock_time_download_lands_in_a_sweepable_cache_directory() {
    // Two things at once. A lockfile is a committed artifact, so what `mise lock` writes must
    // not depend on where the machine points TEMP — and the scratch has to sit somewhere
    // ordinary cache clearing reaches, because `TempDir` cleanup is best-effort and an orphan
    // under the persistent downloads directory would never be collected.
    let expected = dirs::CACHE.join("lock-provenance");
    let dir = AquaBackend::lock_time_download_dir().unwrap();
    assert!(
        dir.path().starts_with(&expected),
        "expected {} to be under {}",
        dir.path().display(),
        expected.display()
    );
}

#[test]
fn detects_legacy_root_binary_layout() {
    let temp = tempfile::tempdir().unwrap();
    let ba = BackendArg::from("codex");
    let unrelated = temp
        .path()
        .join(format!("node{}", std::env::consts::EXE_SUFFIX));
    file::write(&unrelated, "unrelated binary").unwrap();
    file::make_executable(&unrelated).unwrap();
    assert!(!AquaBackend::has_legacy_root_bin(&ba, temp.path()));

    let codex = temp
        .path()
        .join(format!("codex{}", std::env::consts::EXE_SUFFIX));
    file::write(&codex, "codex binary").unwrap();
    file::make_executable(&codex).unwrap();
    assert!(AquaBackend::has_legacy_root_bin(&ba, temp.path()));
}

#[test]
fn legacy_fallback_only_accepts_registry_unavailability() {
    let unavailable = eyre!(AquaRegistryError::RegistryNotAvailable(
        "network unavailable".into()
    ));
    assert!(AquaBackend::is_registry_unavailable(&unavailable));

    let template = eyre!(AquaRegistryError::TemplateError(eyre!(
        "missing required variable"
    )));
    assert!(!AquaBackend::is_registry_unavailable(&template));
}

#[test]
fn test_has_github_attestations_config_requires_enabled_config() {
    let mut pkg = AquaPackage::default();
    assert!(!AquaBackend::has_github_attestations_config(&pkg));

    pkg.github_artifact_attestations = Some(AquaGithubArtifactAttestations {
        enabled: Some(false),
        predicate_type: None,
        signer_workflow: None,
    });
    assert!(!AquaBackend::has_github_attestations_config(&pkg));

    pkg.github_artifact_attestations = Some(AquaGithubArtifactAttestations {
        enabled: None,
        predicate_type: None,
        signer_workflow: None,
    });
    assert!(AquaBackend::has_github_attestations_config(&pkg));
}

fn listed_tag_test_package(package_type: AquaPackageType, url: &str) -> AquaPackage {
    let mut pkg = AquaPackage::default();
    pkg.repo_owner = "owner".to_string();
    pkg.repo_name = "repo".to_string();
    pkg.r#type = Some(package_type);
    pkg.url = url.to_string();
    pkg
}

#[test]
fn test_listed_tag_check_never_needs_the_github_api_for_url_packages() {
    // A correct hint for terraform-, kubectl- or vault-style packages costs
    // one probe of the artifact, never a GitHub API request; release-asset
    // packages confirm with the release lookup installing makes anyway.
    let backend = AquaBackend::from_arg(BackendArg::new(
        "tool".to_string(),
        Some("aqua:owner/repo".to_string()),
    ));
    let release = listed_tag_test_package(AquaPackageType::GithubRelease, "");
    assert_eq!(
        backend.listed_tag_check(&release, "v1.2.3", "linux", "amd64"),
        ListedTagCheck::Release
    );

    let http = listed_tag_test_package(
        AquaPackageType::Http,
        "https://releases.example.com/tool/{{.Version}}/tool.zip",
    );
    assert_eq!(
        backend.listed_tag_check(&http, "1.2.3", "linux", "amd64"),
        ListedTagCheck::ArtifactUrl("https://releases.example.com/tool/1.2.3/tool.zip".into())
    );

    let archive = listed_tag_test_package(AquaPackageType::GithubArchive, "");
    assert_eq!(
        backend.listed_tag_check(&archive, "v1.2.3", "linux", "amd64"),
        ListedTagCheck::ArtifactUrl(
            "https://github.com/owner/repo/archive/refs/tags/v1.2.3.tar.gz".into()
        )
    );

    let mut content = listed_tag_test_package(AquaPackageType::GithubContent, "");
    content.path = Some("bin/tool".to_string());
    assert_eq!(
        backend.listed_tag_check(&content, "v1.2.3", "linux", "amd64"),
        ListedTagCheck::ArtifactUrl(
            "https://raw.githubusercontent.com/owner/repo/v1.2.3/bin/tool".into()
        )
    );

    let mut private = http.clone();
    private.private = true;
    assert_eq!(
        backend.listed_tag_check(&private, "1.2.3", "linux", "amd64"),
        ListedTagCheck::Unconfirmable
    );
}

#[tokio::test]
async fn test_a_listed_tag_is_confirmed_for_the_platform_being_locked() {
    // The tag's artifact exists for linux-x64 but not for macos-arm64.
    // Locking linux-x64 keeps the hint whatever the host is; macos-arm64
    // drops it, so that target alone goes back to the old lookup, `v`
    // alternative included.
    let mut server = mockito::Server::new_async().await;
    let linux = server
        .mock("HEAD", "/dl/1.2.3/tool_linux_amd64.zip")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let _darwin = server
        .mock("HEAD", "/dl/1.2.3/tool_darwin_arm64.zip")
        .with_status(404)
        .create_async()
        .await;
    let _darwin_get = server
        .mock("GET", "/dl/1.2.3/tool_darwin_arm64.zip")
        .with_status(404)
        .create_async()
        .await;
    let backend = AquaBackend::from_arg(BackendArg::new(
        "tool".to_string(),
        Some("aqua:owner/repo".to_string()),
    ));
    let pkg = listed_tag_test_package(
        AquaPackageType::Http,
        &format!(
            "{}/dl/{{{{.Version}}}}/tool_{{{{.OS}}}}_{{{{.Arch}}}}.zip",
            server.url()
        ),
    );
    let raw_opts = ToolVersionOptions::default();
    let opts = AquaOptions::new(&raw_opts);
    let target = |key: &str| PlatformTarget::new(crate::platform::Platform::parse(key).unwrap());

    assert!(
        backend
            .confirm_listed_tag(pkg.clone(), "1.2.3", &target("linux-x64"), &opts)
            .await
    );
    assert!(
        !backend
            .confirm_listed_tag(pkg, "1.2.3", &target("macos-arm64"), &opts)
            .await
    );
    linux.assert_async().await;
}

#[test]
fn test_a_listed_release_must_hold_the_targets_asset() {
    // `1.2.3` and `v1.2.3` both exist, but only `v1.2.3` ships the linux
    // asset. A hint naming `1.2.3` is rejected for linux-x64, so that
    // target falls back to the old lookup and its `v` alternative.
    let backend = AquaBackend::from_arg(BackendArg::new(
        "tool".to_string(),
        Some("aqua:owner/repo".to_string()),
    ));
    let mut pkg = listed_tag_test_package(AquaPackageType::GithubRelease, "");
    pkg.asset = "tool_{{.OS}}_{{.Arch}}.tar.gz".to_string();
    let release = |tag: &str, assets: &[&str]| github::GithubRelease {
        tag_name: tag.to_string(),
        draft: false,
        prerelease: false,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        published_at: None,
        assets: assets
            .iter()
            .map(|name| github::GithubAsset {
                from_versions_host: false,
                name: name.to_string(),
                browser_download_url: String::new(),
                url: String::new(),
                digest: None,
                updated_at: None,
            })
            .collect(),
    };
    let linux = PlatformTarget::new(crate::platform::Platform::parse("linux-x64").unwrap());
    let darwin = PlatformTarget::new(crate::platform::Platform::parse("macos-arm64").unwrap());

    let bare = release("1.2.3", &["tool_darwin_arm64.tar.gz"]);
    assert!(!backend.release_has_target_asset(&pkg, "1.2.3", &bare, &linux));
    assert!(backend.release_has_target_asset(&pkg, "1.2.3", &bare, &darwin));

    let prefixed = release("v1.2.3", &["tool_linux_amd64.tar.gz"]);
    assert!(backend.release_has_target_asset(&pkg, "v1.2.3", &prefixed, &linux));
}

#[tokio::test]
async fn test_a_stale_listed_tag_leaves_the_v_alternative_in_place() {
    // The host lists 1.2.3 under tag `1.2.3`, but upstream now ships it as
    // `v1.2.3`. The hint's artifact doesn't answer, so it is not relied
    // on, and the old resolution, `v` alternative and probe included,
    // finds the artifact that exists.
    let mut server = mockito::Server::new_async().await;
    let stale = format!("{}/dl/1.2.3/tool.zip", server.url());
    let live = format!("{}/dl/v1.2.3/tool.zip", server.url());
    let _missing = server
        .mock("HEAD", "/dl/1.2.3/tool.zip")
        .with_status(404)
        .create_async()
        .await;
    let _missing_get = server
        .mock("GET", "/dl/1.2.3/tool.zip")
        .with_status(404)
        .create_async()
        .await;
    let found = server
        .mock("HEAD", "/dl/v1.2.3/tool.zip")
        .with_status(200)
        .expect_at_least(1)
        .create_async()
        .await;
    let pkg = listed_tag_test_package(
        AquaPackageType::Http,
        &format!("{}/dl/{{{{.Version}}}}/tool.zip", server.url()),
    );

    assert!(!artifact_url_exists(&stale).await);
    let (url, version) = resolve_aqua_http_url(&pkg, "1.2.3", Some("v1.2.3"), "linux", "amd64")
        .await
        .unwrap();
    assert_eq!((url.as_str(), version.as_str()), (live.as_str(), "v1.2.3"));
    // A correct hint answers its one probe and nothing else is asked.
    assert!(artifact_url_exists(&live).await);
    found.assert_async().await;
}

#[test]
fn test_version_with_prefix_does_not_double_prefix() {
    assert_eq!(version_with_prefix("1.0.0", Some("tool-")), "tool-1.0.0");
    assert_eq!(
        version_with_prefix("tool-1.0.0", Some("tool-")),
        "tool-1.0.0"
    );
}

#[tokio::test]
async fn test_private_github_download_uses_api_url_without_probe() {
    let browser_url = "not a valid URL";
    let api_url = "https://api.github.com/repos/owner/repo/releases/assets/1";

    assert_eq!(
        select_github_download_url(true, browser_url, Some(api_url)).await,
        api_url
    );
}

#[tokio::test]
async fn test_private_github_download_without_api_url_uses_browser_url() {
    let browser_url = "https://example.com/tool.tar.gz";

    assert_eq!(
        select_github_download_url(true, browser_url, None).await,
        browser_url
    );
}

#[test]
fn test_private_github_probe_uses_api_url() {
    let browser_url = "https://github.com/owner/repo/archive/refs/tags/v1.0.0.tar.gz";
    let api_url = "https://api.github.com/repos/owner/repo/tarball/v1.0.0";

    assert_eq!(
        select_github_probe_url(true, browser_url, Some(api_url)),
        api_url
    );
    assert_eq!(
        select_github_probe_url(false, browser_url, Some(api_url)),
        browser_url
    );
}

#[test]
fn test_effective_extraction_format_accepts_unsupported_aqua_formats() {
    let pkg = AquaPackage::default();

    for (format, expected) in [
        ("tar.br", ExtractionFormat::TarBr),
        ("tbr", ExtractionFormat::TarBr),
        ("br", ExtractionFormat::Br),
        ("tar.lz4", ExtractionFormat::TarLz4),
        ("tlz4", ExtractionFormat::TarLz4),
        ("lz4", ExtractionFormat::Lz4),
        ("tar.sz", ExtractionFormat::TarSz),
        ("tsz", ExtractionFormat::TarSz),
        ("sz", ExtractionFormat::Sz),
        ("rar", ExtractionFormat::Rar),
    ] {
        assert_eq!(
            AquaBackend::effective_extraction_format(&pkg, format).unwrap(),
            expected
        );
    }
}

#[test]
fn test_effective_extraction_format_rejects_unknown_aqua_formats() {
    let pkg = AquaPackage::default();

    let err = AquaBackend::effective_extraction_format(&pkg, "definitely-unknown").unwrap_err();

    assert!(
        format!("{err:#}").contains("unsupported aqua package format: definitely-unknown"),
        "{err:#}"
    );
}

#[test]
fn test_github_archive_omitted_format_defaults_to_targz() {
    let mut pkg = AquaPackage::default();
    pkg.r#type = Some(AquaPackageType::GithubArchive);

    assert_eq!(
        AquaBackend::effective_extraction_format(&pkg, "").unwrap(),
        ExtractionFormat::TarGz
    );
}

#[test]
fn test_github_archive_api_url() {
    let backend = AquaBackend::from_arg(BackendArg::new(
        "tool".to_string(),
        Some("aqua:owner/repo".to_string()),
    ));
    let mut pkg = AquaPackage::default();
    pkg.repo_owner = "owner".to_string();
    pkg.repo_name = "repo".to_string();

    assert_eq!(
        backend.github_archive_api_url(&pkg, "feature/private"),
        "https://api.github.com/repos/owner/repo/tarball/feature%2Fprivate"
    );
}

#[test]
fn test_github_content_api_url() {
    let backend = AquaBackend::from_arg(BackendArg::new(
        "tool".to_string(),
        Some("aqua:owner/repo".to_string()),
    ));
    let mut pkg = AquaPackage::default();
    pkg.repo_owner = "owner".to_string();
    pkg.repo_name = "repo".to_string();
    pkg.path = Some("bin/tool name".to_string());

    assert_eq!(
        backend.github_content_api_url(&pkg, "feature/private"),
        "https://api.github.com/repos/owner/repo/contents/bin/tool%20name?ref=feature%2Fprivate"
    );
}

#[test]
fn test_version_candidates_include_prefixed_v_tag() {
    let candidates = version_candidates("1.2.3", Some("tool/"))
        .into_iter()
        .map(|v| v.into_owned())
        .collect_vec();

    assert_eq!(candidates, vec!["tool/1.2.3", "tool/v1.2.3"]);
}

#[test]
fn test_version_candidates_include_prefixed_v_tag_for_prefixed_version() {
    let candidates = version_candidates("tool/1.2.3", Some("tool/"))
        .into_iter()
        .map(|v| v.into_owned())
        .collect_vec();

    assert_eq!(candidates, vec!["tool/1.2.3", "tool/v1.2.3"]);
}

#[test]
fn test_version_candidates_do_not_double_v_prefix() {
    let candidates = version_candidates("1.2.3", Some("tool-v"))
        .into_iter()
        .map(|v| v.into_owned())
        .collect_vec();

    assert_eq!(candidates, vec!["tool-v1.2.3"]);
}

#[test]
fn test_package_version_candidates_include_package_and_override_prefixes() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: github_release
    repo_owner: example
    repo_name: tool
    version_prefix: maven-
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        version_prefix: lychee-v
"#,
    )
    .unwrap();
    let pkg = registry.package("example/tool").unwrap();
    let candidates = package_version_candidates("1.2.3", &pkg)
        .into_iter()
        .map(Cow::into_owned)
        .collect_vec();

    assert_eq!(
        candidates,
        vec![
            "1.2.3",
            "v1.2.3",
            "maven-1.2.3",
            "maven-v1.2.3",
            "lychee-v1.2.3"
        ]
    );
}

#[test]
fn test_package_version_candidates_apply_prefixed_file_override() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: github_release
    repo_owner: lycheeverse
    repo_name: lychee
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        version_prefix: lychee-v
        asset: lychee.tar.gz
        format: tar.gz
        files:
          - name: lychee
            src: lychee-x86_64-unknown-linux-musl/lychee
"#,
    )
    .unwrap();
    let pkg = registry.package("lycheeverse/lychee").unwrap();
    let versions = package_version_candidates("0.24.2", &pkg);
    let versions = versions.iter().map(|v| v.as_ref()).collect_vec();
    let pkg = pkg.with_version(&versions, "linux", "amd64");

    assert_eq!(pkg.version_prefix.as_deref(), Some("lychee-v"));
    assert_eq!(pkg.files.len(), 1);
    assert_eq!(pkg.files[0].name, "lychee");
}

#[test]
fn test_package_version_candidates_apply_inherited_prefix_format_override() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: http
    repo_owner: haskell
    repo_name: cabal
    version_prefix: cabal-install-v
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        format: tar.xz
"#,
    )
    .unwrap();
    let pkg = registry.package("haskell/cabal").unwrap();
    let backend = Arc::new(BackendArg::new(
        "cabal".to_string(),
        Some("aqua:haskell/cabal/cabal-install".to_string()),
    ));
    let request =
        ToolRequest::new(backend, "3.16.1.0", crate::toolset::ToolSource::Unknown).unwrap();
    let tv = ToolVersion::new(request, "3.16.1.0".to_string());
    let versions = resolved_package_version_candidates("3.16.1.0", None, &pkg);
    let versions = versions.iter().map(|v| v.as_ref()).collect_vec();
    let pkg = AquaBackend::package_with_options_for_pkg(&tv, pkg, &versions).unwrap();

    assert_eq!(pkg.version_prefix.as_deref(), Some("cabal-install-v"));
    assert_eq!(pkg.format, "tar.xz");
}

#[test]
fn test_resolved_package_without_tag_selects_codex_windows_archive() {
    let pkg: AquaPackage = mise_util::yaml::from_str(
        r#"
type: github_release
repo_owner: openai
repo_name: codex
version_prefix: rust-
version_constraint: "false"
version_overrides:
  - version_constraint: semver("<= 0.133.0-alpha.1")
    asset: codex-{{.Arch}}-{{.OS}}.exe.{{.Format}}
    format: zst
  - version_constraint: "true"
    asset: codex-package-{{.Arch}}-{{.OS}}.{{.Format}}
    format: tar.gz
    files:
      - name: codex
        src: bin/codex
    replacements:
      amd64: x86_64
      windows: pc-windows-msvc
"#,
    )
    .unwrap();

    // A pinned release may be absent from GitHub's release listing. Its registry
    // prefix must still be considered before applying the version overrides.
    let pkg = lock_package_for_version(pkg, "0.154.0", None, "windows", "amd64", None).unwrap();

    assert_eq!(
        pkg.asset("rust-v0.154.0", "windows", "amd64").unwrap(),
        "codex-package-x86_64-pc-windows-msvc.tar.gz"
    );
    assert_eq!(pkg.format, "tar.gz");
    assert_eq!(pkg.files[0].src.as_deref(), Some("bin/codex"));
}

#[test]
fn test_resolved_package_known_tag_preserves_override_prefix() {
    let pkg: AquaPackage = mise_util::yaml::from_str(
        r#"
version_constraint: "false"
version_overrides:
  - version_constraint: "true"
    version_prefix: old-v
    asset: old-layout.zip
  - version_constraint: "true"
    version_prefix: new-v
    asset: new-layout.zip
"#,
    )
    .unwrap();
    let pkg = lock_package_for_version(pkg, "1.0.0", Some("new-v1.0.0"), "windows", "amd64", None)
        .unwrap();

    assert_eq!(
        pkg.asset("new-v1.0.0", "windows", "amd64").unwrap(),
        "new-layout.zip"
    );
}

#[test]
fn test_lock_package_without_tag_rejects_ambiguous_override_prefixes() {
    let pkg: AquaPackage = mise_util::yaml::from_str(
        r#"
version_constraint: "false"
version_overrides:
  - version_constraint: "true"
    version_prefix: old-v
    asset: old-layout.zip
  - version_constraint: "true"
    version_prefix: new-v
    asset: new-layout.zip
"#,
    )
    .unwrap();
    let err = lock_package_for_version(pkg, "1.0.0", None, "windows", "amd64", None).unwrap_err();
    assert!(err.to_string().contains("ambiguous aqua version prefixes"));
    assert!(err.to_string().contains("old-v, new-v"));
}

#[test]
fn test_lock_package_prefixes_with_disjoint_constraints() {
    let pkg: AquaPackage = mise_util::yaml::from_str(
        r#"
version_constraint: "false"
version_overrides:
  - version_constraint: semver("< 1.0.0")
    version_prefix: old-v
    asset: old-layout.zip
  - version_constraint: semver(">= 1.0.0")
    version_prefix: new-v
    asset: new-layout.zip
"#,
    )
    .unwrap();

    for (version, expected) in [("0.9.0", "old-layout.zip"), ("1.0.0", "new-layout.zip")] {
        let resolved =
            lock_package_for_version(pkg.clone(), version, None, "windows", "amd64", None).unwrap();
        assert_eq!(
            resolved.asset(version, "windows", "amd64").unwrap(),
            expected
        );
    }
}

#[test]
fn test_lock_package_ignores_inactive_override_prefixes() {
    let pkg: AquaPackage = mise_util::yaml::from_str(
        r#"
asset: default.zip
version_overrides:
  - version_constraint: "true"
    version_prefix: old-v
    asset: old-layout.zip
  - version_constraint: "true"
    version_prefix: new-v
    asset: new-layout.zip
"#,
    )
    .unwrap();

    let pkg = lock_package_for_version(pkg, "1.0.0", None, "windows", "amd64", None).unwrap();
    assert_eq!(
        pkg.asset("1.0.0", "windows", "amd64").unwrap(),
        "default.zip"
    );
}

#[test]
fn test_candidate_bin_paths_use_v_prefixed_package_override() {
    let registry = ParsedRegistry::parse_yaml(
        r#"
packages:
  - type: github_release
    repo_owner: sharkdp
    repo_name: fd
    files:
      - name: fd
        src: "{{.AssetWithoutExt}}/fd"
    version_constraint: "false"
    version_overrides:
      - version_constraint: Version == "v10.3.0"
        asset: fd-{{.Version}}-{{.Arch}}-{{.OS}}.{{.Format}}
        format: tar.gz
        replacements:
          amd64: x86_64
          darwin: apple-darwin
      - version_constraint: "true"
        asset: fd-{{.Version}}-{{.Arch}}-{{.OS}}.{{.Format}}
        format: tar.gz
        replacements:
          amd64: amd64
          darwin: apple-darwin
"#,
    )
    .unwrap();
    let pkg = registry.package("sharkdp/fd").unwrap();
    let versions = version_candidates("10.3.0", None);
    let versions = versions.iter().map(|v| v.as_ref()).collect_vec();
    let pkg = pkg.with_version(&versions, "darwin", "amd64");

    let candidates = AquaBackend::candidate_bin_paths_for_platform(
        &pkg,
        "10.3.0",
        Path::new("install"),
        "darwin",
        "amd64",
    )
    .unwrap();

    assert!(
        candidates.contains(&PathBuf::from("fd-v10.3.0-x86_64-apple-darwin")),
        "{candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .all(|p| !p.to_string_lossy().contains("amd64-apple-darwin")),
        "{candidates:?}"
    );
}

#[test]
fn test_complete_windows_ext_preserves_existing_extension() {
    let pkg = AquaPackage::default();
    assert_eq!(
        complete_windows_ext(PathBuf::from("bat/arq.bat"), &pkg, "windows", "1.0.0"),
        PathBuf::from("bat/arq.bat")
    );
    assert_eq!(
        complete_windows_ext(PathBuf::from("lib/tool.jar"), &pkg, "windows", "1.0.0"),
        PathBuf::from("lib/tool.jar")
    );
    assert_eq!(
        complete_windows_ext(PathBuf::from("bin/tool"), &pkg, "windows", "1.0.0"),
        PathBuf::from("bin/tool.exe")
    );
    assert_eq!(
        complete_windows_ext(PathBuf::from("bin/tool_1.0.0"), &pkg, "windows", "v1.0.0"),
        PathBuf::from("bin/tool_1.0.0.exe")
    );
    assert_eq!(
        complete_windows_ext(PathBuf::from("bin/tool.1.0.0"), &pkg, "windows", "v1.0.0"),
        PathBuf::from("bin/tool.1.0.0.exe")
    );
    assert_eq!(
        complete_windows_ext(
            PathBuf::from("bin/x1.8atool_1.8_win"),
            &pkg,
            "windows",
            "1.8"
        ),
        PathBuf::from("bin/x1.8atool_1.8_win.exe")
    );
    assert_eq!(
        complete_windows_ext(PathBuf::from("bin/tool-1.1.1"), &pkg, "windows", "1.1"),
        PathBuf::from("bin/tool-1.1.1.exe")
    );
}

#[test]
fn test_complete_windows_ext_uses_custom_windows_ext() {
    let mut pkg = AquaPackage::default();
    pkg.windows_ext = ".bat".to_string();

    assert_eq!(
        complete_windows_ext(PathBuf::from("dart-sass/sass"), &pkg, "windows", "1.0.0"),
        PathBuf::from("dart-sass/sass.bat")
    );
}

#[test]
fn test_complete_windows_ext_can_default_to_sh() {
    let mut pkg = AquaPackage::default();
    pkg.r#type = Some(AquaPackageType::GithubContent);
    pkg.complete_windows_ext = Some(true);

    assert_eq!(
        complete_windows_ext(PathBuf::from("install"), &pkg, "windows", "1.0.0"),
        PathBuf::from("install.sh")
    );
}

#[test]
fn test_complete_windows_dst_ext_uses_source_extension() {
    let pkg = AquaPackage::default();
    assert_eq!(
        complete_windows_dst_ext(
            Path::new("bat/arq.bat"),
            PathBuf::from("bat/arq"),
            &pkg,
            "windows",
            "1.0.0",
        ),
        PathBuf::from("bat/arq.bat")
    );
    assert_eq!(
        complete_windows_dst_ext(
            Path::new("bin/tool"),
            PathBuf::from("bin/tool"),
            &pkg,
            "windows",
            "1.0.0"
        ),
        PathBuf::from("bin/tool.exe")
    );
    assert_eq!(
        complete_windows_dst_ext(
            Path::new("bin/tool_1.0.0.bat"),
            PathBuf::from("bin/tool_1.0.0"),
            &pkg,
            "windows",
            "v1.0.0"
        ),
        PathBuf::from("bin/tool_1.0.0.bat")
    );
    assert_eq!(
        complete_windows_dst_ext(
            Path::new("bin/tool_1.0.0"),
            PathBuf::from("bin/tool_1.0.0"),
            &pkg,
            "windows",
            "v1.0.0"
        ),
        PathBuf::from("bin/tool_1.0.0.exe")
    );
}

fn slsa_package() -> AquaPackage {
    let mut pkg = AquaPackage::default();
    pkg.slsa_provenance = Some(AquaSlsaProvenance {
        enabled: None,
        r#type: Some("github_release".to_string()),
        repo_owner: None,
        repo_name: None,
        url: None,
        asset: Some("multiple.intoto.jsonl".to_string()),
        source_uri: None,
        source_tag: None,
        signer_identity: None,
        signer_issuer: None,
    });
    let mut over = AquaPackage::default();
    over.slsa_provenance = pkg.slsa_provenance.clone();
    pkg.version_overrides = vec![over];
    pkg
}

fn slsa_signer_opts(identity: Option<&str>, issuer: Option<&str>) -> ToolVersionOptions {
    let mut opts = ToolVersionOptions::default();
    for (key, value) in [
        ("slsa_signer_identity", identity),
        ("slsa_signer_issuer", issuer),
    ] {
        if let Some(value) = value {
            opts.opts
                .insert(key.to_string(), toml::Value::String(value.to_string()));
        }
    }
    opts
}

#[test]
fn test_identity_options_keep_signers_and_unify_var_spelling() {
    let backend = AquaBackend::from_arg(BackendArg::new(
        "osv-scanner".to_string(),
        Some("aqua:google/osv-scanner".to_string()),
    ));
    let identity = |options: &str| {
        let options: ToolVersionOptions = toml::from_str(options).unwrap();
        let request = ToolRequest::new_with_options(
            backend.ba().clone(),
            "2.6.0",
            options,
            crate::toolset::ToolSource::Argument,
        )
        .unwrap();
        backend.install_identity_options(&ToolVersion::new(request, "2.6.0".to_string()))
    };
    let base = identity("");
    assert!(base.is_empty(), "{base:?}");

    // A different signer requirement is a different request.
    assert_ne!(
        identity(
            r#"
slsa_signer_identity = "https://github.com/example/tool/.github/workflows/release.yml@refs/tags/v1.0.0"
slsa_signer_issuer = "https://token.actions.githubusercontent.com"
"#
        ),
        base
    );

    // One var, three spellings, one identity.
    let flat = identity(r#"flavor = "full""#);
    assert_eq!(flat.get("vars.flavor").map(String::as_str), Some("full"));
    assert_eq!(identity(r#""vars.flavor" = "full""#), flat);
    assert_eq!(
        identity(
            r#"
[vars]
flavor = "full"
"#
        ),
        flat
    );
    assert_ne!(flat, base);
    assert_ne!(identity(r#"flavor = "slim""#), flat);

    // `prerelease` asks for prereleases when versions are listed, it is not a var.
    assert_eq!(identity("prerelease = true"), base);

    // These change the install directory or the selected asset.
    assert_ne!(identity("symlink_bins = true"), base);
    assert_ne!(identity(r#"libc = "musl""#), base);
    assert_eq!(identity("symlink_bins = false"), base);
}

#[test]
fn test_slsa_signer_options_fill_missing_registry_signer() {
    let opts = slsa_signer_opts(
        Some("https://github.com/example/tool/.github/workflows/release.yml@refs/tags/v1.0.0"),
        Some("https://token.actions.githubusercontent.com"),
    );
    let opts = AquaOptions::new(&opts);
    let pkg = slsa_package();
    assert!(!pkg.slsa_provenance.as_ref().unwrap().has_signer_identity());

    let pkg = AquaBackend::apply_slsa_signer_options(pkg, &opts).unwrap();

    for slsa in std::iter::once(&pkg)
        .chain(pkg.version_overrides.iter())
        .map(|p| p.slsa_provenance.as_ref().unwrap())
    {
        assert_eq!(
            slsa.signer_identity.as_deref(),
            Some("https://github.com/example/tool/.github/workflows/release.yml@refs/tags/v1.0.0")
        );
        assert_eq!(
            slsa.signer_issuer.as_deref(),
            Some("https://token.actions.githubusercontent.com")
        );
    }
}

#[test]
fn test_package_options_supply_slsa_signer() {
    let backend = Arc::new(BackendArg::new(
        "osv-scanner".to_string(),
        Some("aqua:google/osv-scanner".to_string()),
    ));
    let mut request =
        ToolRequest::new(backend, "2.6.0", crate::toolset::ToolSource::Unknown).unwrap();
    request.set_options(slsa_signer_opts(
        Some("identity"),
        Some("https://token.actions.githubusercontent.com"),
    ));
    let tv = ToolVersion::new(request, "2.6.0".to_string());

    let pkg = AquaBackend::package_with_options_for_pkg(&tv, slsa_package(), &["v2.6.0"]).unwrap();

    assert!(pkg.slsa_provenance.as_ref().unwrap().has_signer_identity());
}

#[test]
fn test_locked_slsa_requires_signer_from_registry_or_options() {
    let backend = Arc::new(BackendArg::new(
        "osv-scanner".to_string(),
        Some("aqua:google/osv-scanner".to_string()),
    ));
    let request = ToolRequest::new(backend, "2.6.0", crate::toolset::ToolSource::Unknown).unwrap();
    let tv = ToolVersion::new(request, "2.6.0".to_string());
    let locked = ProvenanceType::Slsa { url: None };

    // The registry names the provenance asset but not the signer: the reported failure.
    let err =
        AquaBackend::ensure_locked_slsa_signer(&tv, Some(&locked), &slsa_package()).unwrap_err();
    assert!(
        err.to_string()
            .contains("Set slsa_signer_identity and slsa_signer_issuer"),
        "unexpected error: {err}"
    );

    // The same lock installs once the tool options supply the signer.
    let opts = slsa_signer_opts(Some("identity"), Some("issuer"));
    let pkg = AquaBackend::apply_tool_options(slsa_package(), &AquaOptions::new(&opts)).unwrap();
    AquaBackend::ensure_locked_slsa_signer(&tv, Some(&locked), &pkg).unwrap();

    // A lock that does not require SLSA is unaffected.
    AquaBackend::ensure_locked_slsa_signer(&tv, None, &slsa_package()).unwrap();
}

#[test]
fn test_slsa_signer_options_require_both_fields() {
    for opts in [
        slsa_signer_opts(Some("identity"), None),
        slsa_signer_opts(None, Some("issuer")),
    ] {
        let opts = AquaOptions::new(&opts);
        let err = AquaBackend::apply_slsa_signer_options(slsa_package(), &opts).unwrap_err();
        assert!(
            err.to_string().contains("must be set together"),
            "unexpected error: {err}"
        );
    }
}

#[test]
fn test_slsa_signer_options_reject_empty_and_non_string_values() {
    for (identity, issuer) in [
        (toml::Value::String(String::new()), "issuer"),
        (toml::Value::Integer(5), "issuer"),
    ] {
        let mut opts = slsa_signer_opts(None, Some(issuer));
        opts.opts
            .insert("slsa_signer_identity".to_string(), identity);
        let opts = AquaOptions::new(&opts);
        let err = AquaBackend::apply_slsa_signer_options(slsa_package(), &opts).unwrap_err();
        assert!(
            err.to_string().contains("expected a non-empty string"),
            "unexpected error: {err}"
        );
    }
}

#[test]
fn test_slsa_signer_options_are_not_package_vars() {
    let opts = slsa_signer_opts(Some("identity"), Some("issuer"));
    let opts = AquaOptions::new(&opts);
    assert!(opts.canonical_var_options().unwrap().is_empty());
    assert!(opts.lockfile_options().unwrap().is_empty());
}

#[test]
fn test_apply_var_options_errors_for_duplicate_nested_vars() {
    let mut pkg = AquaPackage::default();
    pkg.asset = "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string();
    pkg.vars = vec![aqua_var("channel", true)];
    let mut opts = ToolVersionOptions::default();
    opts.opts.insert(
        "channel".to_string(),
        toml::Value::String("stable".to_string()),
    );
    let mut vars = toml::Table::new();
    vars.insert(
        "channel".to_string(),
        toml::Value::String("beta".to_string()),
    );
    opts.opts
        .insert("vars".to_string(), toml::Value::Table(vars));

    let opts = AquaOptions::new(&opts);
    let err = AquaBackend::apply_var_options(pkg, &opts).unwrap_err();

    assert!(
        err.to_string()
            .contains("conflicting aqua var `channel`: use only one spelling"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_apply_var_options_reads_prefixed_vars() {
    let mut pkg = AquaPackage::default();
    pkg.asset = "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string();
    pkg.vars = vec![aqua_var("channel", true)];
    let mut opts = ToolVersionOptions::default();
    opts.opts.insert(
        "vars.channel".to_string(),
        toml::Value::String("stable".to_string()),
    );

    let opts = AquaOptions::new(&opts);
    let pkg = AquaBackend::apply_var_options(pkg, &opts).unwrap();

    assert_eq!(
        pkg.asset("1.0.0", "linux", "amd64").unwrap(),
        "tool-stable-1.0.0.tar.gz"
    );
}

#[test]
fn test_apply_var_options_errors_for_duplicate_prefixed_vars() {
    let mut pkg = AquaPackage::default();
    pkg.asset = "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string();
    pkg.vars = vec![aqua_var("channel", true)];
    let mut opts = ToolVersionOptions::default();
    opts.opts.insert(
        "vars.channel".to_string(),
        toml::Value::String("manifest".to_string()),
    );
    opts.opts.insert(
        "channel".to_string(),
        toml::Value::String("stable".to_string()),
    );

    let opts = AquaOptions::new(&opts);
    let err = AquaBackend::apply_var_options(pkg, &opts).unwrap_err();

    assert!(
        err.to_string()
            .contains("conflicting aqua var `channel`: use only one spelling"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_apply_var_options_allows_same_spelling_overrides() {
    let mut pkg = AquaPackage::default();
    pkg.asset = "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string();
    pkg.vars = vec![aqua_var("channel", true)];
    let mut opts = ToolVersionOptions::default();
    opts.opts.insert(
        "channel".to_string(),
        toml::Value::String("manifest".to_string()),
    );
    let mut overrides = ToolVersionOptions::default();
    overrides.opts.insert(
        "channel".to_string(),
        toml::Value::String("stable".to_string()),
    );
    opts.apply_overrides(&overrides);

    let opts = AquaOptions::new(&opts);
    let pkg = AquaBackend::apply_var_options(pkg, &opts).unwrap();

    assert_eq!(
        pkg.asset("1.0.0", "linux", "amd64").unwrap(),
        "tool-stable-1.0.0.tar.gz"
    );
}

#[test]
fn test_apply_var_options_errors_for_array_vars() {
    let mut pkg = AquaPackage::default();
    pkg.vars = vec![aqua_var("channels", true)];
    let mut opts = ToolVersionOptions::default();
    let mut vars = toml::Table::new();
    vars.insert(
        "channels".to_string(),
        toml::Value::Array(vec![toml::Value::String("stable".to_string())]),
    );
    opts.opts
        .insert("vars".to_string(), toml::Value::Table(vars));

    let opts = AquaOptions::new(&opts);
    let err = AquaBackend::apply_var_options(pkg, &opts).unwrap_err();

    assert!(
        err.to_string()
            .contains("aqua var `vars.channels` must be a string, got array"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_apply_var_options_errors_for_missing_required_var() {
    let mut pkg = AquaPackage::default();
    pkg.vars = vec![aqua_var("go_version", true)];
    let opts = ToolVersionOptions::default();
    let opts = AquaOptions::new(&opts);
    let err = AquaBackend::apply_var_options(pkg, &opts).unwrap_err();

    assert!(
        err.to_string()
            .contains("required aqua var not set: go_version"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_lockfile_options_include_aqua_vars() {
    let mut opts = ToolVersionOptions::default();
    opts.opts.insert(
        "channel".to_string(),
        toml::Value::String("stable".to_string()),
    );
    opts.opts.insert(
        "symlink_bins".to_string(),
        toml::Value::String("true".to_string()),
    );
    opts.opts.insert(
        "postinstall".to_string(),
        toml::Value::String("echo ok".to_string()),
    );
    let mut vars = toml::Table::new();
    vars.insert("go_version".to_string(), toml::Value::String("1.24".into()));
    opts.opts
        .insert("vars".to_string(), toml::Value::Table(vars));

    let lock_opts = AquaOptions::new(&opts).lockfile_options().unwrap();

    assert_eq!(lock_opts.get("vars.channel"), Some(&"stable".to_string()));
    assert_eq!(lock_opts.get("vars.go_version"), Some(&"1.24".to_string()));
    assert!(!lock_opts.contains_key("symlink_bins"));
    assert!(!lock_opts.contains_key("postinstall"));
}

#[test]
fn test_libc_option_is_not_an_aqua_var() {
    let mut opts = ToolVersionOptions::default();
    opts.opts
        .insert("libc".to_string(), toml::Value::String("glibc".into()));

    let aqua_opts = AquaOptions::new(&opts);
    assert_eq!(aqua_opts.libc().unwrap(), Some("gnu"));
    assert_eq!(aqua_opts.var("libc").unwrap(), None);
    let lock_opts = aqua_opts.lockfile_options().unwrap();
    assert_eq!(lock_opts.get("libc"), Some(&"gnu".to_string()));
    assert!(!lock_opts.contains_key("vars.libc"));

    // A registry var that happens to be named `libc` is still reachable.
    opts.opts.shift_remove("libc");
    opts.opts
        .insert("vars.libc".to_string(), toml::Value::String("x".into()));
    let aqua_opts = AquaOptions::new(&opts);
    assert_eq!(aqua_opts.libc().unwrap(), None);
    assert_eq!(aqua_opts.var("libc").unwrap(), Some("x".to_string()));
}

#[test]
fn test_libc_option_rejects_unknown_values() {
    let mut opts = ToolVersionOptions::default();
    opts.opts
        .insert("libc".to_string(), toml::Value::String("uclibc".into()));

    let err = AquaOptions::new(&opts).libc().unwrap_err();
    assert!(
        err.to_string().contains("invalid aqua `libc` option"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_lockfile_options_canonicalize_equivalent_aqua_vars() {
    let mut top_level = ToolVersionOptions::default();
    top_level.opts.insert(
        "channel".to_string(),
        toml::Value::String("stable".to_string()),
    );

    let mut nested = ToolVersionOptions::default();
    let mut vars = toml::Table::new();
    vars.insert(
        "channel".to_string(),
        toml::Value::String("stable".to_string()),
    );
    nested
        .opts
        .insert("vars".to_string(), toml::Value::Table(vars));

    let mut prefixed = ToolVersionOptions::default();
    prefixed.opts.insert(
        "vars.channel".to_string(),
        toml::Value::String("stable".to_string()),
    );

    assert_eq!(
        AquaOptions::new(&top_level).lockfile_options().unwrap(),
        AquaOptions::new(&nested).lockfile_options().unwrap()
    );
    assert_eq!(
        AquaOptions::new(&prefixed).lockfile_options().unwrap(),
        AquaOptions::new(&nested).lockfile_options().unwrap()
    );
}

#[test]
fn test_lockfile_options_errors_for_duplicate_nested_vars() {
    let mut opts = ToolVersionOptions::default();
    let mut vars = toml::Table::new();
    vars.insert(
        "channel".to_string(),
        toml::Value::String("beta".to_string()),
    );
    opts.opts
        .insert("vars".to_string(), toml::Value::Table(vars));
    opts.opts.insert(
        "channel".to_string(),
        toml::Value::String("stable".to_string()),
    );

    let err = AquaOptions::new(&opts).lockfile_options().unwrap_err();

    assert!(
        err.to_string()
            .contains("conflicting aqua var `channel`: use only one spelling"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_lockfile_options_errors_for_duplicate_prefixed_vars() {
    let mut opts = ToolVersionOptions::default();
    opts.opts.insert(
        "channel".to_string(),
        toml::Value::String("stable".to_string()),
    );
    opts.opts.insert(
        "vars.channel".to_string(),
        toml::Value::String("manifest".to_string()),
    );

    let err = AquaOptions::new(&opts).lockfile_options().unwrap_err();

    assert!(
        err.to_string()
            .contains("conflicting aqua var `channel`: use only one spelling"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_aqua_install_time_options_include_flat_vars() {
    assert!(is_install_time_option_key("channel"));
    assert!(is_install_time_option_key("vars.channel"));
    assert!(is_install_time_option_key("vars"));
    assert!(!is_install_time_option_key("symlink_bins"));
}

#[test]
fn test_srcs_support_file_link_with_default_src() {
    let mut pkg = AquaPackage::default();
    pkg.files = vec![AquaFile {
        name: "mc".to_string(),
        link: Some("mc.exe".to_string()),
        ..Default::default()
    }];
    pkg.complete_windows_ext = Some(false);

    let links = AquaBackend::srcs_for_platform(
        &pkg,
        "RELEASE.2025-08-13T08-35-41Z",
        Path::new("install"),
        "windows",
        "amd64",
    )
    .unwrap();

    assert_eq!(
        links,
        vec![AquaFileLink {
            src: PathBuf::from("install").join("mc"),
            dst: PathBuf::from("install").join("mc.exe"),
            hard: false,
            explicit_link: true,
        }]
    );
}

#[test]
fn test_srcs_default_src_to_file_name() {
    // mvdan/sh and (on Windows) astral-sh/uv list files by name only.
    // `symlink_bins` builds `.mise-bins` from these links, so dropping
    // them left the directory missing while PATH pointed at it.
    let mut pkg = AquaPackage::default();
    pkg.files = vec![
        AquaFile {
            name: "uv".to_string(),
            ..Default::default()
        },
        AquaFile {
            name: "uvx".to_string(),
            ..Default::default()
        },
    ];

    let links =
        AquaBackend::srcs_for_platform(&pkg, "1.0.0", Path::new("install"), "windows", "amd64")
            .unwrap();

    let exe = |name: &str| PathBuf::from("install").join(format!("{name}.exe"));
    assert_eq!(
        links,
        vec![
            AquaFileLink {
                src: exe("uv"),
                dst: exe("uv"),
                hard: false,
                explicit_link: false,
            },
            AquaFileLink {
                src: exe("uvx"),
                dst: exe("uvx"),
                hard: false,
                explicit_link: false,
            },
        ]
    );
}

#[test]
fn test_candidate_bin_paths_independent_of_filesystem() {
    // A package whose binary lives in a subdir of the install dir.
    let mut pkg = AquaPackage::default();
    pkg.files = vec![AquaFile {
        name: "uv".to_string(),
        src: Some("uv-bin/uv".to_string()),
        ..Default::default()
    }];

    // An install path that does NOT exist on disk (simulates a mid-install
    // call, before the binaries have been extracted).
    let install_path = Path::new("/definitely/not/here/installs/uv/0.8.21");

    let candidates = AquaBackend::candidate_bin_paths_for_platform(
        &pkg,
        "0.8.21",
        install_path,
        "linux",
        "amd64",
    )
    .unwrap();

    // The candidate dir is derived purely from the package definition; the
    // fact that nothing exists on disk does NOT empty the result. This is
    // the property that makes the cached value safe to persist — the old
    // code's `.filter(|p| p.exists())` here is exactly what poisoned the
    // cache mid-install (#6468).
    assert_eq!(candidates, vec![PathBuf::from("uv-bin")]);
}

#[test]
fn test_srcs_support_custom_windows_ext() {
    let mut pkg = AquaPackage::default();
    pkg.windows_ext = ".bat".to_string();
    pkg.files = vec![AquaFile {
        name: "sass".to_string(),
        src: Some("dart-sass/sass".to_string()),
        ..Default::default()
    }];

    let links =
        AquaBackend::srcs_for_platform(&pkg, "1.0.0", Path::new("install"), "windows", "amd64")
            .unwrap();

    assert_eq!(
        links,
        vec![AquaFileLink {
            src: PathBuf::from("install").join("dart-sass/sass.bat"),
            dst: PathBuf::from("install").join("dart-sass/sass.bat"),
            hard: false,
            explicit_link: false,
        }]
    );
}

#[test]
fn test_srcs_support_hard_file_link() {
    let mut pkg = AquaPackage::default();
    pkg.files = vec![AquaFile {
        name: "pnpm".to_string(),
        src: Some("bin/pnpm".to_string()),
        link: Some("pnpm-hard".to_string()),
        hard: true,
    }];

    let links =
        AquaBackend::srcs_for_platform(&pkg, "1.0.0", Path::new("install"), "linux", "amd64")
            .unwrap();

    assert_eq!(
        links,
        vec![AquaFileLink {
            src: PathBuf::from("install").join("bin/pnpm"),
            dst: PathBuf::from("install").join("bin/pnpm-hard"),
            hard: true,
            explicit_link: true,
        }]
    );
}

#[test]
fn test_srcs_include_prefixed_v_version_paths() {
    let mut pkg = AquaPackage::default();
    pkg.asset = "tool-{{.Version}}-{{.OS}}-{{.Arch}}.tar.gz".to_string();
    pkg.version_prefix = Some("tool-".to_string());
    pkg.files = vec![AquaFile {
        name: "tool".to_string(),
        src: Some("{{.AssetWithoutExt}}/bin/tool".to_string()),
        ..Default::default()
    }];

    let links =
        AquaBackend::srcs_for_platform(&pkg, "1.2.3", Path::new("install"), "linux", "amd64")
            .unwrap();

    assert_eq!(
        links,
        vec![
            AquaFileLink {
                src: PathBuf::from("install").join("tool-tool-1.2.3-linux-amd64/bin/tool"),
                dst: PathBuf::from("install").join("tool-tool-1.2.3-linux-amd64/bin/tool"),
                hard: false,
                explicit_link: false,
            },
            AquaFileLink {
                src: PathBuf::from("install").join("tool-tool-v1.2.3-linux-amd64/bin/tool"),
                dst: PathBuf::from("install").join("tool-tool-v1.2.3-linux-amd64/bin/tool"),
                hard: false,
                explicit_link: false,
            },
        ]
    );
}

#[test]
fn test_srcs_resolved_tag_version_does_not_add_extra_candidates() {
    let mut pkg = AquaPackage::default();
    pkg.asset = "tool-{{.Version}}-{{.OS}}-{{.Arch}}.tar.gz".to_string();
    pkg.version_prefix = Some("tool-".to_string());
    pkg.files = vec![AquaFile {
        name: "tool".to_string(),
        src: Some("{{.AssetWithoutExt}}/bin/tool".to_string()),
        ..Default::default()
    }];

    let links =
        AquaBackend::srcs_for_platform(&pkg, "tool-v1.2.3", Path::new("install"), "linux", "amd64")
            .unwrap();

    assert_eq!(
        links,
        vec![AquaFileLink {
            src: PathBuf::from("install").join("tool-tool-v1.2.3-linux-amd64/bin/tool"),
            dst: PathBuf::from("install").join("tool-tool-v1.2.3-linux-amd64/bin/tool"),
            hard: false,
            explicit_link: false,
        }]
    );
}

#[test]
fn test_relative_path_between_link_and_source() {
    assert_eq!(
        relative_path(
            Path::new("/tmp/install/bin/aliases"),
            Path::new("/tmp/install/bin/tool"),
        )
        .unwrap(),
        PathBuf::from("../tool")
    );
}

#[test]
fn test_relative_path_with_shared_curdir() {
    assert_eq!(
        relative_path(
            Path::new("./install/bin/aliases"),
            Path::new("./install/bin/tool"),
        )
        .unwrap(),
        PathBuf::from("../tool")
    );
}

#[test]
fn test_create_file_link_rejects_hard_link_directory_destination() -> Result<()> {
    let tmp_dir = tempfile::tempdir()?;
    let src = tmp_dir.path().join("tool");
    let dst = tmp_dir.path().join("tool-hard");
    fs::write(&src, "tool")?;
    fs::create_dir(&dst)?;

    let err = AquaBackend::create_file_link(&AquaFileLink {
        src,
        dst,
        hard: true,
        explicit_link: true,
    })
    .unwrap_err()
    .to_string();

    assert!(err.contains("destination is a directory"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_create_file_link_skips_when_dst_aliases_src_inode() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let src = tmp.path().join("Godot");
    fs::write(&src, b"binary contents")?;
    // hard_link gives portable same-inode src/dst without needing a
    // case-insensitive filesystem
    let dst = tmp.path().join("Godot-alias");
    fs::hard_link(&src, &dst)?;

    AquaBackend::create_file_link(&AquaFileLink {
        src: src.clone(),
        dst: dst.clone(),
        hard: false,
        explicit_link: false,
    })?;

    assert!(dst.exists(), "dst must still exist after the early return");
    assert!(!dst.is_symlink(), "dst must not be replaced with a symlink");
    assert_eq!(fs::read(&src)?, b"binary contents");
    Ok(())
}

#[test]
fn test_unescape_regex_literal_no_backslash_is_borrowed() {
    let result = unescape_regex_literal("astral-sh/ruff/.github/workflows/release.yml");
    assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    assert_eq!(result, "astral-sh/ruff/.github/workflows/release.yml");
}

#[test]
fn test_unescape_regex_literal_escaped_dot() {
    assert_eq!(unescape_regex_literal(r"\."), ".");
}

#[test]
fn test_unescape_regex_literal_updatecli_signer_workflow() {
    assert_eq!(
        unescape_regex_literal(r"updatecli/updatecli/\.github/workflows/release\.yaml"),
        "updatecli/updatecli/.github/workflows/release.yaml"
    );
}

#[test]
fn test_unescape_regex_literal_escaped_backslash() {
    assert_eq!(unescape_regex_literal(r"\\"), "\\");
}

#[test]
fn test_unescape_regex_literal_trailing_backslash() {
    assert_eq!(unescape_regex_literal("foo\\"), "foo\\");
}

#[test]
fn test_unescape_regex_literal_empty_string() {
    let result = unescape_regex_literal("");
    assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    assert_eq!(result, "");
}

#[test]
fn test_rebase_github_signer_workflow_after_repo_transfer() {
    assert_eq!(
        rebase_github_signer_workflow(
            "jdx/aube/.github/workflows/release.yml",
            "jdx/aube",
            "aubepkg/aube",
        ),
        "aubepkg/aube/.github/workflows/release.yml"
    );
}

#[test]
fn test_rebase_github_signer_workflow_preserves_reusable_workflow_repo() {
    assert_eq!(
        rebase_github_signer_workflow(
            "aquaproj/aqua-registry/.github/workflows/release.yml",
            "jdx/aube",
            "aubepkg/aube",
        ),
        "aquaproj/aqua-registry/.github/workflows/release.yml"
    );
}

#[test]
fn test_attestation_transfer_retry_result_handling() {
    use crate::github::sigstore::AttestationError;

    assert!(!attestation_needs_transfer_retry(&Ok(true)));
    assert!(attestation_needs_transfer_retry(&Ok(false)));
    assert!(attestation_needs_transfer_retry(&Err(
        AttestationError::NoAttestations
    )));
}

#[test]
fn test_unescape_regex_literal_only_backslash() {
    assert_eq!(unescape_regex_literal("\\"), "\\");
}
