use crate::progress::SingleReport;
use pretty_assertions::assert_eq;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// `cmd.stream` takes the exclusive side of RAW_LOCK while `os.execute` takes the
// shared side. Because both poll rather than await, a steady stream of readers
// starved the writer: a trivial `cmd.stream` child waited 8s behind three
// plugins looping on `os.execute`. Readers must yield to a pending writer. (#13254)
#[test]
fn raw_read_lock_yields_to_a_waiting_writer() {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    let waiting = super::RawWriterWaiting::new();
    let acquired = Arc::new(AtomicBool::new(false));
    let flag = acquired.clone();
    let reader = std::thread::spawn(move || {
        let _guard = super::raw_read_lock_blocking();
        flag.store(true, Ordering::SeqCst);
    });

    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !acquired.load(Ordering::SeqCst),
        "reader acquired the shared lock while a writer was waiting"
    );

    drop(waiting);
    reader.join().unwrap();
    assert!(acquired.load(Ordering::SeqCst));
}

// The counter must govern the async path too: mise's own installs acquire the
// shared side with `RAW_LOCK.read().await`, which queues behind an async writer
// but cannot see a sync one polling `try_write`. (#13254)
#[tokio::test]
async fn raw_read_lock_async_yields_to_a_waiting_writer() {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    let waiting = super::RawWriterWaiting::new();
    let acquired = Arc::new(AtomicBool::new(false));
    let flag = acquired.clone();
    let reader = tokio::spawn(async move {
        let _guard = super::raw_read_lock().await;
        flag.store(true, Ordering::SeqCst);
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !acquired.load(Ordering::SeqCst),
        "async reader acquired the shared lock while a writer was waiting"
    );

    drop(waiting);
    reader.await.unwrap();
    assert!(acquired.load(Ordering::SeqCst));
}

// Readers acquire then verify, so a writer registering mid-acquisition is not
// bypassed. Exercising that interleaving deterministically would need a test-only
// injection point between the two steps; this covers the property it protects —
// continuous reader churn must not keep a writer out. (#13254)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn raw_write_lock_is_not_starved_by_reader_churn() {
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    let stop = Arc::new(AtomicBool::new(false));
    let churn: Vec<_> = (0..4)
        .map(|_| {
            let stop = stop.clone();
            tokio::spawn(async move {
                while !stop.load(Ordering::SeqCst) {
                    let guard = super::raw_read_lock().await;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    drop(guard);
                }
            })
        })
        .collect();

    // Let the readers get going so the writer arrives mid-churn.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let started = Instant::now();
    drop(super::raw_write_lock().await);
    let waited = started.elapsed();

    stop.store(true, Ordering::SeqCst);
    for task in churn {
        task.await.unwrap();
    }
    assert!(
        waited < Duration::from_secs(5),
        "writer waited {waited:?} behind churning readers"
    );
}

#[derive(Debug, Default)]
struct RecordingReport {
    lines: Mutex<Vec<String>>,
    messages: Mutex<Vec<String>>,
}

impl SingleReport for RecordingReport {
    fn println(&self, message: String) {
        self.lines.lock().unwrap().push(message);
    }

    fn set_message(&self, message: String) {
        self.messages.lock().unwrap().push(message);
    }
}

#[test]
fn test_stderr_as_stdout_routes_through_process_output() {
    let report = RecordingReport::default();
    super::CmdLineRunner::new("sh")
        .args(["-c", "printf 'version banner\\n' >&2"])
        .with_pr(&report)
        .stderr_as_stdout()
        .execute()
        .unwrap();

    assert!(report.lines.lock().unwrap().is_empty());
    assert_eq!(
        *report.messages.lock().unwrap(),
        vec!["version banner".to_string()]
    );
}

#[test]
fn test_stderr_as_stdout_replays_hidden_output_on_failure() {
    let report = RecordingReport::default();
    drop(
        super::CmdLineRunner::new("sh")
            .args(["-c", "printf 'verification failed\\n' >&2; exit 1"])
            .with_pr(&report)
            .stderr_as_stdout()
            .execute()
            .unwrap_err(),
    );

    assert_eq!(
        *report.lines.lock().unwrap(),
        vec!["verification failed".to_string()]
    );
}

