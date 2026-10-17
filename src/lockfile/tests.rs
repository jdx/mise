use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn test_retain_unsuperseded_unbound_entries() {
    let tool = |version: &str, backend: &str, specifiers: &[&str]| LockfileTool {
        version: version.to_string(),
        backend: Some(backend.to_string()),
        specifiers: specifiers.iter().map(|s| s.to_string()).collect(),
        options: BTreeMap::new(),
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    };
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "pipx:cowsay".to_string(),
        vec![
            tool("6.1", "pipx:cowsay", &[]),
            tool("6.1", "pipx:cowsay", &["6.1"]),
        ],
    );
    // No bound sibling with the same version: kept.
    lockfile.tools.insert(
        "a".to_string(),
        vec![tool("1", "a", &[]), tool("2", "a", &["2"])],
    );
    // Bound sibling has a different backend: kept.
    lockfile.tools.insert(
        "b".to_string(),
        vec![tool("1", "b", &[]), tool("1", "other", &["1"])],
    );
    // Same platform key but conflicting checksum: kept; identical: pruned.
    let with_platform = |version: &str, checksum: &str, specifiers: &[&str]| {
        let mut t = tool(version, "c", specifiers);
        t.platforms.insert(
            "linux-x64".to_string(),
            PlatformInfo {
                checksum: Some(checksum.to_string()),
                ..Default::default()
            },
        );
        t
    };
    lockfile.tools.insert(
        "c".to_string(),
        vec![
            with_platform("1", "x", &[]),
            with_platform("1", "y", &["1"]),
        ],
    );
    // Matching platform key but the bound entry lacks the saved data: kept.
    let mut empty_bound = tool("1", "e", &["1"]);
    empty_bound
        .platforms
        .insert("linux-x64".to_string(), PlatformInfo::default());
    let mut unbound = with_platform("1", "x", &[]);
    unbound.backend = Some("e".to_string());
    lockfile
        .tools
        .insert("e".to_string(), vec![unbound, empty_bound]);
    // Identical persisted data but a differing read-only size: pruned.
    let mut sized = with_platform("1", "x", &["1"]);
    sized.backend = Some("f".to_string());
    sized.platforms.get_mut("linux-x64").unwrap().size = Some(7);
    let mut unsized_ = with_platform("1", "x", &[]);
    unsized_.backend = Some("f".to_string());
    lockfile
        .tools
        .insert("f".to_string(), vec![unsized_, sized]);
    lockfile.tools.insert(
        "d".to_string(),
        vec![
            with_platform("1", "x", &[]),
            with_platform("1", "x", &["1"]),
        ],
    );
    lockfile.retain_unsuperseded_unbound_entries(|_, _| true);
    assert_eq!(lockfile.tools["pipx:cowsay"].len(), 1);
    assert!(!lockfile.tools["pipx:cowsay"][0].specifiers.is_empty());
    assert_eq!(lockfile.tools["a"].len(), 2);
    assert_eq!(lockfile.tools["b"].len(), 2);
    assert_eq!(lockfile.tools["c"].len(), 2);
    assert_eq!(lockfile.tools["d"].len(), 1);
    assert_eq!(lockfile.tools["e"].len(), 2);
    assert_eq!(lockfile.tools["f"].len(), 1);
}

#[test]
fn test_outdated_lockfile_version_waits_six_months_after_newer_format() {
    use jiff::civil::date;

    assert_eq!(
        LOCKFILE_VERSION_DATES.last().unwrap().0,
        CURRENT_LOCKFILE_VERSION
    );
    assert!(!outdated_lockfile_version(0, date(2027, 2, 23)));
    assert!(outdated_lockfile_version(0, date(2027, 2, 24)));
    assert!(!outdated_lockfile_version(1, date(2027, 3, 13)));
    assert!(outdated_lockfile_version(1, date(2027, 3, 14)));
    assert!(!outdated_lockfile_version(2, date(2027, 3, 27)));
    assert!(outdated_lockfile_version(2, date(2027, 3, 28)));
    assert!(!outdated_lockfile_version(3, date(2028, 1, 1)));
}

#[test]
fn test_url_contradicts_version_reports_a_different_release() {
    assert_eq!(
        url_contradicts_version(
            "2.0.0",
            "https://github.com/o/r/releases/download/cli%2Fv1.2.3/tool-linux",
        ),
        Some(vec!["1.2.3".to_string()])
    );
    // A `version` rewritten without refreshing the platform block.
    assert_eq!(
        url_contradicts_version(
            "2.0.1",
            "https://github.com/jdx/hk/releases/download/v1.56.1/hk-x86_64-unknown-linux-gnu.tar.gz"
        ),
        Some(vec!["1.56.1".to_string()])
    );
    // A platform block left behind by a release that dropped the target.
    assert_eq!(
        url_contradicts_version(
            "2.2.17",
            "https://github.com/aubepkg/aube/releases/download/v2.2.12/libaube-x86_64-apple-darwin.dylib"
        ),
        Some(vec!["2.2.12".to_string()])
    );
}

#[test]
fn test_url_contradicts_version_accepts_matching_releases() {
    for (version, url) in [
        // A percent-encoded `@` in the tag is decoded before comparing.
        (
            "0.46.0",
            "https://github.com/getsentry/toolkit/releases/download/cli%400.46.0/sentry-darwin-arm64",
        ),
        // An encoded slash stays inside the tag.
        (
            "1.2.3",
            "https://github.com/o/r/releases/download/cli%2Fv1.2.3/tool-linux",
        ),
        // The plain case: the tag names the version.
        (
            "1.56.1",
            "https://github.com/jdx/hk/releases/download/v1.56.1/hk-x86_64-unknown-linux-gnu.tar.gz",
        ),
        // Tag prefixes and repeated versions in the file name.
        (
            "2026.8.0",
            "https://github.com/bitwarden/clients/releases/download/cli-v2026.8.0/bw-linux-2026.8.0.zip",
        ),
        // A build tag the version does not carry, and a date-only release tag.
        (
            "3.14.7",
            "https://github.com/astral-sh/python-build-standalone/releases/download/20260901/cpython-3.14.7+20260901-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz",
        ),
        // A vendor-prefixed version whose numeric part is what the URL names.
        (
            "temurin-21.0.4+7",
            "https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.4%2B7/OpenJDK21U-jdk_x64_linux_hotspot_21.0.4_7.tar.gz",
        ),
        // The version runs straight into the tool name.
        ("1.23.4", "https://go.dev/dl/go1.23.4.linux-amd64.tar.gz"),
        // One side is a truncation of the other.
        (
            "22.1.0",
            "https://nodejs.org/dist/v22.1/node-v22.1-linux-x64.tar.xz",
        ),
        // A zero-padded tag names the same release as the stored version.
        (
            "1.2.0",
            "https://github.com/o/r/releases/download/v1.02.0/tool-linux-x64.tar.gz",
        ),
        // ...and the padding may be on the version instead.
        (
            "1.02.0",
            "https://github.com/o/r/releases/download/v1.2.0/tool-linux-x64.tar.gz",
        ),
    ] {
        assert_eq!(
            url_contradicts_version(version, url),
            None,
            "{version} should agree with {url}"
        );
    }
}

#[test]
fn test_url_contradicts_version_stays_quiet_without_proof() {
    // Nothing version-like in the path: many assets are named by platform only.
    assert_eq!(
        url_contradicts_version("1.2.3", "https://example.com/dist/tool-linux-x64.tar.gz"),
        None
    );
    // `x86_64` and a bare date are not dotted numbers.
    assert_eq!(
        url_contradicts_version(
            "1.2.3",
            "https://example.com/dist/20260901/tool-x86_64-sha256.tar.gz"
        ),
        None
    );
    // The entry version is not a dotted number, so there is nothing to compare.
    for version in ["latest", "ref:0d1f2e3", "20260901"] {
        assert_eq!(
            url_contradicts_version(
                version,
                "https://github.com/o/r/releases/download/v1.2.3/t.tar.gz"
            ),
            None,
            "{version} should not be compared"
        );
    }
    // The host is not part of the comparison.
    assert_eq!(
        url_contradicts_version("1.2.3", "http://10.0.0.5/dist/tool.tar.gz"),
        None
    );
}

/// A number in the route to an artifact is not a release identifier. These
/// are all valid locked installs, and rejecting one would be worse than
/// missing a real mismatch, so none of them may fire.
#[test]
fn test_url_contradicts_version_ignores_numbers_outside_release_positions() {
    for (version, url, why) in [
        (
            "2.0.0",
            "https://mirror.example/api/1.0/tool-linux.tar.gz",
            "an API version in the path",
        ),
        (
            "1.2.3",
            "https://cdn.example/v2/artifacts/tool.tar.gz",
            "an unversioned API segment",
        ),
        (
            "1.2.3",
            "https://cdn.example/bucket-3.7/dist/tool-linux-x64.tar.gz",
            "a numbered CDN bucket",
        ),
        (
            "1.2.3",
            "https://example.com/dist/20260901/tool-linux-x64.tar.gz",
            "a date directory",
        ),
        (
            "2.0.0",
            "https://mirror.example/repo/1.0/pool/main/t/tool/tool_all.deb",
            "a distro pool layout",
        ),
        (
            "1.2.3",
            "https://s3.example.com/2.0/downloads/tool-x86_64",
            "a versioned bucket that is not a release tag",
        ),
    ] {
        assert_eq!(
            url_contradicts_version(version, url),
            None,
            "{version} must install despite {why}: {url}"
        );
    }
}

#[test]
fn test_release_identifiers_reads_only_the_tag_and_file_name() {
    // `/releases/download/<tag>/<file>`
    assert_eq!(
        release_identifiers("/jdx/hk/releases/download/v1.56.1/hk-x86_64-linux.tar.gz"),
        vec!["1.56.1"]
    );
    // GitLab's `/-/releases/<tag>/downloads/<file>`
    assert_eq!(
        release_identifiers("/g/p/-/releases/v1.2.3/downloads/tool-linux"),
        vec!["1.2.3"]
    );
    // `/-/archive/<tag>/<file>`
    assert_eq!(
        release_identifiers("/g/p/-/archive/v1.2.3/p-v1.2.3.tar.gz"),
        vec!["1.2.3", "1.2.3"]
    );
    // Neither position carries a number, so the `1.0` route is ignored.
    assert_eq!(
        release_identifiers("/api/1.0/tool-linux.tar.gz"),
        Vec::<&str>::new()
    );
}

#[test]
fn test_dotted_numbers_agree_compares_components_numerically() {
    assert!(dotted_numbers_agree("1.02.0", "1.2.0"));
    assert!(dotted_numbers_agree("1.2.0", "1.02.0"));
    assert!(dotted_numbers_agree("22.1", "22.1.0"));
    assert!(dotted_numbers_agree("0.1.0", "00.1.0"));
    // Zero-stripping must not merge genuinely different components.
    assert!(!dotted_numbers_agree("1.20.0", "1.2.0"));
    assert!(!dotted_numbers_agree("1.2.3", "1.9.9"));
    // A longer trailing component is a different release, not a truncation:
    // 1.2.3 must never accept a 1.2.30 artifact.
    assert!(!dotted_numbers_agree("1.2.3", "1.2.30"));
    assert!(!dotted_numbers_agree("1.2.30", "1.2.3"));
}

#[test]
fn test_dotted_number_runs() {
    assert_eq!(dotted_number_runs("v2.39.0.zip"), vec!["2.39.0"]);
    assert_eq!(
        dotted_number_runs("hk-x86_64-unknown-linux-gnu"),
        Vec::<&str>::new()
    );
    assert_eq!(
        dotted_number_runs("cpython-3.14.7+20260901"),
        vec!["3.14.7"]
    );
    assert_eq!(dotted_number_runs("20260901"), Vec::<&str>::new());
}

fn basic_tool(version: &str, backend: &str) -> LockfileTool {
    LockfileTool {
        version: version.to_string(),
        backend: Some(backend.to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::new(),
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    }
}

#[test]
fn tool_stub_finds_nearest_base_config_lockfile_from_where_it_lives() {
    let temp = tempfile::tempdir().unwrap();
    let project = fs::canonicalize(temp.path()).unwrap().join("project");
    fs::create_dir_all(project.join("bin")).unwrap();
    fs::create_dir_all(project.join("elsewhere")).unwrap();
    // The test harness overrides the config filenames.
    file::write(project.join(".test.mise.toml"), "").unwrap();
    let stub = project.join("bin/tool");
    file::write(&stub, "version = \"1\"\n").unwrap();

    let expected = (project.join("mise.lock"), false);
    assert_eq!(lockfile_path_for_tool_stub(&stub), Some(expected.clone()));

    #[cfg(unix)]
    {
        let link = project.join("elsewhere/tool");
        std::os::unix::fs::symlink(&stub, &link).unwrap();
        assert_eq!(lockfile_path_for_tool_stub(&link), Some(expected));
    }
}

#[test]
fn tool_stub_references_round_trip_and_drop_deleted_stubs() {
    let temp = tempfile::tempdir().unwrap();
    let project = fs::canonicalize(temp.path()).unwrap();
    fs::create_dir_all(project.join("bin")).unwrap();
    file::write(project.join(".test.mise.toml"), "").unwrap();
    let stub = project.join("bin/tool");
    file::write(&stub, "version = \"1\"\n").unwrap();
    let path = project.join("mise.lock");

    let mut lockfile = Lockfile::default();
    lockfile.add_tool_stub(&path, &stub).unwrap();
    lockfile.save(&path).unwrap();
    let contents = file::read_to_string(&path).unwrap();
    assert!(
        contents.contains("tool-stubs = [\"bin/tool\"]"),
        "{contents}"
    );

    let mut lockfile = Lockfile::read(&path).unwrap();
    assert_eq!(
        lockfile.tool_stubs(),
        &BTreeSet::from(["bin/tool".to_string()])
    );
    assert_eq!(tool_stub_path(&path, "bin/tool"), stub);
    assert!(!lockfile.retain_live_tool_stubs(&path));

    fs::remove_file(&stub).unwrap();
    assert!(lockfile.retain_live_tool_stubs(&path));
    assert!(lockfile.tool_stubs().is_empty());
}

#[test]
fn new_lockfiles_write_current_version_and_request_bindings() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "dummy".to_string(),
        vec![LockfileTool {
            version: "1.1.0".to_string(),
            backend: Some("asdf:dummy".to_string()),
            specifiers: BTreeSet::from(["1".to_string()]),
            options: BTreeMap::new(),
            platforms: BTreeMap::new(),
            aube: None,
            uv: None,
        }],
    );

    lockfile.save(&path).unwrap();
    let contents = file::read_to_string(&path).unwrap();
    assert!(contents.contains("lockfile_version = 3"));
    assert!(contents.contains("specifiers = [\"1\"]"));

    let reloaded = Lockfile::read(&path).unwrap();
    assert_eq!(reloaded.lockfile_version(), 3);
    assert_eq!(
        reloaded.tools["dummy"][0].specifiers,
        BTreeSet::from(["1".to_string()])
    );
}

#[test]
fn forge_ids_require_explicit_upgrade_and_write_inline() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    file::write(&path, "lockfile_version = 2\n[tools]\n").unwrap();
    let mut lockfile = Lockfile::read(&path).unwrap();
    lockfile.save(&path).unwrap();
    assert!(
        file::read_to_string(&path)
            .unwrap()
            .contains("lockfile_version = 2")
    );
    let mut tool = basic_tool("1.0.0", "packslip:example/tool");
    tool.platforms.insert(
        "linux-x64".into(),
        PlatformInfo {
            repository_id: Some("922514152".into()),
            repository_owner_id: Some("216188".into()),
            signer: Some("sigstore-oidc:example".into()),
            ..Default::default()
        },
    );
    tool.platforms.insert(
        "macos-arm64".into(),
        PlatformInfo {
            repository_id: Some("922514152".into()),
            signer: Some("sigstore-oidc:example".into()),
            ..Default::default()
        },
    );
    lockfile.tools.insert("example".into(), vec![tool]);
    lockfile.save(&path).unwrap();

    let contents = file::read_to_string(&path).unwrap();
    assert!(contents.contains("lockfile_version = 2"), "{contents}");
    assert!(!contents.contains("repository_ids"), "{contents}");

    lockfile.upgrade();
    lockfile.save(&path).unwrap();
    let contents = file::read_to_string(&path).unwrap();
    assert!(contents.contains("lockfile_version = 3"), "{contents}");
    assert!(
        contents.contains("repository_ids = { repository = \"922514152\", owner = \"216188\" }"),
        "{contents}"
    );
    assert!(
        contents.contains("repository_ids = { repository = \"922514152\" }"),
        "{contents}"
    );
    let reloaded = Lockfile::read(&path).unwrap();
    let ids = &reloaded.tools["example"][0].platforms["linux-x64"];
    assert_eq!(ids.repository_id.as_deref(), Some("922514152"));
    assert_eq!(ids.repository_owner_id.as_deref(), Some("216188"));
}

