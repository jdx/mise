use super::*;

pub struct GithubAttestationRequest<'a> {
    pub artifact_path: &'a Path,
    pub owner: &'a str,
    pub repo: &'a str,
    pub token: Option<&'a str>,
    pub signer_workflow: Option<&'a str>,
    pub base_url: Option<&'a str>,
    pub digest: Option<&'a str>,
    pub retry_config: RetryConfig,
}

pub async fn verify_github_attestation(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    token: Option<&str>,
    signer_workflow: Option<&str>,
    retry_config: RetryConfig,
) -> Result<bool> {
    verify_github_attestation_inner(GithubAttestationRequest {
        artifact_path,
        owner,
        repo,
        token,
        signer_workflow,
        base_url: None,
        digest: None,
        retry_config,
    })
    .await
}

pub async fn verify_github_attestation_with_base_url(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    token: Option<&str>,
    signer_workflow: Option<&str>,
    base_url: &str,
    retry_config: RetryConfig,
) -> Result<bool> {
    verify_github_attestation_inner(GithubAttestationRequest {
        artifact_path,
        owner,
        repo,
        token,
        signer_workflow,
        base_url: Some(base_url),
        digest: None,
        retry_config,
    })
    .await
}

pub async fn verify_github_attestation_with_base_url_and_digest(
    request: GithubAttestationRequest<'_>,
) -> Result<bool> {
    verify_github_attestation_inner(request).await
}

pub async fn verify_github_attestation_with_attestations(
    artifact_path: &Path,
    attestations: &[Attestation],
    signer_workflow: Option<&str>,
) -> Result<bool> {
    if attestations.is_empty() {
        return Err(AttestationError::NoAttestations);
    }

    let artifact = tokio::fs::read(artifact_path).await?;
    let mut trust_roots = TrustRoots::default();
    verify_attestation_bundles(attestations, &artifact, signer_workflow, &mut trust_roots).await
}

/// Verify attestations that did not come from GitHub's attestations API for
/// the repository (a cache or mirror, say), and return the repository each
/// verified attestation vouches for, as `owner/repo`.
///
/// `/repos/{owner}/{repo}/attestations` only lists attestations made in that
/// repository. Held attestations carry no such guarantee — any repository can
/// attest any digest — so the caller must check the returned repositories.
/// An attestation that verifies but names no repository is not returned.
pub async fn verify_github_attestation_sources(
    artifact_path: &Path,
    attestations: &[Attestation],
    signer_workflow: Option<&str>,
) -> Result<Vec<String>> {
    if attestations.is_empty() {
        return Err(AttestationError::NoAttestations);
    }

    let artifact = tokio::fs::read(artifact_path).await?;
    verify_github_attestation_sources_for_artifact(
        Artifact::from(artifact.as_slice()),
        attestations,
        signer_workflow,
    )
    .await
}

pub(crate) async fn verify_github_attestation_sources_for_artifact(
    artifact: Artifact<'_>,
    attestations: &[Attestation],
    signer_workflow: Option<&str>,
) -> Result<Vec<String>> {
    let mut trust_roots = TrustRoots::default();
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    let mut trust_root_failures = 0;
    for attestation in attestations {
        let Some(bundle_value) = &attestation.bundle else {
            continue;
        };
        let bundle = match serde_json::from_value::<Bundle>(bundle_value.clone()) {
            Ok(bundle) => bundle,
            Err(e) => {
                let error_message = e.to_string();
                let error = AttestationError::Json(e);
                log_attestation_method_failure(&error);
                errors.push(error_message);
                continue;
            }
        };
        match verify_bundle_with_trust_roots(
            artifact.clone(),
            &bundle,
            signer_workflow,
            &mut trust_roots,
        )
        .await
        {
            Ok(()) => match bundle_source_repository(&bundle) {
                Some(repository) => sources.push(repository),
                None => {
                    const ERROR: &str = "verified attestation names no source repository";
                    log::debug!(
                        "GitHub attestation verification method failed; continuing with other attestations: {ERROR}"
                    );
                    errors.push(ERROR.to_string());
                }
            },
            Err(error) => {
                log_attestation_method_failure(&error);
                if matches!(error, AttestationError::TrustRoot(_)) {
                    trust_root_failures += 1;
                }
                errors.push(error.to_string());
            }
        }
    }

    if sources.is_empty() {
        log::debug!(
            "cached GitHub attestation verification failed overall: no attestation verified"
        );
        return Err(AttestationError::Verification(join_error_strings(
            errors,
            || "No valid attestations found".to_string(),
        )));
    }
    if !errors.is_empty() {
        log::debug!(
            "cached GitHub attestation verification succeeded overall despite {} failed attestation method(s)",
            errors.len()
        );
    }
    warn_if_trust_root_unreachable(trust_root_failures);
    Ok(sources)
}

/// Verification passes when any one attestation verifies, so attestations that
/// were skipped because their trust root could not be loaded would otherwise
/// go unnoticed. Warn once rather than per attestation.
pub(crate) fn warn_if_trust_root_unreachable(skipped: usize) {
    if skipped > 0 {
        log::warn!(
            "{skipped} GitHub attestation(s) were not checked because a TUF trust root could not be loaded; verification passed on the remaining attestations. This usually means the Sigstore or GitHub TUF repository (tuf-repo-cdn.sigstore.dev and tuf-repo.github.com by default) is unreachable, or a configured mirror served invalid metadata; run with -v for details"
        );
    }
}

pub(crate) fn log_attestation_method_failure(error: &AttestationError) {
    log::debug!(
        "GitHub attestation verification method failed; continuing with other attestations: {}",
        error.diagnostic_summary()
    );
}

pub(crate) async fn verify_github_attestation_inner(
    request: GithubAttestationRequest<'_>,
) -> Result<bool> {
    let mut builder = AttestationClient::builder().retry_config(request.retry_config);
    if let Some(token) = request.token {
        builder = builder.github_token(token);
    }
    if let Some(base_url) = request.base_url {
        builder = builder.base_url(base_url);
    }
    let client = builder.build()?;
    let digest = match request.digest {
        Some(digest) => digest.to_string(),
        None => calculate_file_digest(request.artifact_path).await?,
    };
    let attestations = client
        .fetch_attestations(FetchParams {
            owner: request.owner.to_string(),
            repo: Some(format!("{}/{}", request.owner, request.repo)),
            digest: format!("sha256:{digest}"),
            limit: 30,
            predicate_type: None,
        })
        .await?;

    if attestations.is_empty() {
        return Err(AttestationError::NoAttestations);
    }

    let artifact = tokio::fs::read(request.artifact_path).await?;
    let mut trust_roots = TrustRoots::default();
    verify_attestation_bundles(
        &attestations,
        &artifact,
        request.signer_workflow,
        &mut trust_roots,
    )
    .await
}
