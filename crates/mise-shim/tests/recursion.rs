//! Run the built shim in states that must never reach `mise`.
//!
//! Each copy runs in its own directory with an empty PATH, so a shim that does try to run `mise`
//! fails with "failed to execute mise" instead of looping; the assertions tell that failure apart
//! from the guard's.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// Copy the shim to a fresh directory as `name`, and run it with an empty PATH and `target` as
/// what `mise x` resolved (`None` for a shim run directly). `target` gets the copy's own path.
fn run_shim_as(case: &str, name: &str, target: Option<fn(&PathBuf) -> PathBuf>) -> Output {
    let dir = env::temp_dir().join(format!("mise-shim-test-{}-{case}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let bin_dir = dir.join("bin");
    let empty_path = dir.join("empty");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&empty_path).unwrap();
    let shim = bin_dir.join(format!("{name}{}", env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_mise-shim"), &shim).unwrap();

    let mut command = Command::new(&shim);
    command
        .env("PATH", &empty_path)
        .env_remove("__MISE_SHIM_PATH")
        .env_remove("__MISE_SHIM_TARGET");
    if let Some(target) = target {
        command.env("__MISE_SHIM_TARGET", target(&shim));
    }
    let output = command.output().unwrap();
    let _ = fs::remove_dir_all(&dir);
    output
}

#[test]
fn refuses_to_run_as_its_own_name() {
    // `mise x -- mise-shim` resolves to a copy of the shim again, so this used to recurse.
    for (case, name) in [("lower", "mise-shim"), ("upper", "MISE-SHIM")] {
        let output = run_shim_as(case, name, None);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{name}: {stderr}");
        assert!(stderr.contains("refusing to run as"), "{name}: {stderr}");
        assert!(
            !stderr.contains("failed to execute mise"),
            "{name}: {stderr}"
        );
    }
}

#[test]
fn stops_when_mise_x_resolved_to_this_shim() {
    // Two shim copies on PATH: `mise x` skips the caller and picks the other copy, which would
    // run `mise x` again. `mise x` names what it picked, so that copy stops instead.
    let output = run_shim_as("target-self", "mytool", Some(|shim| shim.clone()));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("which is another shim"), "{stderr}");
    assert!(!stderr.contains("failed to execute mise"), "{stderr}");
}

#[test]
fn a_target_elsewhere_still_runs_mise() {
    // The control: a real tool's children inherit the tool's path, and a shim they run must go
    // on to `mise`, which the empty PATH lacks.
    let output = run_shim_as(
        "target-other",
        "mytool",
        Some(|shim| shim.with_file_name("real-tool.exe")),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to execute mise"), "{stderr}");
}
