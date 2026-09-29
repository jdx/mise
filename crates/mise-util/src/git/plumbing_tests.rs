use super::*;

#[test]
fn plumbing_isolates_user_config_and_keeps_the_scratch_index() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = GitPlumbing::new(tmp.path().join("shadow.git"));
    let index = tmp.path().join("scratch-index");
    let work_tree = tmp.path().join("tree");
    let cmd = repo
        .command(
            &PlumbingCall::new(["write-tree"])
                .work_tree(&work_tree)
                .index_file(&index),
        )
        .unwrap();
    let envs: HashMap<_, _> = cmd
        .get_envs()
        .map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string())))
        .collect();
    // sanitize_git_command strips GIT_INDEX_FILE; the call sets it afterwards
    assert_eq!(
        envs.get(OsStr::new("GIT_INDEX_FILE")).cloned().flatten(),
        Some(index.into_os_string())
    );
    assert_eq!(
        envs.get(OsStr::new("GIT_DIR")).cloned(),
        Some(None),
        "GIT_DIR must be removed rather than inherited"
    );
    assert_eq!(
        envs.get(OsStr::new("GIT_CONFIG_NOSYSTEM"))
            .cloned()
            .flatten(),
        Some(OsString::from("1"))
    );
    assert_eq!(
        envs.get(OsStr::new("GIT_CONFIG_GLOBAL")).cloned().flatten(),
        Some(OsString::from("/dev/null"))
    );
    for variable in ["GIT_CONFIG_COUNT", "GIT_CONFIG_PARAMETERS"] {
        assert_eq!(envs.get(OsStr::new(variable)), Some(&None));
    }
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(
        args.iter().any(|arg| arg.starts_with("--git-dir=")),
        "{args:?}"
    );
    assert!(
        args.iter().any(|arg| arg.starts_with("--work-tree=")),
        "{args:?}"
    );
    assert_eq!(args.last().map(String::as_str), Some("write-tree"));
}

#[test]
fn init_bare_is_idempotent_and_ignores_global_config() {
    if plumbing_binary().is_none() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = GitPlumbing::new(tmp.path().join("shadow.git"));
    assert!(!repo.exists());
    repo.init_bare().unwrap();
    assert!(repo.exists());
    repo.init_bare().unwrap();
    let hooks = repo
        .output_str(PlumbingCall::new(["config", "--get", "gc.auto"]))
        .unwrap();
    assert_eq!(hooks, "0");
    // a global hooksPath or filter must not reach the shadow repository
    let global = repo
        .output_unchecked(PlumbingCall::new([
            "config",
            "--global",
            "--get",
            "core.hooksPath",
        ]))
        .unwrap();
    assert!(!global.status.success() || global.stdout.is_empty());
}

#[test]
fn network_commands_override_hooks_with_a_private_directory() {
    if plumbing_binary().is_none() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let repo = GitPlumbing::new(temp.path().join("network.git"));
    repo.init_bare().unwrap();
    repo.run(PlumbingCall::new([
        "config",
        "core.hooksPath",
        "untrusted-hooks",
    ]))
    .unwrap();
    let internal = repo
        .output_str(PlumbingCall::new(["config", "--get", "core.hooksPath"]))
        .unwrap();
    assert!(Path::new(&internal).is_dir());
    assert_eq!(std::fs::read_dir(&internal).unwrap().count(), 0);
    let out = repo
        .network_output(PlumbingCall::new(["config", "--get", "core.hooksPath"]))
        .unwrap();
    assert!(out.status.success());
    let hooks = String::from_utf8(out.stdout).unwrap();
    let hooks = Path::new(hooks.trim());
    assert!(hooks.is_absolute());
    assert_ne!(hooks, Path::new("/dev/null"));
    assert!(
        !hooks.exists(),
        "temporary hooks directory was not cleaned up"
    );
}
