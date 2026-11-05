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
async fn test_on_spawn_runs_only_once_the_process_started() {
    let spawned = Arc::new(AtomicUsize::new(0));
    let runner = |program: &str| {
        let spawned = spawned.clone();
        super::CmdLineRunner::new(program)
            .args(["-c", "exit 3"])
            .with_on_spawn(move || {
                spawned.fetch_add(1, Ordering::SeqCst);
            })
    };
    let cancelled = runner("sh").execute_async_with_cancel_check(|| true).await;
    assert!(cancelled.is_err());
    let missing = runner("mise-test-missing-program").execute_async().await;
    assert!(missing.is_err());
    assert_eq!(spawned.load(Ordering::SeqCst), 0);
    // a command that starts and then fails has spawned
    let failed = runner("sh").execute_async().await;
    assert!(failed.is_err());
    assert_eq!(spawned.load(Ordering::SeqCst), 1);
}

#[test]
fn test_on_spawn_runs_for_a_synchronous_execute() {
    let spawned = Arc::new(AtomicUsize::new(0));
    let counter = spawned.clone();
    super::CmdLineRunner::new("sh")
        .args(["-c", "exit 0"])
        .with_on_spawn(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .execute()
        .unwrap();
    assert_eq!(spawned.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_capped_lines_split_a_long_line_on_character_boundaries() {
    let input = "ab\r\néééé\nlast".as_bytes();
    let mut lines = super::CappedLines::with_max(input, 5, Default::default());
    let mut seen = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        seen.push(line);
    }
    // "éééé\n" is 9 bytes: a 5-byte piece would cut the third "é", so it moves on
    assert_eq!(seen, ["ab", "éé", "éé", "last"]);

    // a line exactly the limit long is one line, not a line and an empty one
    let mut lines = super::CappedLines::with_max("abcde\nf".as_bytes(), 5, Default::default());
    assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("abcde"));
    assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("f"));
    assert_eq!(lines.next_line().await.unwrap(), None);

    // so is one that ends with CRLF, whose `\r` lies past the limit
    let pieces = capped_pieces(b"abcde\r\nf\r\n", Some(5), &[]).await;
    assert_eq!(pieces, ["abcde", "f"]);
}

#[tokio::test]
async fn test_capped_lines_bound_output_without_newlines() {
    let input = vec![b'x'; super::MAX_LINE_BYTES * 3 + 7];
    let mut lines = super::CappedLines::new(input.as_slice(), Default::default());
    let mut lengths = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        lengths.push(line.len());
    }
    let max = super::MAX_LINE_BYTES;
    assert_eq!(lengths, [max, max, max, 7]);
}

async fn capped_pieces(input: &[u8], max: Option<usize>, secrets: &[&str]) -> Vec<String> {
    let redactor = crate::redactions::Redactor::new(secrets.iter().map(|s| s.to_string()));
    let mut lines = match max {
        Some(max) => super::CappedLines::with_max(input, max, redactor),
        None => super::CappedLines::new(input, redactor),
    };
    let mut pieces = Vec::new();
    while let Some(piece) = lines.next_line().await.unwrap() {
        pieces.push(piece);
    }
    pieces
}

#[tokio::test]
async fn test_capped_lines_never_cut_a_secret_at_the_limit() {
    let secret = "s3cr3t-ToKeN-value";
    let max = super::MAX_LINE_BYTES;
    // the secret starts 5 bytes before the cut, so a plain cut would split it
    let input = format!("{}{secret}{}", "x".repeat(max - 5), "y".repeat(100));
    let pieces = capped_pieces(input.as_bytes(), None, &[secret]).await;
    assert_eq!(pieces.concat(), input, "no output lost or repeated");
    assert!(pieces.iter().all(|piece| piece.len() < max + secret.len()));
    let redactor = crate::redactions::Redactor::new([secret.to_string()]);
    let shown: Vec<String> = pieces.iter().map(|piece| redactor.redact(piece)).collect();
    assert_eq!(shown.concat().matches("[redacted]").count(), 1);
    for part in ["s3c", "ToKeN", "value", "t-T"] {
        assert!(
            shown.iter().all(|piece| !piece.contains(part)),
            "{part} shown"
        );
    }
}