#[test]
fn test_stderr_callback_is_not_replayed_on_failure() {
    let report = RecordingReport::default();
    let callback_lines = Arc::new(Mutex::new(Vec::new()));
    let callback_lines_ref = Arc::clone(&callback_lines);
    drop(
        super::CmdLineRunner::new("sh")
            .args(["-c", "printf 'callback failure\n' >&2; exit 1"])
            .with_pr(&report)
            .stderr_as_stdout()
            .with_on_stderr(move |line| callback_lines_ref.lock().unwrap().push(line))
            .execute()
            .unwrap_err(),
    );

    assert!(report.lines.lock().unwrap().is_empty());
    assert_eq!(
        *callback_lines.lock().unwrap(),
        vec!["callback failure".to_string()]
    );
}

/// A command that fails during an install has already said why on stderr.
/// The error that ends the run carried only the exit status, so under
/// `--quiet` — where the reporter prints nothing — the reason was lost.
/// See: <https://github.com/jdx/mise/discussions/13306>
#[test]
fn test_failure_error_names_the_last_stderr_line() {
    let err = super::CmdLineRunner::new("sh")
            .args([
                "-c",
                "printf 'noise\n' >&2; printf 'error while loading shared libraries: libncurses.so.6\n' >&2; exit 127",
            ])
            .execute()
            .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("exit code 127"),
        "expected the exit status, got {message:?}"
    );
    assert!(
        message.contains("last stderr: error while loading shared libraries: libncurses.so.6"),
        "expected the child's last stderr line, got {message:?}"
    );
}

#[test]
fn test_failure_error_without_stderr_is_unchanged() {
    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "exit 3"])
        .execute()
        .unwrap_err();

    let message = err.to_string();
    assert_eq!(message, "sh exited with non-zero status: exit code 3");
}

#[test]
fn test_stderr_tail_for_error_ignores_stdout_and_blank_lines() {
    let output = vec![
        ("the reason".to_string(), super::OutputSource::Stderr),
        ("   ".to_string(), super::OutputSource::Stderr),
        ("later stdout".to_string(), super::OutputSource::Stdout),
    ];

    assert_eq!(
        super::stderr_tail_for_error(&output),
        Some("the reason".to_string())
    );
    assert_eq!(super::stderr_tail_for_error(&[]), None);
}

#[test]
fn test_stderr_tail_for_error_truncates_by_character_not_byte() {
    let line = "あ".repeat(super::STDERR_TAIL_MAX_CHARS + 10);
    let output = vec![(line, super::OutputSource::Stderr)];

    let tail = super::stderr_tail_for_error(&output).unwrap();
    assert!(tail.ends_with('…'));
    // Counting bytes would have cut a diagnostic in a non-ASCII locale at a
    // third of the documented limit.
    assert_eq!(
        tail.chars().count(),
        super::STDERR_TAIL_MAX_CHARS + 1,
        "expected {} characters plus the ellipsis",
        super::STDERR_TAIL_MAX_CHARS
    );
}

#[test]
fn test_stderr_tail_for_error_keeps_a_line_at_the_limit_whole() {
    let line = "あ".repeat(super::STDERR_TAIL_MAX_CHARS);
    let output = vec![(line.clone(), super::OutputSource::Stderr)];

    assert_eq!(super::stderr_tail_for_error(&output), Some(line));
}

#[test]
fn test_failure_output_tail_preserves_output_within_limit() {
    let mut output = super::FailureOutputTail::default();
    output.push("first".to_string());
    output.push("second".to_string());

    let lines = output
        .into_output()
        .into_iter()
        .map(|(line, _)| line)
        .collect::<Vec<_>>();
    assert_eq!(lines, ["first", "second"]);
}

#[test]
fn test_failure_output_tail_discards_oldest_output() {
    let mut output = super::FailureOutputTail::default();
    for i in 0..=super::FAILURE_OUTPUT_TAIL_BYTES / 8 {
        output.push(format!("{i:07}"));
    }

    assert!(output.bytes <= super::FAILURE_OUTPUT_TAIL_BYTES);
    let lines = output
        .into_output()
        .into_iter()
        .map(|(line, _)| line)
        .collect::<Vec<_>>();
    assert_eq!(
        lines.first().unwrap(),
        super::FAILURE_OUTPUT_TRUNCATED_NOTICE
    );
    assert!(!lines.contains(&"0000000".to_string()));
    assert_eq!(lines.last().unwrap(), "0008192");
}