#[test]
fn empty_lockfile_uses_current_revision() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    file::write(&path, "").unwrap();
    let lockfile = Lockfile::read(&path).unwrap();
    assert_eq!(lockfile.lockfile_version(), CURRENT_LOCKFILE_VERSION);
}

#[test]
fn forge_ids_require_version_three_when_reading() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    file::write(
            &path,
            "lockfile_version = 2\n[[tools.example]]\nversion = \"1.0.0\"\nbackend = \"packslip:example/tool\"\n[tools.example.\"platforms.linux-x64\"]\nrepository_ids = { repository = \"922514152\" }\n",
        )
        .unwrap();

    let error = Lockfile::read(&path).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("forge repository IDs require lockfile revision 3")
    );
}

#[test]
fn aube_graph_round_trips_through_mise_lock() {
    let yaml = r#"lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
importers:
  .:
    dependencies:
      cli:
        specifier: 1.0.0
        version: 1.0.0(peer@2.0.0)
packages:
  cli@1.0.0(peer@2.0.0):
    resolution: {integrity: sha512-root}
    dependencies:
      peer: 2.0.0
  peer@2.0.0:
    resolution: {integrity: sha512-peer}
    os: [linux, darwin]
"#;
    let graph = AubeLock::from_yaml(yaml).unwrap();
    let reordered = AubeLock::from_yaml(
        r#"packages:
  peer@2.0.0:
    os: [linux, darwin]
    resolution: {integrity: sha512-peer}
  cli@1.0.0(peer@2.0.0):
    dependencies:
      peer: 2.0.0
    resolution: {integrity: sha512-root}
importers:
  .:
    dependencies:
      cli:
        version: 1.0.0(peer@2.0.0)
        specifier: 1.0.0
settings:
  autoInstallPeers: true
lockfileVersion: '9.0'
"#,
    )
    .unwrap();
    let reparsed = AubeLock::from_yaml(&graph.to_yaml().unwrap()).unwrap();
    assert_eq!(graph, reparsed);
    assert_eq!(
        GraphRef::from(graph.clone()).identity(),
        GraphRef::from(reparsed).identity()
    );
    assert_ne!(
        GraphRef::from(graph.clone()).identity(),
        GraphRef::from(reordered).identity()
    );

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    let mut lockfile = Lockfile::default();
    let mut tool = basic_tool("1.0.0", "npm:cli");
    tool.aube = Some(graph.clone().into());
    lockfile.tools.insert("npm:cli".into(), vec![tool]);
    lockfile.save(&path).unwrap();

    let reloaded = Lockfile::read(path).unwrap();
    assert_eq!(
        reloaded.tools["npm:cli"][0]
            .aube
            .as_ref()
            .unwrap()
            .load()
            .unwrap(),
        &graph
    );
    assert_eq!(
        reloaded.tools["npm:cli"][0]
            .aube
            .as_ref()
            .unwrap()
            .identity(),
        GraphRef::from(graph).identity()
    );
}

#[test]
fn legacy_lockfiles_remain_unversioned_when_rewritten() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    file::write(
        &path,
        "[[tools.dummy]]\nversion = \"1.0.0\"\nbackend = \"asdf:dummy\"\n",
    )
    .unwrap();

    let lockfile = Lockfile::read(&path).unwrap();
    assert_eq!(lockfile.lockfile_version(), 0);
    lockfile.save(&path).unwrap();
    let contents = file::read_to_string(&path).unwrap();
    assert!(!contents.contains("lockfile_version"));
    assert!(!contents.contains("specifiers"));
}

#[test]
fn future_lockfile_versions_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    file::write(&path, "lockfile_version = 4\n[tools]\n").unwrap();

    let err = Lockfile::read(&path).unwrap_err();
    assert!(
        err.to_string()
            .contains("unsupported lockfile version 4; this mise supports up to version 3")
    );
}

#[test]
fn request_binding_moves_between_versions_with_matching_options() {
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "dummy".to_string(),
        vec![
            LockfileTool {
                version: "1.0.0".to_string(),
                backend: Some("asdf:dummy".to_string()),
                specifiers: BTreeSet::from(["1".to_string()]),
                options: BTreeMap::new(),
                platforms: BTreeMap::new(),
                aube: None,
                uv: None,
            },
            basic_tool("1.1.0", "asdf:dummy"),
        ],
    );

    assert!(lockfile.bind_request("dummy", "1", "1.1.0", &BTreeMap::new()));

    assert!(lockfile.tools["dummy"][0].specifiers.is_empty());
    assert_eq!(
        lockfile.tools["dummy"][1].specifiers,
        BTreeSet::from(["1".to_string()])
    );
}

#[test]
fn failed_request_binding_preserves_the_existing_binding() {
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "dummy".to_string(),
        vec![LockfileTool {
            version: "1.0.0".to_string(),
            backend: Some("asdf:dummy".to_string()),
            specifiers: BTreeSet::from(["1".to_string()]),
            options: BTreeMap::new(),
            platforms: BTreeMap::new(),
            aube: None,
            uv: None,
        }],
    );

    assert!(!lockfile.bind_request("dummy", "1", "2.0.0", &BTreeMap::new()));
    assert_eq!(
        lockfile.tools["dummy"][0].specifiers,
        BTreeSet::from(["1".to_string()])
    );
}

#[test]
fn test_legacy_lockfile_path_discovery_is_cached() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cache_key = (
        temp_dir.path().join("mise.toml"),
        Some(true),
        vec!["packages/*".to_string()],
    );
    let discoveries = AtomicUsize::new(0);
    let expected = IndexSet::from([temp_dir.path().join("packages/a/mise.lock")]);

    for _ in 0..3 {
        let paths = cached_legacy_lockfile_paths(cache_key.clone(), || {
            discoveries.fetch_add(1, Ordering::Relaxed);
            expected.clone()
        });
        assert_eq!(paths, expected);
    }

    assert_eq!(discoveries.load(Ordering::Relaxed), 1);

    invalidate_caches();
    let paths = cached_legacy_lockfile_paths(cache_key, || {
        discoveries.fetch_add(1, Ordering::Relaxed);
        expected.clone()
    });
    assert_eq!(paths, expected);
    assert_eq!(discoveries.load(Ordering::Relaxed), 2);
}

#[test]
fn test_legacy_lockfile_path_discovery_uses_effective_monorepo_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("mise.toml");
    let discoveries = AtomicUsize::new(0);

    for roots in [["packages/*"], ["crates/*"]] {
        cached_legacy_lockfile_paths(
            (
                config_path.clone(),
                Some(true),
                roots.into_iter().map(str::to_string).collect(),
            ),
            || {
                discoveries.fetch_add(1, Ordering::Relaxed);
                IndexSet::new()
            },
        );
    }

    assert_eq!(discoveries.load(Ordering::Relaxed), 2);
}

fn basic_tv(backend: &str, version: &str) -> ToolVersion {
    let tool_name = backend
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or_else(|| {
            backend
                .rsplit_once(':')
                .map(|(_, name)| name)
                .unwrap_or(backend)
        })
        .to_string();
    let short = tool_name.clone();
    let backend = Arc::new(crate::args::BackendArg::new_raw(
        short,
        Some(backend.to_string()),
        tool_name,
        None,
        crate::args::BackendResolution::new(true),
    ));
    let request = crate::toolset::ToolRequest::new(backend, version, ToolSource::Unknown).unwrap();
    ToolVersion::new(request, version.to_string())
}

fn basic_tv_from_source(
    backend: &str,
    request: &str,
    version: &str,
    source: ToolSource,
) -> ToolVersion {
    let prototype = basic_tv(backend, request);
    let request =
        crate::toolset::ToolRequest::new(prototype.request.ba().clone(), request, source).unwrap();
    ToolVersion::new(request, version.to_string())
}

#[test]
fn test_tools_by_source_for_update_does_not_write_to_borrowed_owner() {
    let owner = ToolSource::MiseToml(PathBuf::from("/repo/mise.toml"));
    let mut runtime = basic_tv_from_source("aqua:example/tool", "2", "2.0.0", ToolSource::Argument);
    runtime
        .request
        .set_lockfile_scope(crate::toolset::tool_request::LockfileScope::Source(
            owner.clone(),
        ));
    let tools_by_source = tools_by_source_for_update(&Toolset::default(), &[runtime]);
    assert!(!tools_by_source.contains_key(&owner));
    assert_eq!(
        tools_by_source[&ToolSource::Argument]["tool"][0].version,
        "2.0.0"
    );
}

#[test]
fn test_tools_by_source_for_update_attributes_each_new_version_to_its_source() {
    let source_a = ToolSource::MiseToml(PathBuf::from("/repo/packages/a/mise.toml"));
    let source_b = ToolSource::MiseToml(PathBuf::from("/repo/packages/b/mise.toml"));
    let new_versions = vec![
        basic_tv_from_source("aqua:example/tool", "stable", "release-a", source_a.clone()),
        basic_tv_from_source("aqua:example/tool", "stable", "release-b", source_b.clone()),
    ];

    let tools_by_source = tools_by_source_for_update(&Toolset::default(), &new_versions);

    assert_eq!(tools_by_source[&source_a]["tool"].len(), 1);
    assert_eq!(tools_by_source[&source_a]["tool"][0].version, "release-a");
    assert_eq!(tools_by_source[&source_b]["tool"].len(), 1);
    assert_eq!(tools_by_source[&source_b]["tool"][0].version, "release-b");
}

#[test]
fn test_tools_by_source_for_update_last_duplicate_request_version_wins() {
    let source = ToolSource::MiseToml(PathBuf::from("/repo/mise.toml"));
    let new_versions = vec![
        basic_tv_from_source("aqua:example/tool", "stable", "release-old", source.clone()),
        basic_tv_from_source("aqua:example/tool", "stable", "release-new", source.clone()),
    ];

    let tools_by_source = tools_by_source_for_update(&Toolset::default(), &new_versions);

    assert_eq!(tools_by_source[&source]["tool"].len(), 1);
    assert_eq!(tools_by_source[&source]["tool"][0].version, "release-new");
}

fn tool_with_conda_dep(version: &str, backend: &str, platform: &str, dep: &str) -> LockfileTool {
    let mut platforms = BTreeMap::new();
    platforms.insert(
        platform.to_string(),
        PlatformInfo {
            checksum: None,
            size: None,
            url: None,
            url_api: None,
            conda_deps: Some(vec![dep.to_string()]),
            ..Default::default()
        },
    );
    LockfileTool {
        version: version.to_string(),
        backend: Some(backend.to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::new(),
        platforms,
        aube: None,
        uv: None,
    }
}

fn add_test_conda_package(lockfile: &mut Lockfile, platform: &str, basename: &str) {
    lockfile.set_conda_package(
        platform,
        basename,
        CondaPackageInfo {
            url: format!("https://example.com/{basename}.conda"),
            checksum: Some(format!("sha256:{basename}")),
        },
    );
}

#[test]
fn empty_resolution_preserves_existing_conda_dependencies() {
    let platform_key = "linux-x64";
    let dependency = "libfoo-1.0-h123_0";
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "ffmpeg".to_string(),
        vec![tool_with_conda_dep(
            "7.1.1",
            "conda:ffmpeg",
            platform_key,
            dependency,
        )],
    );
    add_test_conda_package(&mut lockfile, platform_key, dependency);

    let result = (
        "ffmpeg".to_string(),
        "7.1.1".to_string(),
        "conda:ffmpeg".to_string(),
        Platform::parse(platform_key).unwrap(),
        Ok(PlatformInfo::default()),
        BTreeMap::new(),
        BTreeMap::new(),
        crate::lockfile::LockResolutionStatus::Optional,
    );

    assert!(!apply_lock_result(&mut lockfile, result).unwrap());
    lockfile.cleanup_unreferenced_conda_packages();

    let platform = &lockfile.tools["ffmpeg"][0].platforms[platform_key];
    assert_eq!(
        platform.conda_deps.as_deref(),
        Some(&[dependency.to_string()][..])
    );
    assert!(
        lockfile
            .get_conda_package(platform_key, dependency)
            .is_some()
    );
}

#[test]
fn empty_resolution_creates_new_tool_entry_without_platform_metadata() {
    let mut lockfile = Lockfile::default();
    let result = (
        "dummy".to_string(),
        "1.0.0".to_string(),
        "asdf:dummy".to_string(),
        Platform::parse("linux-x64").unwrap(),
        Ok(PlatformInfo::default()),
        BTreeMap::new(),
        BTreeMap::new(),
        crate::lockfile::LockResolutionStatus::Optional,
    );

    assert!(!apply_lock_result(&mut lockfile, result).unwrap());
    let tool = &lockfile.tools["dummy"][0];
    assert_eq!(tool.version, "1.0.0");
    assert!(tool.platforms.is_empty());
}

#[test]
fn test_array_format_required() {
    // Test that multi-version (array) format is read correctly
    let multi_version_toml = r#"
[[tools.node]]
version = "20.10.0"
backend = "core:node"

[[tools.python]]
version = "3.11.0"
backend = "core:python"
"#;

    let table: toml::Table = toml::from_str(multi_version_toml).unwrap();
    let tools: toml::Table = table.get("tools").unwrap().clone().try_into().unwrap();

    let mut lockfile = Lockfile::default();
    for (short, value) in tools {
        let versions = match value {
            toml::Value::Array(arr) => arr
                .into_iter()
                .map(LockfileTool::try_from)
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            _ => panic!("expected array format"),
        };
        lockfile.tools.insert(short, versions);
    }

    // Verify that we have the expected tools
    assert_eq!(lockfile.tools.len(), 2);
    assert!(lockfile.tools.contains_key("node"));
    assert!(lockfile.tools.contains_key("python"));

    // Verify node
    let node_versions = &lockfile.tools["node"];
    assert_eq!(node_versions.len(), 1);
    assert_eq!(node_versions[0].version, "20.10.0");
    assert_eq!(node_versions[0].backend, Some("core:node".to_string()));

    // Verify python
    let python_versions = &lockfile.tools["python"];
    assert_eq!(python_versions.len(), 1);
    assert_eq!(python_versions[0].version, "3.11.0");
}

#[test]
fn test_save_uses_array_format() {
    let mut lockfile = Lockfile::default();
    let mut platforms = BTreeMap::new();
    platforms.insert(
        "macos-arm64".to_string(),
        PlatformInfo {
            checksum: Some("sha256:abc123".to_string()),
            size: Some(12345678),
            url: Some("https://example.com/node.tar.gz".to_string()),
            url_api: Some("https://api.github.com.com/repos/test/1234".to_string()),
            conda_deps: None,
            ..Default::default()
        },
    );

    let tool = LockfileTool {
        version: "20.10.0".to_string(),
        backend: Some("core:node".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::new(),
        platforms,
        aube: None,
        uv: None,
    };

    lockfile.tools.insert("node".to_string(), vec![tool]);

    // Create a temporary file to test saving
    let temp_dir = std::env::temp_dir();
    let test_lockfile = temp_dir.join("test_lockfile.lock");

    // Save and verify it uses multi-version format
    lockfile.save(&test_lockfile).unwrap();

    let content = std::fs::read_to_string(&test_lockfile).unwrap();

    // Should use [[tools.node]] array syntax, not [tools.node] single version
    assert!(content.contains("[[tools.node]]"));
    // Verify it doesn't use single-version format (but allow platforms sections)
    assert!(!content.lines().any(|line| line.trim() == "[tools.node]"));

    // Clean up
    let _ = std::fs::remove_file(&test_lockfile);
}

#[test]
fn test_concurrent_save_no_enoent() {
    // Regression: two concurrent save() calls used to share a fixed
    // `mise.lock.tmp` path, so the loser of the rename race got
    // "No such file or directory". Each save must now use a unique
    // temp file so concurrent writers never trip over each other.
    let temp_dir = tempfile::tempdir().unwrap();
    let path = Arc::new(temp_dir.path().join("concurrent.lock"));
    // Pre-create the file so this mirrors the real update path, where
    // save() only runs on lockfiles that already exist.
    std::fs::write(&*path, "").unwrap();

    let threads: Vec<_> = (0..8)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                for _ in 0..20 {
                    Lockfile::default().save(&*path).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }

    // Any leftover temp files would indicate persist() never ran.
    let leftovers: Vec<_> = std::fs::read_dir(temp_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() != std::ffi::OsStr::new("concurrent.lock"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "unexpected temp files left behind: {leftovers:?}"
    );
}

#[cfg(unix)]
#[test]
fn test_save_preserves_existing_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("mise.lock");
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();

    Lockfile::default().save(&path).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o664);
}

#[test]
fn test_save_preserves_existing_header_url() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("mise.lock");
    std::fs::write(
            &path,
            "# @generated - this file is auto-generated by `mise lock` https://mise.jdx.dev/dev-tools/mise-lock.html\n\n[tools]\n",
        )
        .unwrap();

    let lockfile = Lockfile::read(&path).unwrap();
    lockfile.save(&path).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.starts_with(
            "# @generated - this file is auto-generated by `mise lock` https://mise.jdx.dev/dev-tools/mise-lock.html\n"
        ));
}

