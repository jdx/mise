use super::*;

fn portable_for(values: &[&str]) -> Portable {
    Portable {
        mappings: Vec::new(),
        arguments: Vec::new(),
        names: values.iter().map(|_| "OUT_DIR".to_string()).collect(),
        values: values.iter().map(|value| (*value).to_string()).collect(),
    }
}

#[test]
fn compiler_timing_survives_a_changed_action_key() {
    let invocation = CacheDigest::blake3(b"invocation");
    let timing = RustcInputPrediction {
        version: 3,
        inputs: Vec::new(),
        environment: Vec::new(),
        compiler_duration_ns: 42,
        crate_name: "demo".into(),
    };
    let prediction = ActionPrediction {
        invocation: invocation.clone(),
        action: CacheDigest::blake3(b"old action"),
        adapter: "rustc".into(),
        payload: String::from_utf8(canonical_json(&timing).unwrap()).unwrap(),
    };

    let decoded = decode_prediction_timing(&prediction, &invocation).unwrap();
    assert_eq!(decoded.crate_name, "demo");
    assert_eq!(decoded.duration_ns, 42);
}

/// `--remap-path-prefix` covers the paths rustc writes itself, so most
/// artifacts come out clean. A crate that keeps the value as a string does
/// not, and that is the case the outputs are read to catch.
#[test]
fn an_output_carrying_a_normalized_value_is_not_portable() {
    let root = tempfile::tempdir().unwrap();
    let out_dir = "/checkout/target/debug/build/widget-abc/out";
    let clean = root.path().join("clean.rlib");
    std::fs::write(
        &clean,
        b"rustc output naming ${target}/debug/build/widget-abc/out",
    )
    .unwrap();
    let carries = root.path().join("carries.rlib");
    std::fs::write(&carries, format!("compiled in {out_dir} at some offset")).unwrap();

    let portable = portable_for(&[out_dir]);
    assert!(
        portable
            .outputs_are_clean(std::slice::from_ref(&clean))
            .unwrap()
    );
    assert!(
        !portable
            .outputs_are_clean(std::slice::from_ref(&carries))
            .unwrap()
    );
    // One dirty output is enough: the artifact is published as a set.
    assert!(!portable.outputs_are_clean(&[clean, carries]).unwrap());
}

/// Nothing was made portable, so there is no portable key to publish under
/// and no claim to check.
#[test]
fn nothing_portable_is_never_clean() {
    assert!(!portable_for(&[]).outputs_are_clean(&[]).unwrap());
}

#[test]
fn a_value_is_found_at_any_offset_and_in_either_spelling() {
    assert!(carries(b"/a/b", "/a/b"));
    assert!(carries(b"...../a/b.....", "/a/b"));
    assert!(carries(b"/a/a/b", "/a/b"));
    assert!(!carries(b"/a/", "/a/b"));
    assert!(!carries(b"", "/a/b"));
    // A Windows value may have been written with forward slashes.
    assert!(carries(b"c:/a/b", "c:\\a\\b"));
    assert!(!carries(b"c:/a/c", "c:\\a\\b"));
}

fn staged_outputs(root: &Path, entries: Vec<(&[u8], PathBuf)>) -> StagedOutputs {
    let directory = tempfile::tempdir_in(root).unwrap();
    let files = entries
        .into_iter()
        .enumerate()
        .map(|(index, (contents, destination))| {
            let path = directory.path().join(format!("output-{index}"));
            std::fs::write(&path, contents).unwrap();
            (
                tempfile::TempPath::try_from_path(path).unwrap(),
                destination,
            )
        })
        .collect();
    StagedOutputs { directory, files }
}

fn test_outputs(root: &Path) -> RustcOutputs {
    let directory = root.join("out");
    RustcOutputs {
        files: vec![directory.join("libdemo.rlib")],
        dep_info: directory.join("demo.d"),
        directory,
    }
}

fn test_file(name: &str) -> CacheFileNode {
    CacheFileNode {
        digest: CacheDigest::blake3(b"artifact"),
        executable: false,
        mode: if cfg!(unix) { 0o644 } else { 0 },
        name: name.into(),
    }
}

fn test_directory(files: Vec<CacheFileNode>) -> CacheDirectory {
    CacheDirectory {
        directories: Vec::new(),
        files,
        symlinks: Vec::new(),
        version: 1,
    }
}

fn test_output_directory(file: CacheFileNode) -> CacheDirectory {
    test_directory(vec![file, test_file("demo.d")])
}

#[test]
fn parses_verbose_rustc_identity() {
    let verbose = "rustc 1.97.0 (abc 2026-08-01)\n\
                       binary: rustc\n\
                       commit-hash: abc\n\
                       commit-date: 2026-08-01\n\
                       host: x86_64-unknown-linux-gnu\n\
                       release: 1.97.0\n\
                       LLVM version: 22.0.0\n";
    assert_eq!(identity_field(verbose, "release").unwrap(), "1.97.0");
    assert_eq!(
        identity_field(verbose, "host").unwrap(),
        "x86_64-unknown-linux-gnu"
    );
}