#[test]
fn test_failure_output_tail_truncates_large_unicode_line() {
    let mut output = super::FailureOutputTail::default();
    output.push("あ".repeat(super::FAILURE_OUTPUT_TAIL_BYTES));

    assert!(output.bytes <= super::FAILURE_OUTPUT_TAIL_BYTES);
    let lines = output
        .into_output()
        .into_iter()
        .map(|(line, _)| line)
        .collect::<Vec<_>>();
    assert_eq!(
        lines.first().unwrap(),
        super::FAILURE_OUTPUT_TRUNCATED_NOTICE
    );
    assert!(
        lines
            .last()
            .unwrap()
            .is_char_boundary(lines.last().unwrap().len())
    );
    assert!(lines.last().unwrap().len() < super::FAILURE_OUTPUT_TAIL_BYTES);
}

#[test]
fn test_failure_output_tail_only_enabled_for_hidden_stdout() {
    let report = RecordingReport::default();
    assert!(
        super::CmdLineRunner::new("true")
            .with_pr(&report)
            .failure_output_tail()
            .is_some()
    );
    assert!(
        super::CmdLineRunner::new("true")
            .with_pr(&report)
            .with_on_stdout(|_| {})
            .failure_output_tail()
            .is_none()
    );
    assert!(
        super::CmdLineRunner::new("true")
            .failure_output_tail()
            .is_none()
    );
}

#[tokio::test]
async fn test_failure_output_tail_replayed_on_async_failure() {
    let report = RecordingReport::default();
    let err = super::CmdLineRunner::new("sh")
        .args([
            "-c",
            "i=0; while [ $i -lt 10000 ]; do printf '%07d\\n' $i; i=$((i + 1)); done; exit 1",
        ])
        .with_pr(&report)
        .execute_async()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("exited with non-zero status"));
    let lines = report.lines.lock().unwrap();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with(super::FAILURE_OUTPUT_TRUNCATED_NOTICE));
    assert!(!lines[0].contains("0000000"));
    assert!(lines[0].ends_with("0009999"));
}

#[test]
fn test_child_process_isolation() {
    use super::ChildProcessIsolation::{Inherit, ProcessGroup, Session};

    assert_eq!(super::child_process_isolation(true, true, true), Inherit);
    assert_eq!(super::child_process_isolation(false, true, true), Session);
    assert_eq!(
        super::child_process_isolation(false, false, true),
        ProcessGroup
    );
    assert_eq!(
        super::child_process_isolation(false, true, false),
        ProcessGroup
    );
}

#[tokio::test]
async fn test_cmd_line_runner_execute_async() {
    let stdout = Arc::new(Mutex::new(Vec::new()));
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let observed_stdout = Arc::new(Mutex::new(Vec::new()));
    let observed_stderr = Arc::new(Mutex::new(Vec::new()));
    let stdout_clone = stdout.clone();
    let stderr_clone = stderr.clone();
    let observed_stdout_clone = observed_stdout.clone();
    let observed_stderr_clone = observed_stderr.clone();
    super::CmdLineRunner::new("sh")
        .args(["-c", "printf out; printf err >&2"])
        .with_on_stdout(move |line| stdout_clone.lock().unwrap().push(line))
        .with_on_stderr(move |line| stderr_clone.lock().unwrap().push(line))
        .with_stdout_observer(move |line| {
            observed_stdout_clone.lock().unwrap().push(line.to_string());
        })
        .with_stderr_observer(move |line| {
            observed_stderr_clone.lock().unwrap().push(line.to_string());
        })
        .execute_async()
        .await
        .unwrap();
    assert_eq!(stdout.lock().unwrap().as_slice(), ["out"]);
    assert_eq!(stderr.lock().unwrap().as_slice(), ["err"]);
    assert_eq!(observed_stdout.lock().unwrap().as_slice(), ["out"]);
    assert_eq!(observed_stderr.lock().unwrap().as_slice(), ["err"]);
}

#[tokio::test]
async fn test_output_observers_compose() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (first, second) = (seen.clone(), seen.clone());
    super::CmdLineRunner::new("sh")
        .args(["-c", "printf out"])
        .with_on_stdout(|_| {})
        .with_stdout_observer(move |line| first.lock().unwrap().push(format!("1:{line}")))
        .with_stdout_observer(move |line| second.lock().unwrap().push(format!("2:{line}")))
        .execute_async()
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().as_slice(), ["1:out", "2:out"]);
}