#[test]
fn test_options_field_parsing_and_serialization() {
    // Test parsing lockfile with options
    let toml_with_options = r#"
[[tools.ripgrep]]
version = "14.0.0"
backend = "ubi:BurntSushi/ripgrep"
options = { exe = "rg", matching = "musl" }

[tools.ripgrep.platforms.linux-x64]
checksum = "blake3:abc123"
"#;

    let table: toml::Table = toml::from_str(toml_with_options).unwrap();
    let tools: toml::Table = table.get("tools").unwrap().clone().try_into().unwrap();

    let mut lockfile = Lockfile::default();
    for (short, value) in tools {
        let versions = match value {
            toml::Value::Array(arr) => arr
                .into_iter()
                .map(LockfileTool::try_from)
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            _ => vec![LockfileTool::try_from(value).unwrap()],
        };
        lockfile.tools.insert(short, versions);
    }

    // Verify options were parsed correctly
    let ripgrep = &lockfile.tools["ripgrep"][0];
    assert_eq!(ripgrep.options.get("exe"), Some(&"rg".to_string()));
    assert_eq!(ripgrep.options.get("matching"), Some(&"musl".to_string()));
}

#[test]
fn test_options_field_not_serialized_when_empty() {
    let mut lockfile = Lockfile::default();
    let tool = LockfileTool {
        version: "14.0.0".to_string(),
        backend: Some("ubi:BurntSushi/ripgrep".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::new(), // Empty options
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    };
    lockfile.tools.insert("ripgrep".to_string(), vec![tool]);

    let temp_dir = std::env::temp_dir();
    let test_lockfile = temp_dir.join("test_lockfile_no_options.lock");

    lockfile.save(&test_lockfile).unwrap();
    let content = std::fs::read_to_string(&test_lockfile).unwrap();

    // Should NOT contain "options" when it's empty
    assert!(!content.contains("options"));

    let _ = std::fs::remove_file(&test_lockfile);
}

#[test]
fn test_options_field_serialized_when_present() {
    let mut lockfile = Lockfile::default();
    let mut options = BTreeMap::new();
    options.insert("exe".to_string(), "rg".to_string());
    options.insert("matching".to_string(), "musl".to_string());

    let tool = LockfileTool {
        version: "14.0.0".to_string(),
        backend: Some("ubi:BurntSushi/ripgrep".to_string()),
        specifiers: BTreeSet::new(),
        options,
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    };
    lockfile.tools.insert("ripgrep".to_string(), vec![tool]);

    let temp_dir = std::env::temp_dir();
    let test_lockfile = temp_dir.join("test_lockfile_with_options.lock");

    lockfile.save(&test_lockfile).unwrap();
    let content = std::fs::read_to_string(&test_lockfile).unwrap();

    // Should contain options
    assert!(content.contains("options"));
    assert!(content.contains("exe"));
    assert!(content.contains("rg"));

    let _ = std::fs::remove_file(&test_lockfile);
}

#[test]
fn test_options_matching_in_get_locked_version() {
    // This tests that get_locked_version requires exact options match
    let toml_with_options = r#"
[[tools.ripgrep]]
version = "14.0.0"
backend = "ubi:BurntSushi/ripgrep"
options = { exe = "rg", matching = "musl" }

[[tools.ripgrep]]
version = "14.0.0"
backend = "ubi:BurntSushi/ripgrep"
options = { exe = "rg" }
"#;

    let table: toml::Table = toml::from_str(toml_with_options).unwrap();
    let tools: toml::Table = table.get("tools").unwrap().clone().try_into().unwrap();

    let mut lockfile = Lockfile::default();
    for (short, value) in tools {
        let versions = match value {
            toml::Value::Array(arr) => arr
                .into_iter()
                .map(LockfileTool::try_from)
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            _ => vec![LockfileTool::try_from(value).unwrap()],
        };
        lockfile.tools.insert(short, versions);
    }

    // Verify we have 2 entries for ripgrep with different options
    assert_eq!(lockfile.tools["ripgrep"].len(), 2);
    assert_eq!(lockfile.tools["ripgrep"][0].options.len(), 2);
    assert_eq!(lockfile.tools["ripgrep"][1].options.len(), 1);
}

#[test]
fn test_lockfile_version_matches_prefix_boundary() {
    assert!(lockfile_version_matches_prefix_boundary("1", "1"));
    assert!(lockfile_version_matches_prefix_boundary("1", "1.0.0"));
    assert!(lockfile_version_matches_prefix_boundary("1.7", "1.7.1"));
    assert!(lockfile_version_matches_prefix_boundary("1", "1-rc1"));
    assert!(lockfile_version_matches_prefix_boundary("1", "v1.0.0"));
    assert!(lockfile_version_matches_prefix_boundary("v1", "1.0.0"));
    assert!(lockfile_version_matches_prefix_boundary(
        "temurin-",
        "temurin-25.0.1"
    ));
    // the textual trap: a stale 10.0.0 must NOT satisfy prefix:1
    assert!(!lockfile_version_matches_prefix_boundary("1", "10.0.0"));
    assert!(!lockfile_version_matches_prefix_boundary("0.8", "0.81.0"));
    // an empty prefix must not match everything
    assert!(!lockfile_version_matches_prefix_boundary("", "1.0.0"));
    assert!(!lockfile_version_matches_prefix_boundary("2", "1.0.0"));
}

#[test]
fn test_cmp_lockfile_versions_newest_first() {
    use std::cmp::Ordering;

    // The tie `versions` cannot break on its own: both chunks reduce to `7`.
    // Without the version-string tie-break these compare Equal and the
    // caller's stable sort keeps whichever the file listed first.
    assert_eq!(
        cmp_lockfile_versions_newest_first("3.7b", "3.7c"),
        Ordering::Greater
    );
    assert_eq!(
        cmp_lockfile_versions_newest_first("3.7c", "3.7b"),
        Ordering::Less
    );

    // The tie-break must stay a tie-break: where the versions really do
    // order, the string must not get a say. Lexicographically "1.10.0" is
    // the smaller of these two.
    assert_eq!(
        cmp_lockfile_versions_newest_first("1.9.0", "1.10.0"),
        Ordering::Greater
    );
    assert_eq!(
        cmp_lockfile_versions_newest_first("1.10.0", "1.9.0"),
        Ordering::Less
    );

    // Non-semver, and the pair that separates this from every ordering
    // that falls back to file order: `semver::Version` parses neither, and
    // lexicographically "1.10b" comes first, so reading off the end of the
    // stored order answers "1.9b".
    assert_eq!(
        cmp_lockfile_versions_newest_first("1.9b", "1.10b"),
        Ordering::Greater
    );
    assert_eq!(
        cmp_lockfile_versions_newest_first("1.10b", "1.9b"),
        Ordering::Less
    );

    // A prerelease is older than its release. `Versioning::new` tries
    // `SemVer` before anything else, so this follows semver precedence
    // rather than the "package release revision" reading of `-rc1`.
    assert_eq!(
        cmp_lockfile_versions_newest_first("1.0.0-rc1", "1.0.0"),
        Ordering::Greater
    );

    // Identical strings still tie, so a stable sort leaves duplicate
    // entries in the order they were read.
    assert_eq!(
        cmp_lockfile_versions_newest_first("1.0.0", "1.0.0"),
        Ordering::Equal
    );
}

#[test]
fn test_latest_selection_picks_the_newer_of_a_tied_pair() {
    // The order `merge_tool_entries` writes: lexicographic by version
    // string. `get_locked_version` sorts this newest-first and takes the
    // head, so the head has to be `3.7c`.
    let mut entries = ["3.7b", "3.7c"];
    entries.sort_by(|a, b| cmp_lockfile_versions_newest_first(a, b));
    assert_eq!(entries.first(), Some(&"3.7c"));
}

#[test]
fn test_lockfile_version_matches_normalizes_v_prefix() {
    assert!(lockfile_version_matches("latest", "not-a-semver"));
    assert!(lockfile_version_matches("1.2", "v1.2.3"));
    assert!(lockfile_version_matches("v1.2", "1.2.3"));
    assert!(lockfile_version_matches("V1.2", "v1.2.3"));
    assert!(!lockfile_version_matches("1.3", "v1.2.3"));
    assert!(!lockfile_version_matches("v", "1.2.3"));
    assert!(!lockfile_version_matches("v", "v1.2.3"));
    assert!(lockfile_version_matches("v", "versioned"));
}

#[test]
fn test_lockfile_path_for_config() {
    // Simple case: mise.toml in project root
    let (path, is_local) = lockfile_path_for_config(Path::new("/foo/bar/mise.toml"), None);
    assert_eq!(path, PathBuf::from("/foo/bar/mise.lock"));
    assert!(!is_local);

    // Local config
    let (path, is_local) = lockfile_path_for_config(Path::new("/foo/bar/mise.local.toml"), None);
    assert_eq!(path, PathBuf::from("/foo/bar/mise.local.lock"));
    assert!(is_local);

    // Config in .config directory
    let (path, is_local) = lockfile_path_for_config(Path::new("/foo/bar/.config/mise.toml"), None);
    assert_eq!(path, PathBuf::from("/foo/bar/.config/mise.lock"));
    assert!(!is_local);

    // Config in .mise directory
    let (path, is_local) = lockfile_path_for_config(Path::new("/foo/bar/.mise/config.toml"), None);
    assert_eq!(path, PathBuf::from("/foo/bar/.mise/mise.lock"));
    assert!(!is_local);

    // Config in conf.d directory - should go to parent of conf.d
    let (path, is_local) =
        lockfile_path_for_config(Path::new("/foo/bar/.mise/conf.d/foo.toml"), None);
    assert_eq!(path, PathBuf::from("/foo/bar/.mise/mise.lock"));
    assert!(!is_local);

    // Config in .config/mise/conf.d directory
    let (path, is_local) =
        lockfile_path_for_config(Path::new("/foo/bar/.config/mise/conf.d/foo.toml"), None);
    assert_eq!(path, PathBuf::from("/foo/bar/.config/mise/mise.lock"));
    assert!(!is_local);

    // Config in mise/conf.d directory
    let (path, is_local) =
        lockfile_path_for_config(Path::new("/foo/bar/mise/conf.d/foo.toml"), None);
    assert_eq!(path, PathBuf::from("/foo/bar/mise/mise.lock"));
    assert!(!is_local);

    // conf.d folder fragments share the same lockfile as file fragments
    let (path, is_local) =
        lockfile_path_for_config(Path::new("/foo/bar/mise/conf.d/git/mise.toml"), None);
    assert_eq!(path, PathBuf::from("/foo/bar/mise/mise.lock"));
    assert!(!is_local);
    let (path, is_local) = lockfile_path_for_config(
        Path::new("/foo/bar/.mise/conf.d/git/mise.linux.local.toml"),
        None,
    );
    assert_eq!(path, PathBuf::from("/foo/bar/.mise/mise.linux.local.lock"));
    assert!(is_local);
}

#[test]
fn test_lockfile_path_for_config_monorepo_root_routing() {
    let monorepo_root = Path::new("/repo");

    let (path, is_local) = lockfile_path_for_config(
        Path::new("/repo/packages/api/mise.toml"),
        Some(monorepo_root),
    );
    assert_eq!(path, PathBuf::from("/repo/mise.lock"));
    assert!(!is_local);

    let (path, is_local) = lockfile_path_for_config(
        Path::new("/repo/packages/api/mise.ci.toml"),
        Some(monorepo_root),
    );
    assert_eq!(path, PathBuf::from("/repo/mise.ci.lock"));
    assert!(!is_local);

    let (path, is_local) = lockfile_path_for_config(
        Path::new("/repo/packages/api/mise.local.toml"),
        Some(monorepo_root),
    );
    assert_eq!(path, PathBuf::from("/repo/mise.local.lock"));
    assert!(is_local);

    let (path, is_local) =
        lockfile_path_for_config(Path::new("/repo/mise.toml"), Some(monorepo_root));
    assert_eq!(path, PathBuf::from("/repo/mise.lock"));
    assert!(!is_local);

    let (path, is_local) =
        lockfile_path_for_config(Path::new("/outside/mise.toml"), Some(monorepo_root));
    assert_eq!(path, PathBuf::from("/outside/mise.lock"));
    assert!(!is_local);

    let (path, is_local) = lockfile_path_for_config(
        Path::new("/repo/packages/api/.mise/conf.d/foo.toml"),
        Some(monorepo_root),
    );
    assert_eq!(path, PathBuf::from("/repo/mise.lock"));
    assert!(!is_local);
}

#[test]
fn test_preserve_absent_tool_entries_for_monorepo_upsert() {
    let existing = vec![basic_tool("20.0.0", "core:node")];
    let (mut merged, consumed) = merge_tool_entries(
        vec![basic_tool("22.0.0", "core:node")],
        Some(&existing),
        |_, _| None,
    );

    preserve_absent_tool_entries(&mut merged, Some(&existing), &consumed);

    let versions = merged
        .iter()
        .map(|tool| tool.version.as_str())
        .collect_vec();
    assert_eq!(versions.len(), 2);
    assert!(versions.contains(&"20.0.0"));
    assert!(versions.contains(&"22.0.0"));
}

#[test]
fn test_reinsert_deferred_baselines_restores_dropped_entry_without_duplicates() {
    let baseline = basic_tool("0.11.0", "github:owner/repo");
    let deferred = HashMap::from([("repo".to_string(), vec![baseline.clone(), baseline.clone()])]);
    let mut merged = vec![basic_tool("0.12.0", "github:owner/repo")];

    reinsert_deferred_baselines(&mut merged, &deferred, "repo");

    assert_eq!(
        merged
            .iter()
            .filter(|tool| tool.version == baseline.version && tool.options == baseline.options)
            .count(),
        1
    );
}

/// A github tool entry whose provenance was cryptographically verified on `platform`.
fn verified_github_tool(version: &str, platform: &str) -> LockfileTool {
    let mut tool = basic_tool(version, "github:owner/repo");
    tool.platforms.insert(
        platform.to_string(),
        PlatformInfo {
            url: Some(format!("https://example.com/repo-{version}.tar.gz")),
            provenance: Some(ProvenanceType::GithubAttestations),
            provenance_verified: Some(true),
            ..Default::default()
        },
    );
    tool
}
/// The versions of `tools` in lockfile order.
fn lockfile_tool_versions(tools: &[LockfileTool]) -> Vec<&str> {
    tools.iter().map(|tool| tool.version.as_str()).collect()
}

#[test]
fn test_prune_verified_provenance_baselines_drops_resolved_baseline() {
    // `mise use repo@0.12.0` with 0.12.0 already installed skips the download, so
    // update_lockfiles keeps 0.11.0 as a provenance baseline. Once auto-lock has
    // verified 0.12.0 the baseline is just a stale duplicate and must go.
    let platform = Platform::current().to_key();
    let requested: BTreeSet<String> = ["0.12.0".to_string()].into();

    let mut tools = vec![
        verified_github_tool("0.12.0", &platform),
        verified_github_tool("0.11.0", &platform),
    ];
    let pruned = prune_verified_provenance_baselines(&mut tools, &requested, &platform);
    assert_eq!(pruned, vec!["0.11.0".to_string()]);
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0"]);

    // Repeated unresolved upgrades can stack several baselines; all of them go.
    let mut tools = vec![
        verified_github_tool("0.12.0", &platform),
        verified_github_tool("0.10.0", &platform),
        verified_github_tool("0.11.0", &platform),
    ];
    let pruned = prune_verified_provenance_baselines(&mut tools, &requested, &platform);
    assert_eq!(pruned, vec!["0.11.0".to_string(), "0.10.0".to_string()]);
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0"]);
}

#[test]
fn test_prune_verified_provenance_baselines_keeps_entries_still_needed() {
    let platform = Platform::current().to_key();
    let requested: BTreeSet<String> = ["0.12.0".to_string()].into();

    // The upgrade is still unverified on this platform: the baseline is what the
    // deferred regression check compares against, so it must stay.
    let mut tools = vec![
        basic_tool("0.12.0", "github:owner/repo"),
        verified_github_tool("0.11.0", &platform),
    ];
    assert!(prune_verified_provenance_baselines(&mut tools, &requested, &platform).is_empty());
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0", "0.11.0"]);

    // Verified on another platform only: this platform's check is still pending.
    let mut tools = vec![
        verified_github_tool("0.12.0", "other-platform"),
        verified_github_tool("0.11.0", &platform),
    ];
    assert!(prune_verified_provenance_baselines(&mut tools, &requested, &platform).is_empty());
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0", "0.11.0"]);

    // Recorded provenance is authoritative regardless of the legacy bit.
    let mut detected_only = verified_github_tool("0.12.0", &platform);
    detected_only
        .platforms
        .get_mut(&platform)
        .unwrap()
        .provenance_verified = None;
    let mut tools = vec![detected_only, verified_github_tool("0.11.0", &platform)];
    assert_eq!(
        prune_verified_provenance_baselines(&mut tools, &requested, &platform),
        vec!["0.11.0"]
    );
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0"]);

    // Two option variants of the requested version, only one verified: the other
    // still relies on the baseline for its deferred check.
    let mut musl_variant = basic_tool("0.12.0", "github:owner/repo");
    musl_variant
        .options
        .insert("flavor".to_string(), "musl".to_string());
    let mut tools = vec![
        verified_github_tool("0.12.0", &platform),
        musl_variant,
        verified_github_tool("0.11.0", &platform),
    ];
    assert!(prune_verified_provenance_baselines(&mut tools, &requested, &platform).is_empty());
    assert_eq!(
        lockfile_tool_versions(&tools),
        vec!["0.12.0", "0.12.0", "0.11.0"]
    );

    // Once that variant verifies as well, the baseline has served both and goes.
    let mut musl_variant = verified_github_tool("0.12.0", &platform);
    musl_variant
        .options
        .insert("flavor".to_string(), "musl".to_string());
    let mut tools = vec![
        verified_github_tool("0.12.0", &platform),
        musl_variant,
        verified_github_tool("0.11.0", &platform),
    ];
    assert_eq!(
        prune_verified_provenance_baselines(&mut tools, &requested, &platform),
        vec!["0.11.0".to_string()]
    );
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0", "0.12.0"]);

    // Both versions are requested (multi-version config): neither is a baseline.
    let both: BTreeSet<String> = ["0.11.0".to_string(), "0.12.0".to_string()].into();
    let mut tools = vec![
        verified_github_tool("0.12.0", &platform),
        verified_github_tool("0.11.0", &platform),
    ];
    assert!(prune_verified_provenance_baselines(&mut tools, &both, &platform).is_empty());
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0", "0.11.0"]);

    // A downgrade never has a baseline: the newer, unrequested entry is not ours to drop.
    let downgrade: BTreeSet<String> = ["0.11.0".to_string()].into();
    let mut tools = vec![
        verified_github_tool("0.12.0", &platform),
        verified_github_tool("0.11.0", &platform),
    ];
    assert!(prune_verified_provenance_baselines(&mut tools, &downgrade, &platform).is_empty());
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0", "0.11.0"]);

    // A prior version without provenance was never a baseline.
    let mut tools = vec![
        verified_github_tool("0.12.0", &platform),
        basic_tool("0.11.0", "github:owner/repo"),
    ];
    assert!(prune_verified_provenance_baselines(&mut tools, &requested, &platform).is_empty());
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0", "0.11.0"]);

    // Provenance baselines only exist for github backends.
    let mut tools = vec![
        verified_github_tool("0.12.0", &platform),
        verified_github_tool("0.11.0", &platform),
    ];
    for tool in &mut tools {
        tool.backend = Some("aqua:owner/repo".to_string());
    }
    assert!(prune_verified_provenance_baselines(&mut tools, &requested, &platform).is_empty());
    assert_eq!(lockfile_tool_versions(&tools), vec!["0.12.0", "0.11.0"]);
}
#[test]
fn test_preserve_absent_does_not_resurrect_rekeyed_empty_options_entry() {
    // #10564: after merge_tool_entries rekeys an empty-options entry into a
    // same-version optioned entry, the monorepo preserve pass must not
    // resurrect the old empty-options entry as a duplicate sibling.
    let mut platforms = BTreeMap::new();
    platforms.insert(
        "linux-x64".to_string(),
        PlatformInfo {
            checksum: Some("sha256:OLD".to_string()),
            ..Default::default()
        },
    );
    let existing = vec![LockfileTool {
        version: "26.0.1".to_string(),
        backend: Some("core:java".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::new(),
        platforms,
        aube: None,
        uv: None,
    }];
    let mut options = BTreeMap::new();
    options.insert("shorthand_vendor".to_string(), "openjdk".to_string());
    let fresh_platforms = BTreeMap::from([(
        "linux-x64".to_string(),
        PlatformInfo {
            checksum: Some("sha256:NEW".to_string()),
            ..Default::default()
        },
    )]);
    let (mut merged, consumed) = merge_tool_entries(
        vec![LockfileTool {
            version: "26.0.1".to_string(),
            backend: Some("core:java".to_string()),
            specifiers: BTreeSet::new(),
            options: options.clone(),
            platforms: fresh_platforms,
            aube: None,
            uv: None,
        }],
        Some(&existing),
        |_, _| Some(options.clone()),
    );

    preserve_absent_tool_entries(&mut merged, Some(&existing), &consumed);

    // Single entry: rekeyed, not duplicated — and it kept the platforms.
    assert_eq!(merged.len(), 1, "rekeyed entry duplicated: {merged:?}");
    assert_eq!(
        merged[0].platforms["linux-x64"].checksum.as_deref(),
        Some("sha256:NEW")
    );

    // Counter-case: when the tool was NOT resolved this run (merged has no
    // same-version optioned entry), a fragmented existing lockfile must be
    // preserved verbatim — both siblings survive, no data loss.
    let mut fragmented = existing.clone();
    fragmented.push(LockfileTool {
        version: "26.0.1".to_string(),
        backend: Some("core:java".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::from([("shorthand_vendor".to_string(), "openjdk".to_string())]),
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    });
    let mut merged = vec![basic_tool("22.0.0", "core:node")];
    preserve_absent_tool_entries(&mut merged, Some(&fragmented), &HashSet::new());
    assert_eq!(
        merged.len(),
        3,
        "unresolved entries must be preserved: {merged:?}"
    );
}

#[test]
fn test_merge_tool_entries_rekeys_empty_options_entry_on_new_options() {
    // Regression guard for #10564: when a freshly-resolved entry for a
    // version gains non-empty `options` (e.g. java's shorthand_vendor)
    // while the on-disk lockfile still has that version keyed under empty
    // options, the old entry's platforms must be rekeyed, not dropped.
    let mut existing_platforms = BTreeMap::new();
    existing_platforms.insert(
        "linux-x64".to_string(),
        PlatformInfo {
            checksum: Some("sha256:OLD".to_string()),
            url: Some("https://example.com/java-linux-x64.tar.gz".to_string()),
            ..Default::default()
        },
    );
    existing_platforms.insert(
        "macos-arm64".to_string(),
        PlatformInfo {
            checksum: Some("sha256:OLD-MAC".to_string()),
            url: Some("https://example.com/java-macos-arm64.tar.gz".to_string()),
            ..Default::default()
        },
    );
    let existing = vec![LockfileTool {
        version: "26.0.1".to_string(),
        backend: Some("core:java".to_string()),
        specifiers: BTreeSet::from(["latest".to_string()]),
        options: BTreeMap::new(),
        platforms: existing_platforms,
        aube: None,
        uv: None,
    }];

    let mut new_options = BTreeMap::new();
    new_options.insert("shorthand_vendor".to_string(), "openjdk".to_string());
    let mut fresh_platforms = BTreeMap::new();
    fresh_platforms.insert(
        "linux-x64".to_string(),
        PlatformInfo {
            checksum: Some("sha256:NEW".to_string()),
            url: Some("https://example.com/java-linux-x64.tar.gz".to_string()),
            ..Default::default()
        },
    );
    let fresh = vec![LockfileTool {
        version: "26.0.1".to_string(),
        backend: Some("core:java".to_string()),
        specifiers: BTreeSet::from(["26".to_string()]),
        options: new_options.clone(),
        platforms: fresh_platforms,
        aube: None,
        uv: None,
    }];

    let (merged, consumed) =
        merge_tool_entries(fresh, Some(&existing), |_, _| Some(new_options.clone()));

    assert!(consumed.contains(&("26.0.1".to_string(), BTreeMap::new())));
    assert_eq!(
        merged.len(),
        1,
        "expected a single merged entry, got {merged:?}"
    );
    let tool = &merged[0];
    assert_eq!(tool.options, new_options);
    assert_eq!(
        tool.specifiers,
        BTreeSet::from(["26".to_string(), "latest".to_string()])
    );
    assert_eq!(tool.platforms.len(), 2);
    assert_eq!(
        tool.platforms["linux-x64"].checksum.as_deref(),
        Some("sha256:NEW")
    );
    assert_eq!(
        tool.platforms["macos-arm64"].checksum.as_deref(),
        Some("sha256:OLD-MAC")
    );
}

#[test]
fn test_merge_tool_entries_preserves_disjoint_empty_options_variant() {
    let unix_options = BTreeMap::from([("compile".to_string(), "false".to_string())]);
    let existing = vec![
        LockfileTool {
            version: "3.4.2".to_string(),
            backend: Some("core:ruby".to_string()),
            specifiers: BTreeSet::new(),
            options: unix_options.clone(),
            platforms: BTreeMap::from([(
                "linux-x64".to_string(),
                PlatformInfo {
                    url: Some("https://example.com/ruby-linux.tar.gz".to_string()),
                    ..Default::default()
                },
            )]),
            aube: None,
            uv: None,
        },
        LockfileTool {
            version: "3.4.2".to_string(),
            backend: Some("core:ruby".to_string()),
            specifiers: BTreeSet::new(),
            options: BTreeMap::new(),
            platforms: BTreeMap::from([(
                "windows-x64".to_string(),
                PlatformInfo {
                    url: Some("https://example.com/ruby-windows.7z".to_string()),
                    ..Default::default()
                },
            )]),
            aube: None,
            uv: None,
        },
    ];
    let fresh = vec![
        LockfileTool {
            version: "3.4.2".to_string(),
            backend: Some("core:ruby".to_string()),
            specifiers: BTreeSet::new(),
            options: unix_options,
            platforms: BTreeMap::from([(
                "linux-x64".to_string(),
                PlatformInfo {
                    url: Some("https://example.com/ruby-linux.tar.gz".to_string()),
                    ..Default::default()
                },
            )]),
            aube: None,
            uv: None,
        },
        LockfileTool {
            version: "3.4.2".to_string(),
            backend: Some("core:ruby".to_string()),
            specifiers: BTreeSet::new(),
            options: BTreeMap::new(),
            platforms: BTreeMap::from([(
                "windows-x64".to_string(),
                PlatformInfo {
                    url: Some("https://example.com/ruby-windows.7z".to_string()),
                    ..Default::default()
                },
            )]),
            aube: None,
            uv: None,
        },
    ];

    let (merged, consumed) = merge_tool_entries(fresh, Some(&existing), |_, platform| {
        if platform.is_windows() {
            Some(BTreeMap::new())
        } else {
            Some(BTreeMap::from([(
                "compile".to_string(),
                "false".to_string(),
            )]))
        }
    });

    assert!(consumed.is_empty());
    assert_eq!(
        merged.len(),
        2,
        "option variants were conflated: {merged:?}"
    );
    assert!(
        merged
            .iter()
            .any(|tool| { tool.options.is_empty() && tool.platforms.contains_key("windows-x64") })
    );
    assert!(
        merged.iter().any(|tool| {
            !tool.options.is_empty() && !tool.platforms.contains_key("windows-x64")
        })
    );

    let mut lockfile = Lockfile::default();
    lockfile.tools.insert("ruby".to_string(), merged);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    lockfile.save(&path).unwrap();
    let first = std::fs::read_to_string(&path).unwrap();
    Lockfile::read(&path).unwrap().save(&path).unwrap();
    let second = std::fs::read_to_string(&path).unwrap();

    assert_eq!(first, second, "lockfile serialization was not idempotent");
    assert_eq!(second.matches("[[tools.ruby]]").count(), 2);
    assert_eq!(second.matches("platforms.windows-x64").count(), 1);
}

#[test]
fn locked_backend_inherits_only_an_unambiguous_backend() {
    let entry = |version: &str, backend: &str| LockfileTool {
        version: version.to_string(),
        backend: Some(backend.to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::new(),
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    };
    let aqua_old = entry("1.57.0", "aqua:jdx/hk");
    let aqua_new = entry("1.58.1", "aqua:jdx/hk");
    let packslip_new = entry("1.58.1", "packslip:github.com/jdx/hk");

    let one_backend = [&aqua_old, &aqua_new];
    assert_eq!(
        locked_backend(&one_backend, Some("2.0.0")).as_deref(),
        Some("aqua:jdx/hk")
    );

    let split = [&aqua_old, &packslip_new];
    assert_eq!(
        locked_backend(&split, Some("1.58.1")).as_deref(),
        Some("packslip:github.com/jdx/hk")
    );
    assert_eq!(
        locked_backend(&split, Some("1.57.0")).as_deref(),
        Some("aqua:jdx/hk")
    );
    assert_eq!(locked_backend(&split, Some("2.0.0")), None);
    assert_eq!(locked_backend(&split, None).as_deref(), Some("aqua:jdx/hk"));
}

#[test]
fn switch_backend_moves_only_the_given_versions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    std::fs::write(
        &path,
        r#"lockfile_version = 2

[[tools.hk]]
version = "1.57.0"
backend = "aqua:jdx/hk"

[tools.hk."platforms.linux-x64"]
url = "https://example.com/hk-1.57.0"

[[tools.hk]]
version = "1.58.1"
backend = "aqua:jdx/hk"

[tools.hk."platforms.linux-x64"]
checksum = "sha256:abc"
url = "https://example.com/hk-1.58.1"

[tools.hk."platforms.macos-arm64"]
url = "https://example.com/hk-1.58.1-mac"
"#,
    )
    .unwrap();
    let mut lockfile = Lockfile::read(&path).unwrap();
    let moved = lockfile.switch_backend(
        "hk",
        "aqua:jdx/hk",
        &BTreeSet::from(["1.58.1".to_string()]),
        |_| Some("packslip:github.com/jdx/hk".to_string()),
    );
    assert_eq!(
        moved,
        vec![(
            "1.58.1".to_string(),
            "packslip:github.com/jdx/hk".to_string(),
            true
        )]
    );
    assert!(!lockfile.has_platforms("hk", "1.58.1"));
    assert!(lockfile.has_platforms("hk", "1.57.0"));
    lockfile.save(&path).unwrap();

    // The switched entry drops the old backend's artifacts; the version
    // that was not asked for keeps its backend and artifacts.
    let reread = Lockfile::read(&path).unwrap();
    let entries = reread.tools_for("hk").unwrap();
    let switched = entries.iter().find(|t| t.version == "1.58.1").unwrap();
    assert_eq!(
        switched.backend.as_deref(),
        Some("packslip:github.com/jdx/hk")
    );
    assert!(switched.platforms.is_empty());
    let kept = entries.iter().find(|t| t.version == "1.57.0").unwrap();
    assert_eq!(kept.backend.as_deref(), Some("aqua:jdx/hk"));
    assert!(reread.has_platforms("hk", "1.57.0"));
}

#[test]
fn test_merge_tool_entries_does_not_copy_changed_options_data() {
    let old_options = BTreeMap::from([("flavor".to_string(), "old".to_string())]);
    let new_options = BTreeMap::from([("flavor".to_string(), "new".to_string())]);
    let existing = vec![LockfileTool {
        version: "1.0.0".to_string(),
        backend: Some("example:tool".to_string()),
        specifiers: BTreeSet::new(),
        options: old_options,
        platforms: BTreeMap::from([(
            "linux-x64".to_string(),
            PlatformInfo {
                checksum: Some("sha256:OLD".to_string()),
                ..Default::default()
            },
        )]),
        aube: None,
        uv: None,
    }];
    let fresh = vec![LockfileTool {
        version: "1.0.0".to_string(),
        backend: Some("example:tool".to_string()),
        specifiers: BTreeSet::new(),
        options: new_options.clone(),
        platforms: BTreeMap::from([(
            "linux-x64".to_string(),
            PlatformInfo {
                checksum: Some("sha256:NEW".to_string()),
                ..Default::default()
            },
        )]),
        aube: None,
        uv: None,
    }];

    let (merged, consumed) =
        merge_tool_entries(fresh, Some(&existing), |_, _| Some(new_options.clone()));

    assert!(consumed.is_empty());
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].platforms["linux-x64"].checksum.as_deref(),
        Some("sha256:NEW")
    );
}

#[test]
fn test_merge_tool_entries_preserves_data_when_resolution_fails() {
    let existing = vec![LockfileTool {
        version: "26.0.1".to_string(),
        backend: Some("core:java".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::new(),
        platforms: BTreeMap::from([(
            "linux-x64".to_string(),
            PlatformInfo {
                checksum: Some("sha256:OLD".to_string()),
                ..Default::default()
            },
        )]),
        aube: None,
        uv: None,
    }];
    let fresh = vec![LockfileTool {
        version: "26.0.1".to_string(),
        backend: Some("core:java".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::from([("shorthand_vendor".to_string(), "openjdk".to_string())]),
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    }];

    let (merged, consumed) = merge_tool_entries(fresh, Some(&existing), |_, _| None);

    assert!(consumed.is_empty());
    assert_eq!(merged.len(), 2);
    assert!(merged.iter().any(|tool| {
        tool.options.is_empty()
            && tool.platforms["linux-x64"].checksum.as_deref() == Some("sha256:OLD")
    }));
}

#[test]
fn test_merge_tool_entries_keeps_other_platform_option_variants() {
    let variant = |pattern: &str, platform: &str, checksum: &str| LockfileTool {
        version: "0.11.0".to_string(),
        backend: Some("github:koalaman/shellcheck".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::from([("asset_pattern".to_string(), pattern.to_string())]),
        platforms: BTreeMap::from([(
            platform.to_string(),
            PlatformInfo {
                checksum: Some(checksum.to_string()),
                ..Default::default()
            },
        )]),
        aube: None,
        uv: None,
    };
    let existing = vec![
        variant("shellcheck-*.zip", "windows-x64", "sha256:WIN"),
        variant("shellcheck-*.tar.xz", "linux-x64", "sha256:LINUX"),
    ];
    // Only the current platform's variant is resolved from the toolset.
    let mut fresh_linux = variant("shellcheck-*.tar.xz", "linux-x64", "sha256:LINUX");
    fresh_linux.platforms.clear();

    let (merged, _) = merge_tool_entries(vec![fresh_linux], Some(&existing), |_, _| None);

    assert_eq!(merged.len(), 2);
    assert!(merged.iter().any(|tool| {
        tool.options["asset_pattern"] == "shellcheck-*.zip"
            && tool.platforms["windows-x64"].checksum.as_deref() == Some("sha256:WIN")
    }));
    assert!(merged.iter().any(|tool| {
        tool.options["asset_pattern"] == "shellcheck-*.tar.xz"
            && tool.platforms["linux-x64"].checksum.as_deref() == Some("sha256:LINUX")
    }));
}

#[test]
fn test_merge_tool_entries_keeps_unrefreshed_platforms_of_changed_variant() {
    let tool = |pattern: &str, platforms: &[(&str, &str)]| LockfileTool {
        version: "0.11.0".to_string(),
        backend: Some("github:koalaman/shellcheck".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::from([("asset_pattern".to_string(), pattern.to_string())]),
        platforms: platforms
            .iter()
            .map(|(platform, checksum)| {
                (
                    platform.to_string(),
                    PlatformInfo {
                        checksum: Some(checksum.to_string()),
                        ..Default::default()
                    },
                )
            })
            .collect(),
        aube: None,
        uv: None,
    };
    let existing = vec![tool(
        "shellcheck-*.old",
        &[
            ("linux-x64", "sha256:OLDLINUX"),
            ("windows-x64", "sha256:WIN"),
        ],
    )];
    let fresh = vec![tool(
        "shellcheck-*.new",
        &[("linux-x64", "sha256:NEWLINUX")],
    )];

    let (merged, _) = merge_tool_entries(fresh, Some(&existing), |_, _| None);

    assert_eq!(merged.len(), 2);
    let old = merged
        .iter()
        .find(|t| t.options["asset_pattern"] == "shellcheck-*.old")
        .unwrap();
    assert_eq!(old.platforms.len(), 1);
    assert_eq!(
        old.platforms["windows-x64"].checksum.as_deref(),
        Some("sha256:WIN")
    );
}

#[test]
fn test_merge_tool_entries_retained_variants_do_not_count_as_fresh() {
    let tool = |pattern: &str, platforms: &[&str]| LockfileTool {
        version: "0.11.0".to_string(),
        backend: Some("github:koalaman/shellcheck".to_string()),
        specifiers: BTreeSet::new(),
        options: BTreeMap::from([("asset_pattern".to_string(), pattern.to_string())]),
        platforms: platforms
            .iter()
            .map(|platform| (platform.to_string(), PlatformInfo::default()))
            .collect(),
        aube: None,
        uv: None,
    };
    let existing = vec![
        tool("a", &["windows-x64"]),
        tool("b", &["windows-x64", "macos-arm64"]),
    ];
    let fresh = vec![tool("c", &["linux-x64"])];

    let (merged, _) = merge_tool_entries(fresh, Some(&existing), |_, _| None);

    for pattern in ["a", "b"] {
        let kept = merged
            .iter()
            .find(|t| t.options["asset_pattern"] == pattern)
            .unwrap();
        assert!(kept.platforms.contains_key("windows-x64"));
    }
}

#[test]
fn test_record_rekey_decision_requires_consensus() {
    let key = ("1.0.0".to_string(), "linux-x64".to_string());
    let options_a = BTreeMap::from([("flavor".to_string(), "a".to_string())]);
    let options_b = BTreeMap::from([("flavor".to_string(), "b".to_string())]);
    let mut decisions = RekeyDecisions::new();

    record_rekey_decision(&mut decisions, key.clone(), Some(options_a.clone()));
    record_rekey_decision(&mut decisions, key.clone(), Some(options_a));
    assert!(decisions[&key].is_some());

    record_rekey_decision(&mut decisions, key.clone(), Some(options_b));
    assert_eq!(decisions[&key], None);

    // Once requests disagree, later candidates cannot make the decision
    // authoritative again.
    record_rekey_decision(
        &mut decisions,
        key.clone(),
        Some(BTreeMap::from([("flavor".to_string(), "a".to_string())])),
    );
    assert_eq!(decisions[&key], None);
}

#[test]
fn test_merge_tool_entries_repairs_empty_non_empty_duplicate() {
    let unix_options = BTreeMap::from([("compile".to_string(), "false".to_string())]);
    let existing = vec![
        LockfileTool {
            version: "3.4.2".to_string(),
            backend: Some("core:ruby".to_string()),
            specifiers: BTreeSet::new(),
            options: BTreeMap::new(),
            platforms: BTreeMap::from([(
                "linux-x64".to_string(),
                PlatformInfo {
                    checksum: Some("sha256:LEGACY".to_string()),
                    ..Default::default()
                },
            )]),
            aube: None,
            uv: None,
        },
        LockfileTool {
            version: "3.4.2".to_string(),
            backend: Some("core:ruby".to_string()),
            specifiers: BTreeSet::new(),
            options: unix_options.clone(),
            platforms: BTreeMap::from([(
                "linux-x64".to_string(),
                PlatformInfo {
                    checksum: Some("sha256:OLD".to_string()),
                    ..Default::default()
                },
            )]),
            aube: None,
            uv: None,
        },
    ];
    let fresh = vec![LockfileTool {
        version: "3.4.2".to_string(),
        backend: Some("core:ruby".to_string()),
        specifiers: BTreeSet::new(),
        options: unix_options.clone(),
        platforms: BTreeMap::from([(
            "linux-x64".to_string(),
            PlatformInfo {
                checksum: Some("sha256:NEW".to_string()),
                ..Default::default()
            },
        )]),
        aube: None,
        uv: None,
    }];

    let (merged, consumed) =
        merge_tool_entries(fresh, Some(&existing), |_, _| Some(unix_options.clone()));

    assert!(consumed.contains(&("3.4.2".to_string(), BTreeMap::new())));
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].platforms["linux-x64"].checksum.as_deref(),
        Some("sha256:NEW")
    );
}

#[test]
fn test_preserve_absent_keeps_other_non_empty_monorepo_variant() {
    let current_options = BTreeMap::from([("vendor".to_string(), "current".to_string())]);
    let other_options = BTreeMap::from([("vendor".to_string(), "other".to_string())]);
    let existing = vec![LockfileTool {
        version: "1.0.0".to_string(),
        backend: Some("example:tool".to_string()),
        specifiers: BTreeSet::new(),
        options: other_options.clone(),
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    }];
    let fresh = vec![LockfileTool {
        version: "1.0.0".to_string(),
        backend: Some("example:tool".to_string()),
        specifiers: BTreeSet::new(),
        options: current_options,
        platforms: BTreeMap::new(),
        aube: None,
        uv: None,
    }];
    let (mut merged, consumed) = merge_tool_entries(fresh, Some(&existing), |_, _| None);

    preserve_absent_tool_entries(&mut merged, Some(&existing), &consumed);

    assert_eq!(merged.len(), 2);
    assert!(merged.iter().any(|tool| tool.options == other_options));
}

#[test]
fn test_tool_version_needs_auto_lock_for_bare_updated_entry() {
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "ruff".to_string(),
        vec![basic_tool("0.15.20", "aqua:astral-sh/ruff")],
    );

    let tv = basic_tv("aqua:astral-sh/ruff", "0.15.20");

    assert!(tool_version_needs_auto_lock(&lockfile, &tv, &[Platform::current()]).unwrap());
}

#[test]
fn test_tool_version_needs_auto_lock_ignores_complete_entry() {
    let mut lockfile = Lockfile::default();
    let mut tool = basic_tool("0.15.20", "aqua:astral-sh/ruff");
    tool.platforms.insert(
        Platform::current().to_key(),
        PlatformInfo {
            checksum: Some("sha256:abc123".to_string()),
            url: Some("https://example.com/ruff.tar.gz".to_string()),
            ..Default::default()
        },
    );
    lockfile.tools.insert("ruff".to_string(), vec![tool]);

    let tv = basic_tv("aqua:astral-sh/ruff", "0.15.20");

    assert!(!tool_version_needs_auto_lock(&lockfile, &tv, &[Platform::current()]).unwrap());
}

#[test]
fn test_tool_version_needs_auto_lock_ignores_pruned_version() {
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "ruff".to_string(),
        vec![basic_tool("0.15.20", "aqua:astral-sh/ruff")],
    );

    let stale = basic_tv("aqua:astral-sh/ruff", "0.15.19");

    assert!(!tool_version_needs_auto_lock(&lockfile, &stale, &[Platform::current()]).unwrap());
}

#[test]
fn test_tool_version_needs_auto_lock_uses_target_specific_options() {
    let unix_options = BTreeMap::from([("compile".to_string(), "false".to_string())]);
    let tools = vec![
        LockfileTool {
            version: "3.4.2".to_string(),
            backend: Some("core:ruby".to_string()),
            specifiers: BTreeSet::new(),
            options: unix_options.clone(),
            platforms: BTreeMap::from([(
                "linux-x64".to_string(),
                PlatformInfo {
                    checksum: Some("sha256:linux".to_string()),
                    url: Some("https://example.com/ruby-linux.tar.gz".to_string()),
                    ..Default::default()
                },
            )]),
            aube: None,
            uv: None,
        },
        LockfileTool {
            version: "3.4.2".to_string(),
            backend: Some("core:ruby".to_string()),
            specifiers: BTreeSet::new(),
            options: BTreeMap::new(),
            platforms: BTreeMap::from([(
                "windows-x64".to_string(),
                PlatformInfo {
                    checksum: Some("sha256:windows".to_string()),
                    url: Some("https://example.com/ruby-windows.7z".to_string()),
                    ..Default::default()
                },
            )]),
            aube: None,
            uv: None,
        },
    ];

    assert!(!lockfile_target_needs_auto_lock(
        &tools,
        "3.4.2",
        "linux-x64",
        &unix_options,
    ));
    assert!(!lockfile_target_needs_auto_lock(
        &tools,
        "3.4.2",
        "windows-x64",
        &BTreeMap::new(),
    ));
    assert!(lockfile_target_needs_auto_lock(
        &tools,
        "3.4.2",
        "windows-x64",
        &unix_options,
    ));
}

#[test]
fn test_complete_target_still_auto_locks_deferred_provenance_upgrade() {
    let platform = Platform::current().to_key();
    let mut baseline = basic_tool("0.11.0", "github:owner/repo");
    baseline.platforms.insert(
        platform.clone(),
        PlatformInfo {
            provenance: Some(ProvenanceType::GithubAttestations),
            ..Default::default()
        },
    );
    let mut pending = basic_tool("0.12.0", "github:owner/repo");
    pending.platforms.insert(
        platform.clone(),
        PlatformInfo {
            checksum: Some("sha256:pending".to_string()),
            url: Some("https://example.com/repo-0.12.0.tar.gz".to_string()),
            ..Default::default()
        },
    );
    let mut tools = vec![baseline, pending];

    assert!(
        lockfile_target_needs_auto_lock(&tools, "0.12.0", &platform, &BTreeMap::new()),
        "complete URL/checksum data must not skip deferred provenance verification"
    );

    tools[1].platforms.get_mut(&platform).unwrap().provenance =
        Some(ProvenanceType::GithubAttestations);
    assert!(!lockfile_target_needs_auto_lock(
        &tools,
        "0.12.0",
        &platform,
        &BTreeMap::new()
    ));
}

#[test]
fn test_no_new_versions_retries_only_deferred_provenance() {
    let platform = Platform::current();
    let platform_key = platform.to_key();
    let tv = basic_tv("github:owner/repo", "0.12.0");
    let mut baseline = basic_tool("0.11.0", "github:owner/repo");
    baseline.platforms.insert(
        platform_key.clone(),
        PlatformInfo {
            provenance: Some(ProvenanceType::GithubAttestations),
            ..Default::default()
        },
    );
    let pending = basic_tool("0.12.0", "github:owner/repo");
    let mut lockfile = Lockfile::default();
    lockfile
        .tools
        .insert("repo".to_string(), vec![baseline, pending.clone()]);

    assert!(
        tool_version_needs_deferred_provenance_retry(
            &lockfile,
            &tv,
            std::slice::from_ref(&platform),
        )
        .unwrap(),
        "a later install with no new versions must retry deferred verification"
    );

    lockfile.tools.insert("repo".to_string(), vec![pending]);
    assert!(
        !tool_version_needs_deferred_provenance_retry(
            &lockfile,
            &tv,
            std::slice::from_ref(&platform),
        )
        .unwrap(),
        "ordinary incomplete lock entries must not broaden the no-new-version retry"
    );
}

#[test]
fn test_merge_lockfile_preserving_root_keeps_root_on_conflict() {
    let mut root = Lockfile::default();
    root.tools
        .insert("node".to_string(), vec![basic_tool("20.0.0", "core:node")]);

    let mut subproject = Lockfile::default();
    subproject.tools.insert(
        "node".to_string(),
        vec![
            basic_tool("20.0.0", "core:node-alt"),
            basic_tool("22.0.0", "core:node"),
        ],
    );

    merge_lockfile_preserving_root(&mut root, subproject);

    let tools = root.tools_for("node").unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].version, "20.0.0");
    assert_eq!(tools[0].backend.as_deref(), Some("core:node"));
    assert_eq!(tools[1].version, "22.0.0");
}