#[test]
fn mappings_do_not_duplicate_home_placeholders() {
    let directory = tempfile::tempdir().unwrap();
    let mappings = path_mappings(directory.path(), None, None);
    let placeholders = mappings
        .iter()
        .map(|mapping| &mapping.placeholder)
        .collect::<BTreeSet<_>>();
    assert_eq!(placeholders.len(), mappings.len());
}

#[test]
fn standalone_workspace_mapping_wins_beneath_home() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let workspace = home.join("src/project");
    let mappings = path_mappings_with_env(&workspace, None, None, |name| match name {
        "HOME" => Some(home.as_os_str().to_owned()),
        _ => None,
    });

    assert!(
        mappings
            .iter()
            .any(|mapping| { mapping.placeholder == "workspace" && mapping.root == workspace })
    );
    assert!(
        mappings
            .iter()
            .any(|mapping| mapping.placeholder == "home" && mapping.root == home)
    );
}

#[test]
fn standalone_workspace_mapping_uses_the_outer_workspace_for_members() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    let member = workspace.join("crates/widget");
    std::fs::create_dir_all(&member).unwrap();
    std::fs::write(workspace.join("Cargo.lock"), "").unwrap();
    std::fs::write(member.join("Cargo.toml"), "[package]\nname = \"widget\"\n").unwrap();

    let mappings = path_mappings_with_env(&member, None, None, |_| None);

    assert!(
        mappings
            .iter()
            .any(|mapping| { mapping.placeholder == "workspace" && mapping.root == workspace })
    );
}

#[test]
fn standalone_registry_mapping_uses_the_default_cargo_home() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let dependency = home.join(".cargo/registry/src/index/widget-1.0.0");
    std::fs::create_dir_all(&dependency).unwrap();
    std::fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname = \"widget\"\n",
    )
    .unwrap();

    let mappings = path_mappings_with_env(&dependency, None, None, |name| match name {
        "HOME" => Some(home.as_os_str().to_owned()),
        _ => None,
    });

    assert!(mappings.iter().any(|mapping| {
        mapping.placeholder == "cargo_home" && mapping.root == home.join(".cargo")
    }));
    assert!(
        !mappings
            .iter()
            .any(|mapping| mapping.placeholder == "workspace")
    );
}

#[test]
fn standalone_target_mapping_covers_the_profile_tree() {
    assert_eq!(
        standalone_target_root(Path::new("/tmp/target/debug/deps"), None),
        Path::new("/tmp/target")
    );
    assert_eq!(
        standalone_target_root(
            Path::new("/tmp/target/x86_64-unknown-linux-gnu/release/deps"),
            Some("x86_64-unknown-linux-gnu"),
        ),
        Path::new("/tmp/target")
    );
    assert_eq!(
        standalone_target_root(
            Path::new("/tmp/target/custom/release/deps"),
            Some("/tmp/targets/custom.json"),
        ),
        Path::new("/tmp/target")
    );
}

#[test]
fn validates_exact_rustc_output_set() {
    let root = tempfile::tempdir().unwrap();
    let outputs = test_outputs(root.path());
    let files =
        validated_outputs(test_output_directory(test_file("libdemo.rlib")), &outputs).unwrap();

    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|(_, path)| path == &outputs.files[0]));
    assert!(files.iter().any(|(_, path)| path == &outputs.dep_info));
}

#[test]
fn rejects_cached_output_path_traversal() {
    let root = tempfile::tempdir().unwrap();
    let outputs = test_outputs(root.path());
    assert!(
        validated_outputs(
            test_output_directory(test_file("../libdemo.rlib")),
            &outputs,
        )
        .is_err()
    );
}

#[test]
fn rejects_executable_rustc_outputs() {
    let root = tempfile::tempdir().unwrap();
    let outputs = test_outputs(root.path());
    let mut file = test_file("libdemo.rlib");
    file.executable = true;
    assert!(validated_outputs(test_output_directory(file), &outputs).is_err());
}

#[test]
fn accepts_wasm_executable_rustc_outputs() {
    let root = tempfile::tempdir().unwrap();
    let mut outputs = test_outputs(root.path());
    outputs.files = vec![outputs.directory.join("demo.wasm")];
    let mut file = test_file("demo.wasm");
    file.executable = true;

    assert!(validated_outputs(test_output_directory(file), &outputs).is_ok());
}

#[cfg(unix)]
#[test]
fn restores_declared_executable_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    std::fs::write(&source, b"wasm").unwrap();
    let node = CacheFileNode {
        digest: CacheDigest::blake3(b"wasm"),
        executable: true,
        mode: 0o644,
        name: "fixture.wasm".into(),
    };

    let (staged, _) = stage_verified_cached_output(root.path(), 0, &source, &node).unwrap();
    assert_eq!(
        std::fs::metadata(staged).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn rejects_group_or_world_writable_rustc_outputs() {
    let root = tempfile::tempdir().unwrap();
    let outputs = test_outputs(root.path());
    let mut file = test_file("libdemo.rlib");
    file.mode = 0o666;
    assert!(validated_outputs(test_output_directory(file), &outputs).is_err());
}

#[cfg(unix)]
#[test]
fn publication_masks_unsafe_rustc_output_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o666))
        .unwrap();
    assert_eq!(file_mode(&file.as_file().metadata().unwrap()), 0o644);
}