#[cfg(unix)]
#[tokio::test]
async fn test_direct_inline_supervision() {
    let runner = || {
        super::CmdLineRunner::new("missing-shell")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-c", "sleep 10"])
            .optimize_inline("sleep 10", &[], true)
    };
    let err = runner()
        .with_timeout(std::time::Duration::from_millis(20))
        .execute_async()
        .await
        .unwrap_err();
    assert!(err.to_string().contains("timed out"), "{err:?}");
    let err = runner()
        .execute_async_with_cancel_check(|| true)
        .await
        .unwrap_err();
    assert!(crate::errors::ProcessError::is_task_interrupted_before_start(&err));
    let output = super::CmdLineRunner::new("missing-shell")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .optimize_inline("cat", &[], true)
        .stdin_string("literal input")
        .read_bounded(1024)
        .await
        .unwrap();
    assert_eq!(output, "literal input");
}

#[cfg(unix)]
#[tokio::test]
async fn test_cmd_line_runner_read_bounded() {
    let output = super::CmdLineRunner::new("sh")
        .args(["-c", "cat"])
        .stdin_string("bounded input")
        .with_timeout(std::time::Duration::from_secs(1))
        .read_bounded(1024)
        .await
        .unwrap();
    assert_eq!(output, "bounded input");

    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "printf 12345"])
        .with_timeout(std::time::Duration::from_secs(1))
        .read_bounded(4)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("output exceeded 4 bytes"));
}

#[tokio::test]
async fn test_execute_async_skips_pre_cancelled_command() {
    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "exit 0"])
        .execute_async_with_cancel_check(|| true)
        .await
        .unwrap_err();

    assert!(crate::errors::ProcessError::is_task_interrupted_before_start(&err));
}

#[tokio::test]
async fn test_execute_async_catches_cancellation_after_spawn() {
    let checks = Arc::new(AtomicUsize::new(0));
    let checks_c = checks.clone();
    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "sleep 30"])
        .execute_async_with_cancel_check(move || checks_c.fetch_add(1, Ordering::SeqCst) > 0)
        .await
        .unwrap_err();

    assert!(crate::errors::ProcessError::is_sigint(&err));
    assert!(checks.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn test_execute_raw_async_catches_cancellation_after_spawn() {
    let checks = Arc::new(AtomicUsize::new(0));
    let checks_c = checks.clone();
    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "sleep 30"])
        .raw(true)
        .execute_async_with_cancel_check(move || checks_c.fetch_add(1, Ordering::SeqCst) > 0)
        .await
        .unwrap_err();

    assert!(crate::errors::ProcessError::is_sigint(&err));
    assert!(checks.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn test_cmd_line_runner_read_ignores_raw_mode() {
    let output = super::CmdLineRunner::new("sh")
        .args(["-c", "printf out"])
        .raw(true)
        .read()
        .await
        .unwrap();
    assert_eq!(output, "out");
}

#[tokio::test]
async fn test_cmd_line_runner_read_replays_stderr_on_failure() {
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let stderr_clone = stderr.clone();
    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "printf err >&2; exit 1"])
        .with_on_stderr(move |line| stderr_clone.lock().unwrap().push(line))
        .read()
        .await
        .unwrap_err();
    assert!(err.to_string().contains("exited with non-zero status"));
    assert_eq!(stderr.lock().unwrap().as_slice(), ["err"]);
}

#[tokio::test]
async fn test_cmd_line_runner_execute_hashes_async() {
    let (stdout_hash, stderr_hash) = super::CmdLineRunner::new("sh")
        .args(["-c", "printf stdout; printf stderr >&2"])
        .execute_hashes_async(1024)
        .await
        .unwrap();
    assert_eq!(stdout_hash, blake3::hash(b"stdout").to_hex().to_string());
    assert_eq!(stderr_hash, blake3::hash(b"stderr").to_hex().to_string());
}

#[tokio::test]
async fn test_cmd_line_runner_execute_hashes_async_limits_output() {
    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "printf 12345"])
        .execute_hashes_async(4)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("output exceeded 4 bytes"));
}

#[tokio::test]
async fn test_cmd_line_runner_execute_hashes_async_times_out() {
    let err = super::CmdLineRunner::new("sh")
        // Replace the shell so there is no descendant holding the pipes
        // after the timed-out process is terminated.
        .args(["-c", "exec sleep 60"])
        .with_timeout(std::time::Duration::from_millis(10))
        .execute_hashes_async(1024)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("timed out"));
}

#[tokio::test]
async fn test_cmd_line_runner_execute_hashes_async_rejects_undrained_pipes() {
    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "sleep 60 &"])
        .execute_hashes_async_with_drain_timeout(1024, std::time::Duration::from_millis(20))
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("command output pipes did not close")
    );
}