#[test]
fn test_lookup_merge_preserves_uv_only_for_matching_backends() {
    for (primary_backend, legacy_backend, primary_graph, inherit) in [
        (Some("pypi:black"), Some("pypi:black"), false, true),
        (Some("pypi:black"), Some("pypi:black"), true, false),
        (Some("github:psf/black"), Some("pypi:black"), false, false),
        (None, Some("pypi:black"), false, false),
        (Some("pypi:black"), None, false, false),
        (None, None, false, false),
    ] {
        let mut primary = basic_tool("1.0.0", "pypi:black");
        primary.backend = primary_backend.map(str::to_owned);
        primary.uv = primary_graph.then(|| UvLock::default().into());
        let original_graph = primary.uv.clone();
        let mut legacy = basic_tool("1.0.0", "pypi:black");
        legacy.backend = legacy_backend.map(str::to_owned);
        legacy.specifiers.insert("latest".to_string());
        let graph = UvLock {
            graph: toml::toml! { revision = 3 },
            ..Default::default()
        };
        legacy.uv = Some(graph.clone().into());
        let mut root = Lockfile::default();
        root.tools.insert("black".to_string(), vec![primary]);
        let mut other = Lockfile::default();
        other.tools.insert("black".to_string(), vec![legacy]);

        merge_lockfile_for_lookup(&mut root, other);

        assert_eq!(root.tools["black"].len(), 1);
        let merged = &root.tools["black"][0];
        assert!(merged.specifiers.contains("latest"));
        assert_eq!(
            merged.uv,
            if inherit {
                Some(graph.into())
            } else {
                original_graph
            }
        );
    }
}

