use super::*;
use std::sync::{
    Mutex, Once,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Debug)]
struct CapturedLog {
    message: String,
    sequence: u64,
    thread_id: std::thread::ThreadId,
}

struct LogCapture {
    first_sequence: u64,
    thread_id: std::thread::ThreadId,
}

struct CapturingLogger {
    next_sequence: AtomicU64,
    records: Mutex<Vec<CapturedLog>>,
}

static CAPTURING_LOGGER: CapturingLogger = CapturingLogger {
    next_sequence: AtomicU64::new(0),
    records: Mutex::new(Vec::new()),
};
static LOGGER_INIT: Once = Once::new();

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) && record.target() == "mise_sigstore::github" {
            self.records.lock().unwrap().push(CapturedLog {
                message: record.args().to_string(),
                sequence: self.next_sequence.fetch_add(1, Ordering::Relaxed),
                thread_id: std::thread::current().id(),
            });
        }
    }

    fn flush(&self) {}
}

fn capture_logs() -> LogCapture {
    LOGGER_INIT.call_once(|| {
        log::set_logger(&CAPTURING_LOGGER).unwrap();
        log::set_max_level(log::LevelFilter::Debug);
    });
    LogCapture {
        first_sequence: CAPTURING_LOGGER.next_sequence.load(Ordering::Relaxed),
        thread_id: std::thread::current().id(),
    }
}

fn captured_messages(capture: &LogCapture) -> Vec<String> {
    CAPTURING_LOGGER
        .records
        .lock()
        .unwrap()
        .iter()
        .filter(|record| {
            record.sequence >= capture.first_sequence && record.thread_id == capture.thread_id
        })
        .map(|record| record.message.clone())
        .collect()
}

fn fixture_attestation(json: &str) -> Attestation {
    serde_json::from_value(serde_json::json!({
        "bundle": serde_json::from_str::<serde_json::Value>(json).unwrap(),
    }))
    .unwrap()
}

fn invalid_attestation() -> Attestation {
    serde_json::from_value(serde_json::json!({
        "bundle": {"not": "a sigstore bundle"},
    }))
    .unwrap()
}

fn fixture_digest() -> Sha256Hash {
    Sha256Hash::from_hex("b4058dece685259910d3aba5782445996eea79dbdb3cf952a6eb81aadf0373ff")
        .unwrap()
}

#[tokio::test]
async fn github_attestation_sources_succeeds_when_one_method_fails() {
    let capture = capture_logs();
    let digest = fixture_digest();
    let sources = crate::github::verify_github_attestation_sources_for_artifact(
        Artifact::from(&digest),
        &[
            invalid_attestation(),
            fixture_attestation(include_str!(
                "../../tests/fixtures/github_build_provenance_jdx_mise.json"
            )),
        ],
        None,
    )
    .await
    .unwrap();

    assert_eq!(sources, ["jdx/mise"]);
    let messages = captured_messages(&capture);
    assert!(messages.iter().any(|message| message.contains(
        "cached GitHub attestation verification method failed; continuing with other attestations: invalid attestation JSON"
    )));
    assert!(messages.iter().any(|message| message.contains(
        "cached GitHub attestation verification succeeded overall despite 1 failed attestation method(s)"
    )));
}

#[tokio::test]
async fn github_attestation_sources_fails_when_all_methods_fail() {
    let capture = capture_logs();
    let digest = fixture_digest();
    let error = crate::github::verify_github_attestation_sources_for_artifact(
        Artifact::from(&digest),
        &[invalid_attestation()],
        None,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, AttestationError::Verification(_)));
    let messages = captured_messages(&capture);
    assert!(messages.iter().any(|message| message.contains(
        "cached GitHub attestation verification method failed; continuing with other attestations: invalid attestation JSON"
    )));
    assert!(messages.iter().any(|message| message
        == "cached GitHub attestation verification failed overall: no attestation verified"));
}

#[test]
fn attestation_diagnostic_logs_do_not_leak_secrets() {
    let capture = capture_logs();
    let error = AttestationError::Verification(
        "TUF fetch failed at https://user:password@mirror.example/root.json?token=query-token&mode=refresh; Authorization: Basic encoded-credentials; GITHUB_TOKEN=bare-token; API_KEY=\"quoted token with spaces\"; nested_api_key='another quoted token'; Bearer bearer-token".to_string(),
    );
    crate::github::log_attestation_method_failure(&error);
    let messages = captured_messages(&capture);
    assert_eq!(messages.len(), 1);
    let message = &messages[0];

    for secret in [
        "user",
        "password",
        "query-token",
        "encoded-credentials",
        "bare-token",
        "quoted token with spaces",
        "another quoted token",
        "bearer-token",
    ] {
        assert!(!message.contains(secret), "leaked {secret}: {message}");
    }
    assert!(message.contains("attestation signature verification failed"));
}
