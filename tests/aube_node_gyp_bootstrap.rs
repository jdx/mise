//! Regression: embedded aube re-execs the host as `__node-gyp-bootstrap`.
//!
//! Without an early `main` intercept, mise's naked-run rewrite turns that into
//! `mise run __node-gyp-bootstrap`, which fails with "no tasks defined" and
//! breaks `allow_builds` installs that need node-gyp (gemini-cli → node-pty).
//! Registry `test-tool` can miss this; this binary-level check must keep passing.

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn mise_binary_services_aube_node_gyp_bootstrap_trampoline() {
    let mise = env!("CARGO_BIN_EXE_mise");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let project = std::env::temp_dir().join(format!("mise-aube-ngyp-{stamp}"));
    fs::create_dir_all(&project).expect("create temp project dir");
    let project = project.to_str().expect("utf-8 path");

    let output = Command::new(mise)
        .args(["__node-gyp-bootstrap", project])
        .output()
        .expect("spawn mise");

    let _ = fs::remove_dir_all(project);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        output.status.success(),
        "mise __node-gyp-bootstrap should exit 0; status={:?}\nstdout={stdout}\nstderr={stderr}",
        output.status
    );
    assert!(
        stdout.contains("node-gyp"),
        "expected bootstrapped node-gyp path on stdout, got: {stdout:?}"
    );
    assert!(
        !combined.contains("no tasks defined"),
        "trampoline must not fall through to naked `mise run`; output was: {combined}"
    );
    assert!(
        !combined.contains("Are you in a project directory"),
        "trampoline must not fall through to naked `mise run`; output was: {combined}"
    );

    let path = stdout.lines().next().unwrap_or_default().trim();
    assert!(
        std::path::Path::new(path).is_file(),
        "bootstrapped path should exist and be a file: {path:?}"
    );
}