#[test]
fn test_merge_lockfile_for_lookup_keeps_request_bindings_enabled() {
    for (root_version, other_version) in [(0, 1), (1, 0), (1, 2), (2, 1)] {
        let mut root = Lockfile {
            lockfile_version: root_version,
            ..Default::default()
        };
        root.tools.insert(
            "node".to_string(),
            vec![LockfileTool {
                version: "20.0.0".to_string(),
                backend: Some("core:node".to_string()),
                specifiers: BTreeSet::from(["20".to_string()]),
                options: BTreeMap::new(),
                platforms: BTreeMap::new(),
                aube: None,
                uv: None,
            }],
        );
        let mut other = Lockfile {
            lockfile_version: other_version,
            ..Default::default()
        };
        other
            .tools
            .insert("node".to_string(), vec![basic_tool("22.0.0", "core:node")]);

        merge_lockfile_for_lookup(&mut root, other);

        assert_eq!(root.lockfile_version(), root_version.max(other_version));
        assert!(root.uses_request_bindings());
        assert!(root.tools["node"][0].specifiers.contains("20"));
        assert!(root.tools["node"][1].specifiers.is_empty());
        assert_eq!(
            root.entry_lockfile_version("node", &root.tools["node"][0]),
            root_version
        );
        assert_eq!(
            root.entry_lockfile_version("node", &root.tools["node"][1]),
            other_version
        );
    }
}