#[test]
fn rolls_back_outputs_after_a_partial_persist() {
    let root = tempfile::tempdir().unwrap();
    let first_destination = root.path().join("first.rlib");
    let blocked_destination = root.path().join("blocked.rmeta");
    std::fs::create_dir(&blocked_destination).unwrap();
    let staged = staged_outputs(
        root.path(),
        vec![
            (b"first", first_destination.clone()),
            (b"second", blocked_destination.clone()),
        ],
    );

    assert!(persist_outputs(staged).is_err());
    assert!(!first_destination.exists());
    assert!(blocked_destination.is_dir());
}

#[test]
fn qualification_does_not_publish_cached_outputs() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("cached.rlib");
    let staged = staged_outputs(root.path(), vec![(b"cached", destination.clone())]);

    finalize_restored_outputs(staged, false).unwrap();

    assert!(!destination.exists());
}

#[test]
fn materialized_outputs_are_independent_from_the_cas() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("cas-blob");
    std::fs::write(&source, b"artifact").unwrap();
    let staging = tempfile::tempdir_in(root.path()).unwrap();
    let node = test_file("artifact.rlib");

    let (output, _) = stage_verified_cached_output(staging.path(), 0, &source, &node).unwrap();
    std::fs::write(&output, b"modified").unwrap();

    assert_eq!(std::fs::read(source).unwrap(), b"artifact");
    assert_eq!(std::fs::read(output).unwrap(), b"modified");
}

#[test]
fn rejects_cached_outputs_with_the_wrong_size() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("cas-blob");
    std::fs::write(&source, b"short").unwrap();
    let staging = tempfile::tempdir_in(root.path()).unwrap();
    let node = test_file("artifact.rlib");

    assert!(stage_verified_cached_output(staging.path(), 0, &source, &node).is_err());
}

#[test]
fn materializes_read_only_cached_outputs() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("cas-blob");
    std::fs::write(&source, b"artifact").unwrap();
    let mut permissions = std::fs::metadata(&source).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&source, permissions).unwrap();
    let staging = tempfile::tempdir_in(root.path()).unwrap();
    let node = test_file("artifact.rlib");

    let (output, _) = stage_verified_cached_output(staging.path(), 0, &source, &node).unwrap();

    assert_eq!(std::fs::read(output).unwrap(), b"artifact");
    assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
    make_owner_writable(&source).unwrap();
}

#[test]
fn rejects_same_size_corrupt_cached_metadata() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("cas-blob");
    std::fs::write(&source, b"corrupt!").unwrap();
    let digest = CacheDigest::blake3(b"artifact");

    assert!(read_verified_blob(&source, &digest, "test blob").is_err());
}

#[test]
#[ignore = "local materialization benchmark"]
fn benchmark_cached_output_materialization() {
    let size_mib = std::env::var("MBX_BENCH_MIB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(128);
    let iterations = std::env::var("MBX_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4);
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("cas-blob");
    let mut source_file = std::fs::File::create(&source).unwrap();
    let chunk = vec![0x5a; 1024 * 1024];
    for _ in 0..size_mib {
        source_file.write_all(&chunk).unwrap();
    }
    source_file.sync_all().unwrap();
    drop(source_file);
    let digest = CacheDigest::blake3_file(&source).unwrap();
    let node = CacheFileNode {
        digest: digest.clone(),
        executable: false,
        mode: if cfg!(unix) { 0o644 } else { 0 },
        name: "artifact.rlib".into(),
    };

    let staging = tempfile::tempdir_in(root.path()).unwrap();
    let started = std::time::Instant::now();
    for _ in 0..iterations {
        let temporary = staging.path().join("legacy-output");
        reflink_copy::reflink_or_copy(&source, &temporary).unwrap();
        let temporary = tempfile::TempPath::try_from_path(temporary).unwrap();
        make_owner_writable(&temporary).unwrap();
        assert!(digest.matches_file(&temporary).unwrap());
        apply_file_mode(&temporary, node.mode, node.executable).unwrap();
    }
    let legacy = started.elapsed();

    let staging = tempfile::tempdir_in(root.path()).unwrap();
    let started = std::time::Instant::now();
    let mut method = None;
    for _ in 0..iterations {
        let (_, observed) =
            stage_verified_cached_output(staging.path(), 0, &source, &node).unwrap();
        method = Some(observed);
    }
    let materialized = started.elapsed();

    println!(
        "materialized {iterations} x {size_mib} MiB with {method:?}: legacy_reverify={legacy:.2?}, verified_cas={materialized:.2?}, speedup={:.2}x",
        legacy.as_secs_f64() / materialized.as_secs_f64()
    );
}