#[tokio::test]
async fn test_capped_lines_keep_overlapping_secrets_and_characters_whole() {
    // "é" is bytes 8 and 9, so 9 is no boundary; 8, 7 and 6 would expose part of "bcd" or
    // "cdé", so the cut moves back to 5, where both start whole in the next piece
    let input = "aaaaabcdéfff";
    let pieces = capped_pieces(input.as_bytes(), Some(9), &["bcd", "cdé"]).await;
    assert_eq!(pieces, ["aaaaa", "bcdéfff"]);
    // a secret longer than a piece stays whole, in one longer piece
    let pieces = capped_pieces(b"abcdefgh-z", Some(3), &["abcdefgh"]).await;
    assert_eq!(pieces[0], "abcdefgh");
    assert_eq!(pieces.concat(), "abcdefgh-z");
}

/// Which bytes of `text` redacting it would hide.
fn hidden(redactor: &crate::redactions::Redactor, text: &str) -> Vec<bool> {
    let mut mask = vec![false; text.len()];
    for (s, e) in redactor.match_spans(text.as_bytes()) {
        mask[s..e].iter_mut().for_each(|hidden| *hidden = true);
    }
    mask
}

#[tokio::test]
async fn test_capped_lines_keep_distinct_overlapping_secrets_whole() {
    let redactor = crate::redactions::Redactor::new(["abcde".into(), "cdefgh".into()]);
    let pieces = capped_pieces(b"xabcdefghy", Some(5), &["abcde", "cdefgh"]).await;
    assert_eq!(pieces, ["x", "abcdefgh", "y"]);
    let shown: String = pieces.iter().map(|p| redactor.redact(p)).collect();
    assert_eq!(shown, redactor.redact("xabcdefghy"));

    let redactor = crate::redactions::Redactor::new(["secret".into(), "secret-long".into()]);
    for max in 1..14 {
        let pieces = capped_pieces(b"zzsecret-longzz", Some(max), &["secret", "secret-long"]).await;
        let shown: String = pieces.iter().map(|p| redactor.redact(p)).collect();
        assert!(!shown.contains("-long"), "max {max}: {shown}");
        assert!(!shown.contains("secret"), "max {max}: {shown}");
    }
}

#[tokio::test]
async fn test_capped_lines_hide_what_redacting_the_whole_line_hides() {
    let cases: &[(&str, &[&str])] = &[
        ("xxabcdefghyyabcdezzcdefgh", &["abcde", "cdefgh"]),
        (
            "..secret-long..secret..secret-longer",
            &["secret", "secret-long"],
        ),
        ("aaaaaaaaaaaaaaaaaaaaaaaaab", &["aaaa"]),
        ("abababababab-ba-ab", &["aba", "bab"]),
        ("ééabcéédéabcé", &["abcé", "édé"]),
        ("no secrets in this line at all", &["zzz"]),
    ];
    for (line, secrets) in cases {
        let redactor = crate::redactions::Redactor::new(secrets.iter().map(|s| s.to_string()));
        let want = hidden(&redactor, line);
        for max in 1..=line.len() + 1 {
            // the line alone, and ended by `\n` or `\r\n`, which must add no empty line
            for ending in ["", "\n", "\r\n"] {
                let input = format!("{line}{ending}");
                let pieces = capped_pieces(input.as_bytes(), Some(max), secrets).await;
                if !ending.is_empty() && max >= 4 {
                    // and the line after it starts right after the line end
                    let input = format!("{line}{ending}next");
                    let mut more = capped_pieces(input.as_bytes(), Some(max), secrets).await;
                    assert_eq!(more.pop().as_deref(), Some("next"), "max {max} {ending:?}");
                    assert_eq!(more, pieces, "max {max} {ending:?}");
                }
                assert_eq!(
                    pieces.concat(),
                    *line,
                    "max {max} {ending:?}: lost or repeated output: {pieces:?}"
                );
                let got: Vec<bool> = pieces.iter().flat_map(|p| hidden(&redactor, p)).collect();
                assert_eq!(got, want, "max {max} {ending:?}: {pieces:?}");
            }
        }
    }
}