#[test]
fn set_aube_lock_matches_backend() {
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "cli".to_string(),
        vec![
            basic_tool("1.0.0", "github:owner/cli"),
            basic_tool("1.0.0", "npm:cli"),
        ],
    );
    let graph = AubeLock::from_yaml("lockfileVersion: '9.0'\npackages: {}\n").unwrap();

    lockfile
        .set_aube_lock("cli", "1.0.0", "npm:cli", &BTreeMap::new(), graph.into())
        .unwrap();

    assert!(lockfile.tools["cli"][0].aube.is_none());
    assert!(lockfile.tools["cli"][1].aube.is_some());
}

#[test]
fn test_read_lockfile_at_only_promotes_when_primary_exists() {
    let temp = tempfile::tempdir().unwrap();
    let primary_path = temp.path().join("mise.lock");
    let legacy_path = temp.path().join("package.lock");

    let mut legacy = Lockfile {
        lockfile_version: 0,
        ..Default::default()
    };
    legacy
        .tools
        .insert("node".to_string(), vec![basic_tool("20.0.0", "core:node")]);
    legacy.save(&legacy_path).unwrap();

    let legacy_only = read_lockfile_at(primary_path.clone(), Some(legacy_path.clone()));
    assert_eq!(legacy_only.lockfile_version(), 0);

    let mut primary = Lockfile::default();
    primary.tools.insert(
        "node".to_string(),
        vec![LockfileTool {
            version: "22.0.0".to_string(),
            backend: Some("core:node".to_string()),
            specifiers: BTreeSet::from(["latest".to_string()]),
            options: BTreeMap::new(),
            platforms: BTreeMap::new(),
            aube: None,
            uv: None,
        }],
    );
    primary.save(&primary_path).unwrap();
    invalidate_caches();

    let mixed = read_lockfile_at(primary_path, Some(legacy_path));
    assert_eq!(mixed.lockfile_version(), CURRENT_LOCKFILE_VERSION);
    assert!(mixed.uses_request_bindings());
    assert!(
        mixed.tools["node"]
            .iter()
            .any(|tool| { tool.version == "22.0.0" && tool.specifiers.contains("latest") })
    );
    assert!(
        mixed.tools["node"]
            .iter()
            .any(|tool| { tool.version == "20.0.0" && tool.specifiers.is_empty() })
    );
}

#[test]
fn test_conda_packages_parsing() {
    let toml_with_conda = r#"
[conda-packages."macos-arm64"]
"ncurses-6.4-h7ea286d_0" = { url = "https://conda.anaconda.org/conda-forge/osx-arm64/ncurses-6.4-h7ea286d_0.conda", checksum = "sha256:abc123" }
"readline-8.2-h92ec313_1" = { url = "https://conda.anaconda.org/conda-forge/osx-arm64/readline-8.2-h92ec313_1.conda" }

[[tools.jq]]
version = "1.7.1"
backend = "conda:jq"
"platforms.macos-arm64" = { url = "https://example.com/jq.conda", checksum = "sha256:def456", conda_deps = ["ncurses-6.4-h7ea286d_0", "readline-8.2-h92ec313_1"] }
"#;

    let temp_dir = std::env::temp_dir();
    let test_lockfile = temp_dir.join("test_conda_parse.lock");
    std::fs::write(&test_lockfile, toml_with_conda).unwrap();

    let lockfile = Lockfile::read(&test_lockfile).unwrap();

    // Verify conda packages were parsed
    let macos_packages = lockfile.conda_packages.get("macos-arm64").unwrap();
    assert_eq!(macos_packages.len(), 2);

    let ncurses = macos_packages.get("ncurses-6.4-h7ea286d_0").unwrap();
    assert_eq!(
        ncurses.url,
        "https://conda.anaconda.org/conda-forge/osx-arm64/ncurses-6.4-h7ea286d_0.conda"
    );
    assert_eq!(ncurses.checksum, Some("sha256:abc123".to_string()));

    let readline = macos_packages.get("readline-8.2-h92ec313_1").unwrap();
    assert!(readline.checksum.is_none());

    // Verify tool with conda_deps was parsed
    let jq = &lockfile.tools["jq"][0];
    let platform_info = jq.platforms.get("macos-arm64").unwrap();
    assert_eq!(
        platform_info.conda_deps,
        Some(vec![
            "ncurses-6.4-h7ea286d_0".to_string(),
            "readline-8.2-h92ec313_1".to_string()
        ])
    );

    let _ = std::fs::remove_file(&test_lockfile);
}

/// Lockfiles written while the pkgx backend existed must still load, and
/// the next save drops what they recorded for it.
#[test]
fn test_legacy_pkgx_lockfile_sections_are_ignored() {
    let legacy = r#"
[[tools."pkgx:stedolan.github.io/jq"]]
version = "1.7.1"
backend = "pkgx:stedolan.github.io/jq"

[tools."pkgx:stedolan.github.io/jq".platforms.linux-x64]
url = "https://dist.pkgx.dev/stedolan.github.io/jq/linux/x86-64/v1.7.1.tar.xz"
checksum = "sha256:abc123"
pkgx_deps = ["github.com/kkos/oniguruma@6.9.9"]
pkgx_provides = ["bin/jq"]
pkgx_runtime_env = { JQ_HOME = "{{prefix}}" }

[[tools.jq]]
version = "1.7.1"
backend = "aqua:jqlang/jq"

[pkgx-packages.linux-x64."github.com/kkos/oniguruma@6.9.9"]
url = "https://dist.pkgx.dev/github.com/kkos/oniguruma/linux/x86-64/v6.9.9.tar.xz"
checksum = "sha256:def456"
pkgx_provides = ["bin/onig-config"]
"#;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    std::fs::write(&path, legacy).unwrap();

    let lockfile = Lockfile::read(&path).unwrap();
    let entry = &lockfile.tools["pkgx:stedolan.github.io/jq"][0];
    assert_eq!(entry.version, "1.7.1");
    let platform = &entry.platforms["linux-x64"];
    assert_eq!(platform.checksum.as_deref(), Some("sha256:abc123"));
    assert_eq!(lockfile.tools["jq"][0].version, "1.7.1");

    lockfile.save(&path).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("pkgx-packages"), "{saved}");
    assert!(!saved.contains("pkgx_"), "{saved}");
    assert!(saved.contains("sha256:abc123"), "{saved}");
}

#[test]
fn test_conda_packages_serialization() {
    let mut lockfile = Lockfile::default();

    // Add conda packages
    lockfile.set_conda_package(
        "macos-arm64",
        "ncurses-6.4-h7ea286d_0",
        CondaPackageInfo {
            url: "https://example.com/ncurses.conda".to_string(),
            checksum: Some("sha256:abc123".to_string()),
        },
    );

    // Add a tool with conda_deps
    let mut platforms = BTreeMap::new();
    platforms.insert(
        "macos-arm64".to_string(),
        PlatformInfo {
            url: Some("https://example.com/jq.conda".to_string()),
            checksum: Some("sha256:def456".to_string()),
            size: None,
            url_api: None,
            conda_deps: Some(vec!["ncurses-6.4-h7ea286d_0".to_string()]),
            ..Default::default()
        },
    );
    lockfile.tools.insert(
        "jq".to_string(),
        vec![LockfileTool {
            version: "1.7.1".to_string(),
            backend: Some("conda:jq".to_string()),
            specifiers: BTreeSet::new(),
            options: BTreeMap::new(),
            platforms,
            aube: None,
            uv: None,
        }],
    );

    let temp_dir = std::env::temp_dir();
    let test_lockfile = temp_dir.join("test_conda_serialize.lock");

    lockfile.save(&test_lockfile).unwrap();
    let content = std::fs::read_to_string(&test_lockfile).unwrap();

    // Verify content contains conda-packages section (TOML formats it as nested tables)
    assert!(
        content.contains("conda-packages"),
        "content should contain conda-packages section: {content}"
    );
    assert!(
        content.contains("ncurses-6.4-h7ea286d_0"),
        "content should contain package name: {content}"
    );
    assert!(
        content.contains("https://example.com/ncurses.conda"),
        "content should contain URL: {content}"
    );

    // Verify tool section contains conda_deps
    assert!(
        content.contains("conda_deps"),
        "content should contain conda_deps: {content}"
    );

    // Round-trip test: read it back
    let reloaded = Lockfile::read(&test_lockfile).unwrap();
    let packages = reloaded.conda_packages.get("macos-arm64").unwrap();
    assert!(packages.contains_key("ncurses-6.4-h7ea286d_0"));

    let _ = std::fs::remove_file(&test_lockfile);
}

#[test]
fn test_cleanup_unreferenced_conda_packages() {
    let mut lockfile = Lockfile::default();

    // Add some conda packages
    lockfile.set_conda_package(
        "macos-arm64",
        "referenced-pkg",
        CondaPackageInfo {
            url: "https://example.com/referenced.conda".to_string(),
            checksum: Some("sha256:abc123".to_string()),
        },
    );
    lockfile.set_conda_package(
        "macos-arm64",
        "unreferenced-pkg",
        CondaPackageInfo {
            url: "https://example.com/unreferenced.conda".to_string(),
            checksum: Some("sha256:def456".to_string()),
        },
    );
    lockfile.set_conda_package(
        "linux-x64",
        "orphan-platform-pkg",
        CondaPackageInfo {
            url: "https://example.com/orphan.conda".to_string(),
            checksum: None,
        },
    );

    // Add a tool that only references one package
    let mut platforms = BTreeMap::new();
    platforms.insert(
        "macos-arm64".to_string(),
        PlatformInfo {
            url: Some("https://example.com/tool.conda".to_string()),
            checksum: None,
            size: None,
            url_api: None,
            conda_deps: Some(vec!["referenced-pkg".to_string()]),
            ..Default::default()
        },
    );
    lockfile.tools.insert(
        "mytool".to_string(),
        vec![LockfileTool {
            version: "1.0.0".to_string(),
            backend: Some("conda:mytool".to_string()),
            specifiers: BTreeSet::new(),
            options: BTreeMap::new(),
            platforms,
            aube: None,
            uv: None,
        }],
    );

    // Verify we have all packages before cleanup
    assert_eq!(lockfile.conda_packages.len(), 2);
    assert_eq!(lockfile.conda_packages["macos-arm64"].len(), 2);
    assert_eq!(lockfile.conda_packages["linux-x64"].len(), 1);

    // Run cleanup
    lockfile.cleanup_unreferenced_conda_packages();

    // Verify only referenced package remains
    assert_eq!(lockfile.conda_packages.len(), 1);
    assert!(lockfile.conda_packages.contains_key("macos-arm64"));
    assert!(!lockfile.conda_packages.contains_key("linux-x64"));
    assert_eq!(lockfile.conda_packages["macos-arm64"].len(), 1);
    assert!(lockfile.conda_packages["macos-arm64"].contains_key("referenced-pkg"));
    assert!(!lockfile.conda_packages["macos-arm64"].contains_key("unreferenced-pkg"));
}

#[test]
fn test_retain_tools_by_short_prunes_removed_tools() {
    let mut lockfile = Lockfile::default();
    lockfile
        .tools
        .insert("dummy".to_string(), vec![basic_tool("1.0.0", "asdf:dummy")]);
    lockfile
        .tools
        .insert("tiny".to_string(), vec![basic_tool("2.1.0", "asdf:tiny")]);

    let keep_shorts = BTreeSet::from(["tiny".to_string()]);
    lockfile.retain_tools_by_short_or_backend(&keep_shorts, &BTreeSet::new());

    assert!(!lockfile.tools.contains_key("dummy"));
    assert!(lockfile.tools.contains_key("tiny"));
}

#[test]
fn test_stale_tool_shorts_identifies_removed_tools() {
    let mut lockfile = Lockfile::default();
    lockfile
        .tools
        .insert("dummy".to_string(), vec![basic_tool("1.0.0", "asdf:dummy")]);
    lockfile
        .tools
        .insert("tiny".to_string(), vec![basic_tool("2.1.0", "asdf:tiny")]);

    let keep_shorts = BTreeSet::from(["tiny".to_string()]);
    let stale = lockfile.stale_tool_shorts(&keep_shorts, &BTreeSet::new());
    assert_eq!(stale, BTreeSet::from(["dummy".to_string()]));
}

#[test]
fn test_stale_tool_shorts_respects_backend_identifiers() {
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "jq".to_string(),
        vec![basic_tool("1.7.1", "aqua:jqlang/jq")],
    );

    let keep_backends = BTreeSet::from(["aqua:jqlang/jq".to_string()]);
    let stale = lockfile.stale_tool_shorts(&BTreeSet::new(), &keep_backends);
    assert!(stale.is_empty());
}

#[test]
fn test_retain_tools_by_short_cleans_unreferenced_conda_packages() {
    let mut lockfile = Lockfile::default();

    add_test_conda_package(&mut lockfile, "linux-x64", "keep-pkg");
    add_test_conda_package(&mut lockfile, "linux-x64", "drop-pkg");

    lockfile.tools.insert(
        "tiny".to_string(),
        vec![tool_with_conda_dep(
            "2.1.0",
            "conda:tiny",
            "linux-x64",
            "keep-pkg",
        )],
    );
    lockfile.tools.insert(
        "dummy".to_string(),
        vec![tool_with_conda_dep(
            "1.0.0",
            "conda:dummy",
            "linux-x64",
            "drop-pkg",
        )],
    );

    let keep_shorts = BTreeSet::from(["tiny".to_string()]);
    lockfile.retain_tools_by_short_or_backend(&keep_shorts, &BTreeSet::new());

    assert!(lockfile.tools.contains_key("tiny"));
    assert!(!lockfile.tools.contains_key("dummy"));

    let linux_packages = lockfile.conda_packages.get("linux-x64").unwrap();
    assert!(linux_packages.contains_key("keep-pkg"));
    assert!(!linux_packages.contains_key("drop-pkg"));
}

#[test]
fn test_retain_tools_by_short_or_backend_preserves_legacy_keyed_entries() {
    let mut lockfile = Lockfile::default();
    lockfile.tools.insert(
        "jq".to_string(),
        vec![LockfileTool {
            version: "1.7.1".to_string(),
            backend: Some("aqua:jqlang/jq".to_string()),
            specifiers: BTreeSet::new(),
            options: BTreeMap::new(),

            platforms: BTreeMap::new(),
            aube: None,
            uv: None,
        }],
    );

    let keep_shorts = BTreeSet::from(["aqua:jqlang/jq".to_string()]);
    let keep_backends = BTreeSet::from(["aqua:jqlang/jq".to_string()]);
    lockfile.retain_tools_by_short_or_backend(&keep_shorts, &keep_backends);

    assert!(lockfile.tools.contains_key("jq"));
}

#[test]
fn test_platform_info_merge_prefers_sha256() {
    // The sha256-vs-blake3 preference only matters when both checksums
    // describe the same artifact, so use a shared URL here.
    let url = Some("https://example.com/a".to_string());
    let sha256_info = PlatformInfo {
        checksum: Some("sha256:abc123".to_string()),
        url: url.clone(),
        ..Default::default()
    };
    let blake3_info = PlatformInfo {
        checksum: Some("blake3:def456".to_string()),
        url: url.clone(),
        ..Default::default()
    };

    // sha256 + blake3 -> sha256
    let merged = sha256_info.merge_with(&blake3_info);
    assert_eq!(merged.checksum, Some("sha256:abc123".to_string()));

    // blake3 + sha256 -> sha256
    let merged = blake3_info.merge_with(&sha256_info);
    assert_eq!(merged.checksum, Some("sha256:abc123".to_string()));

    // blake3 + blake3 -> self (first)
    let another_blake3 = PlatformInfo {
        checksum: Some("blake3:ghi789".to_string()),
        url: url.clone(),
        ..Default::default()
    };
    let merged = blake3_info.merge_with(&another_blake3);
    assert_eq!(merged.checksum, Some("blake3:def456".to_string()));

    // Preserves URL from other if self is missing
    let no_url = PlatformInfo {
        checksum: Some("sha256:abc123".to_string()),
        url: None,
        ..Default::default()
    };
    let merged = no_url.merge_with(&blake3_info);
    assert_eq!(merged.url, url);
}

#[test]
fn test_platform_info_merge_drops_stale_checksum_on_url_change() {
    // When URLs disagree, the other side's checksum/size/url_api describe
    // a different artifact and must not be carried over (e.g. node musl
    // URL bumped but old glibc checksum still on disk).
    let new_info = PlatformInfo {
        checksum: None,
        size: None,
        url: Some("https://example.com/v2.tar.gz".to_string()),
        url_api: None,
        ..Default::default()
    };
    let stale_info = PlatformInfo {
        checksum: Some("sha256:OLD".to_string()),
        size: Some(123),
        url: Some("https://example.com/v1.tar.gz".to_string()),
        url_api: Some("https://api.example.com/v1".to_string()),
        ..Default::default()
    };

    let merged = new_info.merge_with(&stale_info);
    assert_eq!(merged.url.as_deref(), Some("https://example.com/v2.tar.gz"));
    assert_eq!(merged.checksum, None);
    assert_eq!(merged.size, None);
    assert_eq!(merged.url_api, None);

    // Same artifact (matching URLs) preserves the missing fields.
    let same_url = PlatformInfo {
        url: Some("https://example.com/v1.tar.gz".to_string()),
        ..Default::default()
    };
    let merged = same_url.merge_with(&stale_info);
    assert_eq!(merged.checksum.as_deref(), Some("sha256:OLD"));
    assert_eq!(merged.size, Some(123));
}

