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
    let mut trust_roots = TrustRoots::default();
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    for attestation in attestations {
        let Some(bundle_value) = &attestation.bundle else {
            continue;
        };
        let bundle = match serde_json::from_value::<Bundle>(bundle_value.clone()) {
            Ok(bundle) => bundle,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };
        match verify_bundle_with_trust_roots(
            Artifact::from(artifact.as_slice()),
            &bundle,
            signer_workflow,
            &mut trust_roots,
        )
        .await
        {
            Ok(()) => match bundle_source_repository(&bundle) {
                Some(repository) => sources.push(repository),
                None => errors.push("verified attestation names no source repository".to_string()),
            },
            Err(e) => errors.push(e.to_string()),
        }
    }

    if sources.is_empty() {
        return Err(AttestationError::Verification(join_error_strings(
            errors,
            || "No valid attestations found".to_string(),
        )));
    }
    Ok(sources)
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