#[tokio::test]
async fn test_capped_lines_keep_url_credentials_whole() {
    let line = "fetch https://bob:hunter2@example.com/x done";
    for max in 1..line.len() {
        let pieces = capped_pieces(line.as_bytes(), Some(max), &[]).await;
        assert_eq!(pieces.concat(), line);
        for piece in &pieces {
            let shown = crate::redactions::redact_url_userinfo(piece);
            assert!(!shown.contains("hunter2"), "max {max}: {pieces:?}");
            assert!(!shown.contains("bob"), "max {max}: {pieces:?}");
        }
    }
}

#[tokio::test]
async fn test_capped_lines_cut_a_run_of_a_self_overlapping_secret_in_linear_time() {
    // every position of the run starts an "aaaa", so each cut lands inside one
    let input = vec![b'a'; super::MAX_LINE_BYTES * 4 + 3];
    let started = std::time::Instant::now();
    let pieces = capped_pieces(&input, None, &["aaaa"]).await;
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "{elapsed:?}"
    );
    assert_eq!(pieces.concat().len(), input.len());
    // each cut moves forward past one occurrence, so pieces stay full-sized
    assert!(pieces.len() <= 5, "{} pieces", pieces.len());
    assert!(
        pieces
            .iter()
            .all(|piece| piece.len() < super::MAX_LINE_BYTES + 4)
    );
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

/// A deny_env sandbox clears the inherited environment, and says so through `inherit_env`: on
/// Windows a command with a timeout is rebuilt under a Ctrl+C group leader, which copies the
/// environment only by that flag and would otherwise hand mise's whole environment on.
#[tokio::test]
async fn test_deny_env_sandbox_marks_env_cleared() {
    use std::ffi::OsStr;

    let mut runner = super::CmdLineRunner::new("true")
        .env("KEEP", "value")
        .with_sandbox(crate::sandbox::SandboxConfig {
            deny_env: true,
            ..Default::default()
        });
    assert!(runner.inherit_env);

    runner.apply_sandbox().await.unwrap();

    assert!(!runner.inherit_env);
    assert!(
        runner.cmd.as_std().get_envs().any(|(key, value)| {
            key == OsStr::new("KEEP") && value == Some(OsStr::new("value"))
        })
    );
}

/// An env sandbox filters only the child's own environment. Without more, the child reads the
/// withheld variables back from `/proc/<mise pid>/environ` of the process that started it.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn test_deny_env_sandbox_hides_parent_environ() {
    use nix::libc;

    // Root reads any process's environ whatever its dumpable flag, so there is nothing to test.
    if nix::unistd::geteuid().is_root() {
        return;
    }
    let set_dumpable = |dumpable: libc::c_ulong| {
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_DUMPABLE, dumpable, 0, 0, 0) },
            0
        );
    };
    // Another test in this process may already have hidden it; start from the default.
    set_dumpable(1);
    let mut runner = super::CmdLineRunner::new("sh")
        .arg("-c")
        .arg("cat /proc/$PPID/environ >/dev/null 2>&1 && echo readable || echo hidden")
        .with_sandbox(crate::sandbox::SandboxConfig {
            deny_env: true,
            ..Default::default()
        });
    runner.apply_sandbox().await.unwrap();
    let output = runner.read().await;
    set_dumpable(1);
    assert_eq!(output.unwrap().trim(), "hidden");
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