#[test]
fn test_platform_info_merge_clears_stale_source_marker_on_url() {
    let new_info = PlatformInfo {
        checksum: Some("sha256:NEW".to_string()),
        url: Some("https://example.com/v2.tar.gz".to_string()),
        ..Default::default()
    };
    let stale_source = PlatformInfo {
        install: Some("source".to_string()),
        checksum: Some("sha256:OLD".to_string()),
        url: Some("https://example.com/source.tar.gz".to_string()),
        ..Default::default()
    };

    let merged = new_info.merge_with(&stale_source);
    assert_eq!(merged.install, None);
    assert_eq!(merged.url.as_deref(), Some("https://example.com/v2.tar.gz"));
    assert_eq!(merged.checksum.as_deref(), Some("sha256:NEW"));
}

#[test]
fn test_provenance_fields_roundtrip() {
    let info = PlatformInfo {
        checksum: Some("sha256:abc123".to_string()),
        url: Some("https://example.com/tool.tar.gz".to_string()),
        provenance: Some(ProvenanceType::Slsa {
            url: Some("https://example.com/tool.intoto.jsonl".to_string()),
        }),
        provenance_verified: Some(true),
        ..Default::default()
    };

    // Test toml roundtrip — SLSA serializes as a table with url inside
    let toml_val: toml::Value = info.clone().into();
    let table = toml_val.as_table().unwrap();
    let prov_table = table.get("provenance").unwrap().as_table().unwrap();
    let slsa_table = prov_table.get("slsa").unwrap().as_table().unwrap();
    assert_eq!(
        slsa_table.get("url").unwrap().as_str().unwrap(),
        "https://example.com/tool.intoto.jsonl"
    );
    assert_eq!(
        table.get("provenance_verified").and_then(|v| v.as_bool()),
        Some(true)
    );
    let parsed: PlatformInfo = toml_val.try_into().unwrap();
    assert!(parsed.provenance.as_ref().unwrap().is_slsa());
    assert_eq!(parsed.provenance_verified, Some(true));
    match &parsed.provenance {
        Some(ProvenanceType::Slsa { url }) => {
            assert_eq!(
                url.as_deref(),
                Some("https://example.com/tool.intoto.jsonl")
            );
        }
        _ => panic!("expected Slsa provenance"),
    }
}