/// A descendant that outlives the shell must not survive the drain
/// deadline — cleanup goes through the process group, so it reaches the
/// leaves even though only the shell is a direct child.
#[cfg(unix)]
#[tokio::test]
async fn test_cmd_line_runner_execute_hashes_async_kills_descendants() {
    if !super::should_use_pgroup() {
        // No pgroup of our own to killpg; an ancestor owns cleanup.
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("descendant.pid");
    let err = super::CmdLineRunner::new("sh")
        .args([
            "-c",
            &format!("sleep 60 & printf %s \"$!\" >{}", pid_file.display()),
        ])
        .execute_hashes_async_with_drain_timeout(1024, std::time::Duration::from_millis(20))
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("command output pipes did not close")
    );
    let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    let pid = nix::unistd::Pid::from_raw(pid);
    for _ in 0..100 {
        if nix::sys::signal::kill(pid, None).is_err() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("descendant {pid} survived cleanup");
}

#[test]
fn test_env_values_treats_false_as_removal() {
    use std::ffi::OsStr;

    let runner = super::CmdLineRunner::new("true")
        .env("REMOVE", "inherited")
        .env_values([
            ("KEEP", crate::env_value::EnvValue::from("value")),
            ("REMOVE", crate::env_value::EnvValue::from(false)),
        ]);

    let env = runner.cmd.as_std().get_envs().collect::<Vec<_>>();
    assert!(
        env.iter().any(|(key, value)| {
            *key == OsStr::new("KEEP") && value == &Some(OsStr::new("value"))
        })
    );
    assert!(
        env.iter()
            .any(|(key, value)| *key == OsStr::new("REMOVE") && value.is_none())
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn test_macos_sandbox_preserves_env_removals() {
    use std::ffi::OsStr;

    let mut runner = super::CmdLineRunner::new("true")
        .env("KEEP", "value")
        .env_remove("DROP")
        .with_sandbox(crate::sandbox::SandboxConfig {
            deny_read: true,
            ..Default::default()
        });

    runner.apply_sandbox().await.unwrap();

    let env = runner.cmd.as_std().get_envs().collect::<Vec<_>>();
    assert!(
        env.iter().any(|(key, value)| {
            *key == OsStr::new("KEEP") && value == &Some(OsStr::new("value"))
        })
    );
    assert!(
        env.iter()
            .any(|(key, value)| *key == OsStr::new("DROP") && value.is_none())
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn test_macos_sandbox_preserves_piped_stdin() {
    let mut runner = super::CmdLineRunner::new("/bin/cat")
        .stdin_string("sandboxed stdin")
        .with_sandbox(crate::sandbox::SandboxConfig {
            deny_process: true,
            ..Default::default()
        });

    runner.apply_sandbox().await.unwrap();

    assert_eq!(runner.read().await.unwrap(), "sandboxed stdin");
}

#[test]
fn test_running_pid_guard_removes_pid() {
    let pid = 424_242;
    assert!(!super::RUNNING_PIDS.lock().unwrap().contains(&pid));
    {
        let _guard = super::RunningPidGuard::new(Some(pid));
        assert!(super::RUNNING_PIDS.lock().unwrap().contains(&pid));
    }
    assert!(!super::RUNNING_PIDS.lock().unwrap().contains(&pid));
}

/// Raw mode hands the child mise's own stdout, which is why nothing passes
/// through the redactor there. `never_raw` is how a command carrying a
/// credential in its arguments opts out, so the observable contract is that
/// output is captured even with `raw` requested: in raw mode `on_stdout`
/// never fires at all.
#[test]
fn never_raw_keeps_output_captured_even_when_raw_is_requested() {
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let sink = seen.clone();
    super::CmdLineRunner::new("echo")
        .arg("captured")
        .raw(true)
        .never_raw()
        .with_on_stdout(move |line| sink.lock().unwrap().push(line))
        .execute()
        .expect("echo should run");

    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &["captured".to_string()],
        "raw mode was not refused, so nothing could be redacted"
    );
}

#[test]
fn test_cmd_body_args_unix_fallthrough() {
    // On Unix `cmd_body_args` must be exactly `args(flags).arg(body)` — the
    // non-regression contract shared by every CmdLineRunner call site.
    let r = super::CmdLineRunner::new("bash").cmd_body_args(&["-c".to_string()], "echo hi");
    assert_eq!(r.get_args(), vec!["-c".to_string(), "echo hi".to_string()]);
}
