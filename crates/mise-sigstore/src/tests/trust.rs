use super::*;

#[test]
fn github_embedded_root_does_not_load_tuf_root() {
    let mut roots = TrustRoots::default();

    roots.github_embedded_root().unwrap();

    assert!(roots.github_embedded.is_some());
    assert!(roots.github_tuf.is_none());
}

#[tokio::test]
async fn github_tuf_retry_runs_after_embedded_failure_and_can_succeed() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let tuf_calls = Arc::new(AtomicUsize::new(0));
    let calls = tuf_calls.clone();

    let result = verify_github_bundle_with_tuf_retry_after_embedded_result(
        Err(AttestationError::Verification(
            "embedded root rejected bundle".to_string(),
        )),
        || async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(tuf_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn signer_workflow_mismatch_is_not_retryable_with_tuf() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let tuf_calls = Arc::new(AtomicUsize::new(0));
    let calls = tuf_calls.clone();
    let err =
        AttestationError::WorkflowMismatch("expected 'release.yml', found 'build.yml'".to_string());

    let result =
        verify_github_bundle_with_tuf_retry_after_embedded_result(Err(err), || async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await;

    assert!(matches!(result, Err(AttestationError::WorkflowMismatch(_))));
    assert_eq!(tuf_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn github_tuf_retry_preserves_workflow_mismatch() {
    let result = verify_github_bundle_with_tuf_retry_after_embedded_result(
        Err(AttestationError::Verification(
            "embedded root rejected bundle".to_string(),
        )),
        || async {
            Err(AttestationError::WorkflowMismatch(
                "expected 'release.yml', found 'build.yml'".to_string(),
            ))
        },
    )
    .await;

    assert!(matches!(result, Err(AttestationError::WorkflowMismatch(_))));
}

#[tokio::test]
async fn github_tuf_retry_preserves_trust_root_fetch_failure() {
    let result = verify_github_bundle_with_tuf_retry_after_embedded_result(
        Err(AttestationError::Verification(
            "embedded root rejected bundle".to_string(),
        )),
        || async {
            Err(AttestationError::TrustRoot(
                "connection refused".to_string(),
            ))
        },
    )
    .await;

    let Err(AttestationError::TrustRoot(message)) = result else {
        panic!("expected a TrustRoot error, got {result:?}");
    };
    assert!(message.contains("embedded root rejected bundle"));
    assert!(message.contains("connection refused"));
}

#[tokio::test]
async fn github_tuf_retry_keeps_other_failures_as_verification_errors() {
    let result = verify_github_bundle_with_tuf_retry_after_embedded_result(
        Err(AttestationError::Verification(
            "embedded root rejected bundle".to_string(),
        )),
        || async { Err(AttestationError::Verification("bad signature".to_string())) },
    )
    .await;

    assert!(matches!(result, Err(AttestationError::Verification(_))));
}