#[test]
fn test_additional_artifacts_roundtrip() {
    let info = PlatformInfo {
        checksum: Some("sha256:primary".to_string()),
        url: Some("https://example.com/tool.tar.gz".to_string()),
        additional_artifacts: vec![
            ArtifactInfo {
                checksum: Some("sha256:rocm".to_string()),
                url: "https://example.com/tool-rocm.tar.gz".to_string(),
                provenance: Some(ProvenanceType::GithubAttestations),
                provenance_verified: Some(true),
                ..Default::default()
            },
            ArtifactInfo {
                checksum: Some("blake3:debug".to_string()),
                url: "https://example.com/tool-debug.zip".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let mut toml_val: toml::Value = info.clone().into();
    toml_val
        .as_table_mut()
        .unwrap()
        .get_mut("additional_artifacts")
        .unwrap()
        .as_array_mut()
        .unwrap()[0]
        .as_table_mut()
        .unwrap()
        .insert(
            "future_lockfile_field".to_string(),
            toml::Value::String("ignored by older mise versions".to_string()),
        );
    let parsed: PlatformInfo = toml_val.try_into().unwrap();

    assert_eq!(parsed, info);
    assert!(!parsed.is_empty());
}

#[test]
fn test_additional_artifacts_merge_integrity_for_same_urls() {
    let resolved = PlatformInfo {
        url: Some("https://example.com/tool.tar.gz".to_string()),
        additional_artifacts: vec![ArtifactInfo {
            url: "https://example.com/tool-rocm.tar.gz".to_string(),
            url_api: Some("https://api.example.com/rocm".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let installed = PlatformInfo {
        url: resolved.url.clone(),
        additional_artifacts: vec![ArtifactInfo {
            checksum: Some("blake3:rocm".to_string()),
            url: "https://example.com/tool-rocm.tar.gz".to_string(),
            provenance: Some(ProvenanceType::Slsa { url: None }),
            ..Default::default()
        }],
        ..Default::default()
    };

    let merged = resolved.merge_with(&installed);
    assert_eq!(
        merged.additional_artifacts,
        vec![ArtifactInfo {
            checksum: Some("blake3:rocm".to_string()),
            url: "https://example.com/tool-rocm.tar.gz".to_string(),
            url_api: Some("https://api.example.com/rocm".to_string()),
            provenance: Some(ProvenanceType::Slsa { url: None }),
            ..Default::default()
        }]
    );
}

#[test]
fn test_additional_artifacts_merge_preserves_longer_list() {
    let short = vec![ArtifactInfo {
        checksum: Some("sha256:base".to_string()),
        url: "https://example.com/base.tar.gz".to_string(),
        ..Default::default()
    }];
    let long = vec![
        ArtifactInfo {
            url: short[0].url.clone(),
            url_api: Some("https://api.example.com/base".to_string()),
            ..Default::default()
        },
        ArtifactInfo {
            checksum: Some("sha256:extra".to_string()),
            url: "https://example.com/extra.tar.gz".to_string(),
            ..Default::default()
        },
    ];
    let expected = vec![
        ArtifactInfo {
            checksum: short[0].checksum.clone(),
            url: short[0].url.clone(),
            url_api: long[0].url_api.clone(),
            ..Default::default()
        },
        long[1].clone(),
    ];

    assert_eq!(merge_additional_artifacts(&short, &long), expected);
    assert_eq!(merge_additional_artifacts(&long, &short), expected);
}

#[test]
fn test_provenance_legacy_provenance_url_compat() {
    // Old lockfile format: provenance = "slsa" + provenance_url = "..."
    // Must go through TryFrom<toml::Value> which handles the legacy field
    let mut table = toml::Table::new();
    table.insert("provenance".to_string(), "slsa".into());
    table.insert(
        "provenance_url".to_string(),
        "https://example.com/tool.intoto.jsonl".into(),
    );
    let parsed = PlatformInfo::try_from(toml::Value::Table(table)).unwrap();
    assert!(parsed.provenance.as_ref().unwrap().is_slsa());
    match &parsed.provenance {
        Some(ProvenanceType::Slsa { url }) => {
            assert_eq!(
                url.as_deref(),
                Some("https://example.com/tool.intoto.jsonl")
            );
        }
        _ => panic!("expected Slsa provenance"),
    }
}

#[test]
fn test_github_attestations_unavailable_legacy_parse_only() {
    let info = PlatformInfo {
        checksum: Some("sha256:abc123".to_string()),
        url: Some("https://example.com/tool.tar.gz".to_string()),
        github_attestations: Some(GithubAttestationsStatus::Unavailable),
        ..Default::default()
    };

    let toml_val: toml::Value = info.clone().into();
    let table = toml_val.as_table().unwrap();
    assert!(table.get("github_attestations").is_none());
    assert!(table.get("provenance").is_none());

    let mut legacy = table.clone();
    legacy.insert("github_attestations".to_string(), "unavailable".into());
    let parsed: PlatformInfo = toml::Value::Table(legacy).try_into().unwrap();
    assert_eq!(
        parsed.github_attestations,
        Some(GithubAttestationsStatus::Unavailable)
    );
    assert!(parsed.provenance.is_none());
    assert!(!parsed.has_checksum_and_provenance());
}

#[test]
fn test_github_attestations_unavailable_ignored_with_provenance() {
    let mut table = toml::Table::new();
    table.insert("checksum".to_string(), "sha256:abc123".into());
    table.insert("provenance".to_string(), "slsa".into());
    table.insert("github_attestations".to_string(), "unavailable".into());

    let parsed: PlatformInfo = toml::Value::Table(table.clone()).try_into().unwrap();
    assert!(parsed.provenance.as_ref().unwrap().is_slsa());
    assert!(parsed.github_attestations.is_none());
    assert!(parsed.has_checksum_and_provenance());

    table.insert("provenance_verified".to_string(), true.into());
    let parsed: PlatformInfo = toml::Value::Table(table).try_into().unwrap();
    assert!(parsed.has_checksum_and_provenance());

    let serialized: toml::Value = PlatformInfo {
        checksum: Some("sha256:abc123".to_string()),
        provenance: Some(ProvenanceType::GithubAttestations),
        provenance_verified: Some(true),
        github_attestations: Some(GithubAttestationsStatus::Unavailable),
        ..Default::default()
    }
    .into();
    let table = serialized.as_table().unwrap();
    assert!(table.get("provenance").is_some());
    assert_eq!(
        table.get("provenance_verified").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(table.get("github_attestations").is_none());
}

#[test]
fn test_recorded_provenance_trust_ignores_legacy_bit() {
    let detected = PlatformInfo {
        checksum: Some("sha256:detected".to_string()),
        provenance: Some(ProvenanceType::GithubAttestations),
        ..Default::default()
    };
    assert!(detected.has_checksum_and_provenance());

    let verified = PlatformInfo {
        provenance_verified: Some(false),
        ..detected.clone()
    };
    assert!(verified.has_checksum_and_provenance());

    let detected_artifact = ArtifactInfo {
        checksum: detected.checksum,
        url: "https://example.com/supplemental.tar.gz".to_string(),
        provenance: detected.provenance,
        ..Default::default()
    };
    assert!(detected_artifact.has_checksum_and_provenance());

    let verified_artifact = ArtifactInfo {
        provenance_verified: Some(false),
        ..detected_artifact
    };
    assert!(verified_artifact.has_checksum_and_provenance());
}

#[test]
fn test_merge_provenance_state_prefers_verified() {
    let verified = Some(ProvenanceType::GithubAttestations);
    let detected = Some(ProvenanceType::Slsa { url: None });

    assert_eq!(
        merge_provenance_state(verified.clone(), Some(true), detected.clone(), None),
        (verified.clone(), Some(true))
    );
    assert_eq!(
        merge_provenance_state(detected, None, verified.clone(), Some(true)),
        (verified, Some(true))
    );
}

#[test]
fn test_provenance_merge_preserves_existing() {
    let with_provenance = PlatformInfo {
        provenance: Some(ProvenanceType::GithubAttestations),
        ..Default::default()
    };
    let without = PlatformInfo::default();

    // Merging with empty preserves provenance
    let merged = with_provenance.merge_with(&without);
    assert_eq!(merged.provenance, Some(ProvenanceType::GithubAttestations));

    // Merging empty (new) with provenance (old) preserves existing provenance
    let merged = without.merge_with(&with_provenance);
    assert_eq!(merged.provenance, Some(ProvenanceType::GithubAttestations));

    // Merging Slsa { url: None } with Slsa { url: Some(...) } preserves URL
    let with_url = PlatformInfo {
        provenance: Some(ProvenanceType::Slsa {
            url: Some("https://example.com/provenance.intoto.jsonl".to_string()),
        }),
        ..Default::default()
    };
    let without_url = PlatformInfo {
        provenance: Some(ProvenanceType::Slsa { url: None }),
        ..Default::default()
    };
    let merged = without_url.merge_with(&with_url);
    assert!(merged.provenance.as_ref().unwrap().is_slsa());
    match &merged.provenance {
        Some(ProvenanceType::Slsa { url }) => {
            assert_eq!(
                url.as_deref(),
                Some("https://example.com/provenance.intoto.jsonl")
            );
        }
        _ => panic!("expected Slsa provenance"),
    }
    // Also in reverse order
    let merged = with_url.merge_with(&without_url);
    match &merged.provenance {
        Some(ProvenanceType::Slsa { url }) => {
            assert_eq!(
                url.as_deref(),
                Some("https://example.com/provenance.intoto.jsonl")
            );
        }
        _ => panic!("expected Slsa provenance"),
    }

    // A negative GitHub attestation cache entry must not override verified provenance.
    let github_unavailable = PlatformInfo {
        github_attestations: Some(GithubAttestationsStatus::Unavailable),
        ..Default::default()
    };
    let merged = github_unavailable.merge_with(&with_url);
    assert!(merged.provenance.as_ref().unwrap().is_slsa());
    assert!(merged.github_attestations.is_none());
    let merged = with_url.merge_with(&github_unavailable);
    assert!(merged.provenance.as_ref().unwrap().is_slsa());
    assert!(merged.github_attestations.is_none());

    // Merging with empty drops the legacy negative GitHub attestation cache.
    let merged = github_unavailable.merge_with(&PlatformInfo::default());
    assert!(merged.github_attestations.is_none());
}

#[test]
fn test_provenance_not_empty() {
    let info = PlatformInfo {
        provenance: Some(ProvenanceType::Slsa { url: None }),
        ..Default::default()
    };
    assert!(!info.is_empty());

    let info = PlatformInfo {
        github_attestations: Some(GithubAttestationsStatus::Unavailable),
        ..Default::default()
    };
    assert!(info.is_empty());
}

#[test]
fn test_github_attestations_unavailable_counts_as_missing_for_regression() {
    let platform = Platform::current().to_key();
    let mut prior = basic_tool("1.0.0", "github:owner/repo");
    prior.platforms.insert(
        platform.clone(),
        PlatformInfo {
            provenance: Some(ProvenanceType::GithubAttestations),
            ..Default::default()
        },
    );
    let existing = vec![prior];

    let err = check_single_tool_provenance(
        Some(&existing),
        "tool",
        "1.1.0",
        "github:owner/repo",
        &platform,
        None,
    )
    .unwrap();
    assert!(err.contains("has no provenance verification"));

    let mut prior = basic_tool("1.0.0", "github:owner/repo");
    prior.platforms.insert(
        platform.clone(),
        PlatformInfo {
            github_attestations: Some(GithubAttestationsStatus::Unavailable),
            ..Default::default()
        },
    );
    let existing = vec![prior];
    assert!(
        check_single_tool_provenance(
            Some(&existing),
            "tool",
            "1.1.0",
            "github:owner/repo",
            &platform,
            None,
        )
        .is_none()
    );
}

#[test]
fn test_provenance_regression_skips_unverified_already_installed_version() {
    // Reproduces #11225: upgrading a locked tool to a newer version that is
    // *already installed* on disk skips the download, so the new entry has no
    // platform info (no url, no provenance). That missing provenance means
    // "not checked", not "verified absent", and must NOT be flagged as a
    // supply-chain regression.
    let platform = Platform::current().to_key();

    let mut prior = basic_tool("0.11.0", "github:owner/repo");
    prior.platforms.insert(
        platform.clone(),
        PlatformInfo {
            url: Some("https://example.com/repo-0.11.0.tar.gz".to_string()),
            provenance: Some(ProvenanceType::GithubAttestations),
            ..Default::default()
        },
    );
    let mut existing = Lockfile::default();
    existing.tools.insert("repo".to_string(), vec![prior]);

    // New version with an empty platform map (nothing downloaded/verified).
    let new_tools: HashMap<String, Vec<LockfileTool>> = [(
        "repo".to_string(),
        vec![basic_tool("0.12.0", "github:owner/repo")],
    )]
    .into_iter()
    .collect();

    let check = check_provenance_regression(&existing, &new_tools);
    assert!(
        check.regressing.is_empty() && check.errors.is_empty(),
        "already-installed upgrade with no fresh verification must not regress: {:?}",
        check.errors
    );
    // ...but the prior provenance-bearing entry must be preserved as a baseline so
    // the auto-lock pass can still compare against it after the merge.
    let baselines = check
        .deferred_baselines
        .get("repo")
        .expect("baseline must be preserved for the unverified upgrade");
    assert!(
        baselines.iter().any(|t| t.version == "0.11.0"),
        "expected 0.11.0 baseline to be preserved, got {baselines:?}"
    );

    // The platform can also be present but still unverified when installation
    // populated an empty PlatformInfo. The URL predicate must defer this case too.
    let mut present_unverified = basic_tool("0.12.0", "github:owner/repo");
    present_unverified
        .platforms
        .insert(platform.clone(), PlatformInfo::default());
    let new_tools = HashMap::from([("repo".to_string(), vec![present_unverified])]);
    let check = check_provenance_regression(&existing, &new_tools);
    assert!(check.regressing.is_empty() && check.errors.is_empty());
    assert!(
        check
            .deferred_baselines
            .get("repo")
            .is_some_and(|baselines| baselines.iter().any(|t| t.version == "0.11.0")),
        "present-but-unverified platform must preserve the provenance baseline"
    );

    // Contrast: a freshly-installed version populates a url but genuinely has
    // no provenance — that is a real regression and must still be flagged.
    let mut fresh = basic_tool("0.12.0", "github:owner/repo");
    fresh.platforms.insert(
        platform.clone(),
        PlatformInfo {
            url: Some("https://example.com/repo-0.12.0.tar.gz".to_string()),
            provenance: None,
            ..Default::default()
        },
    );
    let new_tools: HashMap<String, Vec<LockfileTool>> =
        [("repo".to_string(), vec![fresh])].into_iter().collect();
    let check = check_provenance_regression(&existing, &new_tools);
    assert!(
        check.regressing.contains("repo") && !check.errors.is_empty(),
        "freshly-installed version that lost provenance must still regress"
    );
    assert!(
        check.deferred_baselines.is_empty(),
        "confirmed regression must not be deferred"
    );
}

#[test]
fn test_deferred_resolution_failure_blocks_when_baseline_present() {
    // Models the auto-lock escalation gate (#11225): when an already-installed
    // upgrade could not be resolved/verified, a failure must fail-closed only if a
    // prior provenance-bearing version is still present as a baseline; otherwise it
    // stays best-effort. This tests the decision `find_provenance_regression_baseline`
    // drives in `auto_lock_new_versions`.
    let platform = Platform::current().to_key();

    let mut baseline = basic_tool("0.11.0", "github:owner/repo");
    baseline.platforms.insert(
        platform.clone(),
        PlatformInfo {
            url: Some("https://example.com/repo-0.11.0.tar.gz".to_string()),
            provenance: Some(ProvenanceType::GithubAttestations),
            ..Default::default()
        },
    );
    // The deferred, still-unverified new version (no url/provenance yet).
    let pending = basic_tool("0.12.0", "github:owner/repo");
    let tools = vec![baseline, pending];

    // A resolution failure for 0.12.0 must become the caller's blocking error.
    let err = deferred_provenance_resolution_error(
        Some(&tools),
        "repo",
        "0.12.0",
        "github:owner/repo",
        &platform,
        "network unavailable",
    )
    .expect("failure must fail-closed while the provenance baseline is present");
    assert!(err.contains("could not verify provenance for repo@0.12.0"));
    assert!(err.contains("network unavailable"));

    // Without a provenance-bearing prior version, a failure stays best-effort.
    let no_baseline = vec![
        basic_tool("0.11.0", "github:owner/repo"),
        basic_tool("0.12.0", "github:owner/repo"),
    ];
    assert!(
        deferred_provenance_resolution_error(
            Some(&no_baseline),
            "repo",
            "0.12.0",
            "github:owner/repo",
            &platform,
            "network unavailable",
        )
        .is_none(),
        "no baseline means a resolution failure should remain non-blocking"
    );
}

#[test]
fn test_provenance_baseline_requires_exact_github_backend() {
    let platform = Platform::current().to_key();
    let mut prior = basic_tool("1.0.0", "github:owner/original");
    prior.platforms.insert(
        platform.clone(),
        PlatformInfo {
            provenance: Some(ProvenanceType::GithubAttestations),
            ..Default::default()
        },
    );
    let existing = vec![prior];

    assert!(
        check_single_tool_provenance(
            Some(&existing),
            "tool",
            "2.0.0",
            "github:owner/fork",
            &platform,
            None,
        )
        .is_none(),
        "an unrelated github repository must not supply the provenance baseline"
    );
}

#[test]
fn test_provenance_regression_only_flags_known_semver_upgrades() {
    let platform = Platform::current().to_key();
    let mut prior = basic_tool("nightly", "github:owner/repo");
    prior.platforms.insert(
        platform.clone(),
        PlatformInfo {
            provenance: Some(ProvenanceType::GithubAttestations),
            ..Default::default()
        },
    );
    let existing = vec![prior];

    assert!(
        check_single_tool_provenance(
            Some(&existing),
            "tool",
            "v1.0.0",
            "github:owner/repo",
            &platform,
            None,
        )
        .is_none()
    );

    let mut prior = basic_tool("v1.0.0-rc.1", "github:owner/repo");
    prior.platforms.insert(
        platform.clone(),
        PlatformInfo {
            provenance: Some(ProvenanceType::GithubAttestations),
            ..Default::default()
        },
    );
    let existing = vec![prior];

    let err = check_single_tool_provenance(
        Some(&existing),
        "tool",
        "v1.0.0",
        "github:owner/repo",
        &platform,
        None,
    )
    .unwrap();
    assert!(err.contains("has no provenance verification"));
}

#[test]
fn test_parse_provenance_version_strips_single_semver_v_prefix() {
    assert!(parse_provenance_version("v1.2.3").is_some());
    assert!(parse_provenance_version("V1.2.3").is_some());
    assert!(parse_provenance_version("vv1.2.3").is_none());
    assert!(parse_provenance_version("v").is_none());
}

#[test]
fn test_set_platform_info_all_platforms_get_slsa_url() {
    // Regression guard: when all platforms have Slsa { url: Some(...) },
    // the lockfile should serialize ALL entries with the expanded form.
    let mut lockfile = Lockfile::default();
    let platforms = vec!["linux-x64", "linux-arm64", "macos-x64", "macos-arm64"];
    for platform in &platforms {
        lockfile.set_platform_info(
            "sops",
            "3.12.1",
            Some("aqua:getsops/sops"),
            &BTreeMap::new(),
            platform,
            PlatformInfo {
                checksum: Some("sha256:abc123".to_string()),
                url: Some(format!("https://example.com/sops-{platform}.tar.gz")),
                provenance: Some(ProvenanceType::Slsa {
                    url: Some(format!("https://example.com/sops-{platform}.intoto.jsonl")),
                }),
                ..Default::default()
            },
        );
    }
    let temp_dir = std::env::temp_dir();
    let test_lockfile = temp_dir.join("test_provenance_all_platforms.lock");
    lockfile.save(&test_lockfile).unwrap();
    let serialized = std::fs::read_to_string(&test_lockfile).unwrap();
    let _ = std::fs::remove_file(&test_lockfile);
    // ALL platform entries should have the expanded provenance.slsa form
    for platform in &platforms {
        assert!(
            serialized.contains(&format!("\"platforms.{platform}\".provenance.slsa")),
            "platform {platform} should have expanded provenance.slsa form, got:\n{serialized}"
        );
    }
    // No short-form provenance should appear
    assert!(
        !serialized.contains("provenance = \"slsa\""),
        "no short-form provenance should appear, got:\n{serialized}"
    );
}

#[test]
fn test_set_platform_info_none_provenance_preserves_existing_url() {
    // When new PlatformInfo has provenance=None, existing Slsa URL should be preserved
    let mut lockfile = Lockfile::default();
    // First: set platform info with Slsa URL
    lockfile.set_platform_info(
        "sops",
        "3.12.1",
        Some("aqua:getsops/sops"),
        &BTreeMap::new(),
        "linux-x64",
        PlatformInfo {
            checksum: Some("sha256:abc123".to_string()),
            url: Some("https://example.com/sops.tar.gz".to_string()),
            provenance: Some(ProvenanceType::Slsa {
                url: Some("https://example.com/sops.intoto.jsonl".to_string()),
            }),
            ..Default::default()
        },
    );
    // Second: set same platform with provenance=None (simulates verification failure)
    lockfile.set_platform_info(
        "sops",
        "3.12.1",
        Some("aqua:getsops/sops"),
        &BTreeMap::new(),
        "linux-x64",
        PlatformInfo {
            checksum: Some("sha256:abc123".to_string()),
            url: Some("https://example.com/sops.tar.gz".to_string()),
            provenance: None,
            ..Default::default()
        },
    );
    // The existing Slsa URL should be preserved by merge
    let tool = &lockfile.tools["sops"][0];
    let info = &tool.platforms["linux-x64"];
    match &info.provenance {
        Some(ProvenanceType::Slsa { url }) => {
            assert_eq!(
                url.as_deref(),
                Some("https://example.com/sops.intoto.jsonl")
            );
        }
        other => panic!("expected Slsa provenance with URL, got: {other:?}"),
    }
}

#[test]
fn test_set_platform_info_replaces_artifact_with_source_outcome() {
    let mut lockfile = Lockfile::default();
    lockfile.set_platform_info(
        "node",
        "24.0.0",
        Some("core:node"),
        &BTreeMap::new(),
        "linux-x64",
        PlatformInfo {
            checksum: Some("sha256:OLD".to_string()),
            size: Some(123),
            url: Some("https://example.com/binary.tar.gz".to_string()),
            url_api: Some("https://api.example.com/binary".to_string()),
            ..Default::default()
        },
    );
    lockfile.set_platform_info(
        "node",
        "24.0.0",
        Some("core:node"),
        &BTreeMap::new(),
        "linux-x64",
        PlatformInfo {
            install: Some("source".to_string()),
            checksum: Some("sha256:NEW".to_string()),
            url: Some("https://example.com/source.tar.gz".to_string()),
            ..Default::default()
        },
    );

    let info = &lockfile.tools["node"][0].platforms["linux-x64"];
    assert_eq!(info.install.as_deref(), Some("source"));
    assert_eq!(info.checksum.as_deref(), Some("sha256:NEW"));
    assert_eq!(info.size, None);
    assert_eq!(
        info.url.as_deref(),
        Some("https://example.com/source.tar.gz")
    );
    assert_eq!(info.url_api, None);
}

#[test]
fn test_set_platform_info_replaces_source_outcome_with_artifact() {
    let mut lockfile = Lockfile::default();
    lockfile.set_platform_info(
        "node",
        "24.0.0",
        Some("core:node"),
        &BTreeMap::new(),
        "linux-x64",
        PlatformInfo {
            install: Some("source".to_string()),
            checksum: Some("sha256:OLD".to_string()),
            url: Some("https://example.com/source.tar.gz".to_string()),
            ..Default::default()
        },
    );
    lockfile.set_platform_info(
        "node",
        "24.0.0",
        Some("core:node"),
        &BTreeMap::new(),
        "linux-x64",
        PlatformInfo {
            checksum: Some("sha256:NEW".to_string()),
            url: Some("https://example.com/binary.tar.gz".to_string()),
            ..Default::default()
        },
    );

    let info = &lockfile.tools["node"][0].platforms["linux-x64"];
    assert_eq!(info.install, None);
    assert_eq!(info.checksum.as_deref(), Some("sha256:NEW"));
    assert_eq!(
        info.url.as_deref(),
        Some("https://example.com/binary.tar.gz")
    );
}

#[test]
fn test_set_platform_info_keeps_platform_specific_option_variants_separate() {
    let mut lockfile = Lockfile::default();
    lockfile.set_platform_info(
        "ruby",
        "3.4.2",
        Some("core:ruby"),
        &BTreeMap::new(),
        "windows-x64",
        PlatformInfo {
            url: Some("https://example.com/ruby-windows.7z".to_string()),
            ..Default::default()
        },
    );
    let unix_options = BTreeMap::from([("compile".to_string(), "false".to_string())]);
    lockfile.set_platform_info(
        "ruby",
        "3.4.2",
        Some("core:ruby"),
        &unix_options,
        "linux-x64",
        PlatformInfo {
            url: Some("https://example.com/ruby-linux.tar.gz".to_string()),
            ..Default::default()
        },
    );

    let tools = &lockfile.tools["ruby"];
    assert_eq!(tools.len(), 2, "option variants were conflated: {tools:?}");
    assert!(
        tools
            .iter()
            .any(|tool| { tool.options.is_empty() && tool.platforms.contains_key("windows-x64") })
    );
    assert!(
        tools.iter().any(|tool| {
            tool.options == unix_options && tool.platforms.contains_key("linux-x64")
        })
    );
}

#[test]
fn test_set_platform_info_keeps_same_platform_sibling_variants() {
    let mut lockfile = Lockfile::default();
    let empty_options = BTreeMap::new();
    let options_a = BTreeMap::from([("flavor".to_string(), "a".to_string())]);
    let options_b = BTreeMap::from([("flavor".to_string(), "b".to_string())]);
    for options in [&empty_options, &options_a, &options_b] {
        lockfile.set_platform_info(
            "example",
            "1.0.0",
            Some("example:tool"),
            options,
            "linux-x64",
            PlatformInfo {
                checksum: Some(format!(
                    "sha256:{}",
                    options.get("flavor").map(String::as_str).unwrap_or("empty")
                )),
                ..Default::default()
            },
        );
    }

    let tools = &lockfile.tools["example"];
    assert_eq!(tools.len(), 3);
    assert!(
        tools
            .iter()
            .all(|tool| tool.platforms.contains_key("linux-x64"))
    );
}

#[test]
fn test_source_platform_info_serializes_as_lock_marker() {
    let mut lockfile = Lockfile::default();
    lockfile.set_platform_info(
        "node",
        "24.0.0",
        Some("core:node"),
        &BTreeMap::new(),
        "linux-riscv64",
        PlatformInfo {
            install: Some("source".to_string()),
            checksum: Some("sha256:SOURCE".to_string()),
            url: Some("https://example.com/node-v24.0.0.tar.gz".to_string()),
            ..Default::default()
        },
    );

    let temp_dir = tempfile::tempdir().unwrap();
    let test_lockfile = temp_dir.path().join("source-platform-marker.lock");
    lockfile.write(&test_lockfile).unwrap();
    let serialized = file::read_to_string(&test_lockfile).unwrap();

    assert!(serialized.contains("platforms.linux-riscv64"));
    assert!(serialized.contains("install = \"source\""));
    assert!(serialized.contains("checksum = \"sha256:SOURCE\""));
    assert!(serialized.contains("url = \"https://example.com/node-v24.0.0.tar.gz\""));
    assert!(!serialized.contains("node-v24.0.0-linux-riscv64.tar.gz"));

    let reloaded = Lockfile::read(&test_lockfile).unwrap();
    let info = &reloaded.tools["node"][0].platforms["linux-riscv64"];
    assert_eq!(info.install.as_deref(), Some("source"));
    assert_eq!(info.checksum.as_deref(), Some("sha256:SOURCE"));
    assert_eq!(
        info.url.as_deref(),
        Some("https://example.com/node-v24.0.0.tar.gz")
    );
}

#[test]
fn test_determine_target_platforms_respects_pre_install_keys() {
    // Regression: auto-lock must not silently re-add platforms (e.g. macos-x64) that
    // a user has dropped. The pre-install snapshot is authoritative when non-empty.
    let pre: BTreeSet<String> = ["linux-arm64", "windows-x64"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let platforms = determine_target_platforms_from_lockfile(&pre).unwrap();
    let keys: BTreeSet<String> = platforms.iter().map(|p| p.to_key()).collect();

    assert!(keys.contains("linux-arm64"));
    assert!(keys.contains("windows-x64"));
    assert!(keys.contains(&Platform::current().to_key()));
    // macos-x64 / macos-arm64 / linux-x64-musl etc. must not leak in unless current.
    for extra in ["macos-x64", "macos-arm64"] {
        if extra != Platform::current().to_key() {
            assert!(
                !keys.contains(extra),
                "{extra} leaked into target platforms: {keys:?}"
            );
        }
    }
}

#[test]
fn test_determine_target_platforms_one_platform_lockfile_is_authoritative() {
    // A user can intentionally maintain a single-platform lockfile. As long as the
    // pre-install snapshot was non-empty, that intent must be respected — even if
    // the only entry happens to be the current platform.
    let current = Platform::current().to_key();
    let pre: BTreeSet<String> = std::iter::once(current.clone()).collect();

    let platforms = determine_target_platforms_from_lockfile(&pre).unwrap();
    let keys: BTreeSet<String> = platforms.iter().map(|p| p.to_key()).collect();

    assert_eq!(keys.len(), 1, "expected only {current}, got {keys:?}");
    assert!(keys.contains(&current));
}

#[test]
fn test_determine_target_platforms_expands_for_fresh_lockfile() {
    // A fresh lockfile (no entries before this install) must expand to the common
    // platform set so first-install gets cross-platform-locked.
    let pre: BTreeSet<String> = BTreeSet::new();

    let platforms = determine_target_platforms_from_lockfile(&pre).unwrap();
    let keys: BTreeSet<String> = platforms.iter().map(|p| p.to_key()).collect();

    for common in Platform::common_platforms() {
        assert!(
            keys.contains(&common.to_key()),
            "{} missing from fresh-lockfile target set: {keys:?}",
            common.to_key()
        );
    }
}
#[test]
fn python_lock_spelling_preserves_distinct_keys_and_revision() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mise.lock");
    file::write(
        &path,
        r#"
lockfile_version = 1
[[tools."pipx:black"]]
version = "24.10.0"
backend = "pipx:black"
[[tools.python]]
version = "3.12.11"
backend = "core:python"
"#,
    )
    .unwrap();
    let lock = Lockfile::read(&path).unwrap();
    assert_eq!(lock.lockfile_version(), 1);
    assert!(lock.tools.contains_key("pipx:black"));
    assert_eq!(
        lock.tools_for("pipx:black").unwrap()[0].backend.as_deref(),
        Some("pipx:black")
    );
    assert_eq!(
        lock.tools["python"][0].backend.as_deref(),
        Some("core:python")
    );
    assert!(lock.tools_for("pypi:black").is_none());
    assert!(lock.needs_upgrade());
    lock.save(&path).unwrap();
    let saved = file::read_to_string(&path).unwrap();
    assert!(saved.contains("tools.\"pipx:black\""));
    assert!(saved.contains("backend = \"pipx:black\""));
    assert!(!saved.contains("pypi:black"));
}