/// The wrapper is looked up with the task's `PATH`, so a bare name would resolve to a
/// `sandbox-exec` in a tool's bin directory or a `_.path` entry.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn test_macos_sandbox_uses_system_sandbox_exec() {
    let mut runner = super::CmdLineRunner::new("true")
        .env("PATH", "/planted/bin:/usr/bin:/bin")
        .with_sandbox(crate::sandbox::SandboxConfig {
            deny_net: true,
            ..Default::default()
        });

    runner.apply_sandbox().await.unwrap();

    assert_eq!(runner.cmd.as_std().get_program(), "/usr/bin/sandbox-exec");
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

// The whole-run timeout sends its final SIGKILL only to trees it still sees
// alive, so an ID freed during the grace period is not signalled again.
#[cfg(unix)]
#[test]
fn alive_process_trees_drops_a_tree_once_it_is_gone() {
    use std::collections::HashSet;

    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep should start");
    let pids = HashSet::from([child.id()]);
    assert_eq!(super::alive_process_trees(&pids), pids);

    child.kill().unwrap();
    child.wait().unwrap();
    assert!(super::alive_process_trees(&pids).is_empty());
}

/// Tasks go through `apply_sandbox`, which must reject a per-host network
/// exception before it builds the sandboxed command.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn test_apply_sandbox_rejects_allow_net() {
    let mut runner =
        super::CmdLineRunner::new("true").with_sandbox(crate::sandbox::SandboxConfig {
            allow_net: vec!["registry.npmjs.org".to_string()],
            ..Default::default()
        });
    let err = runner.apply_sandbox().await.unwrap_err();
    assert!(err.to_string().contains("--allow-net=<host>"), "{err}");
}

#[test]
fn test_spawn_error_names_a_missing_program() {
    let err = super::CmdLineRunner::new("mise-no-such-program-for-test")
        .arg("install")
        .execute()
        .unwrap_err();

    let message = format!("{err:#}");
    assert!(
        message.contains("`mise-no-such-program-for-test` not found"),
        "expected the missing program to be named, got {message:?}"
    );
}

#[tokio::test]
async fn test_spawn_error_names_a_missing_program_async() {
    let err = super::CmdLineRunner::new("mise-no-such-program-for-test")
        .arg("install")
        .execute_async()
        .await
        .unwrap_err();

    let message = format!("{err:#}");
    assert!(
        message.contains("`mise-no-such-program-for-test` not found"),
        "expected the missing program to be named, got {message:?}"
    );
}

#[test]
fn test_spawn_error_blames_a_missing_directory_not_the_program() {
    let err = super::CmdLineRunner::new("sh")
        .args(["-c", "true"])
        .current_dir("/mise-no-such-dir-for-test")
        .execute()
        .unwrap_err();

    let message = format!("{err:#}");
    assert!(!message.contains("not found"), "{message:?}");
    assert!(message.contains("No such file or directory"), "{message:?}");
}

#[test]
fn test_spawn_error_names_a_missing_program_in_raw_mode() {
    let err = super::CmdLineRunner::new("mise-no-such-program-for-test")
        .raw(true)
        .execute()
        .unwrap_err();

    let message = format!("{err:#}");
    assert!(
        message.contains("`mise-no-such-program-for-test` not found"),
        "expected the missing program to be named, got {message:?}"
    );
}

// A script whose interpreter is gone fails to spawn with the same ENOENT, but
// the script itself is there, so it must not be reported as missing.
#[test]
fn test_spawn_error_does_not_blame_a_program_that_exists() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("script");
    std::fs::write(&script, "#!/mise-no-such-interpreter-for-test\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let err = super::CmdLineRunner::new(&script).execute().unwrap_err();

    let message = format!("{err:#}");
    assert!(!message.contains("not found"), "{message:?}");
    assert!(message.contains("No such file or directory"), "{message:?}");
}
