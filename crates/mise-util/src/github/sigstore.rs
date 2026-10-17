//! Sole mise-internal bridge to the `mise-sigstore` crate.
//!
//! Every call mise makes into `mise_sigstore` goes through this module. Callers never
//! touch the underlying crate directly and never pass a GitHub token — the token is resolved
//! internally via [`crate::github::resolve_token_for_api_url`], which walks the full chain
//! (env vars → `credential_command` → `github_tokens.toml` → gh CLI → git credential fill).
//!
//! ## Why this exists
//!
//! Before this module, three sigstore call sites (`src/backend/aqua.rs`,
//! `src/plugins/core/python.rs`, `src/plugins/core/ruby.rs`) passed
//! `crate::env::GITHUB_TOKEN.as_deref()` — env vars only — while the github backend used the
//! full chain. That asymmetry left `mise lock` issuing unauthenticated attestation requests,
//! which hit GitHub's 60/hour IP rate limit after the second run.
//!
//! Concentrating the `mise_sigstore` surface here makes the asymmetry
//! structurally impossible: wrapper signatures omit the token argument, so callers cannot
//! re-introduce the bug without first editing this file.
//!
//! ## Default API URL
//!
//! Functions that accept `api_url: Option<&str>` fall back to [`crate::github::API_URL`]
//! (`"https://api.github.com"`) when `None` is passed. The default must be a full URL so
//! [`crate::github::resolve_token_for_api_url`] can parse the host correctly; a bare hostname
//! would be silently misrouted for GitHub Enterprise Server tenants. After token resolution,
//! the API URL is routed through [`crate::http::apply_url_replacements`] so attestation
//! requests follow the same trusted proxy/cache replacements as normal mise HTTP requests.

use std::path::Path;

use mise_sigstore::sources::github::GitHubSource;
use mise_sigstore::{ArtifactRef, AttestationClient, AttestationSource, FetchParams, RetryConfig};

pub use mise_sigstore::{AttestationError, CosignIdentity, SlsaArtifact, SlsaSignerIdentity};

/// Result alias that matches `mise_sigstore`'s internal convention.
type AttestationResult<T> = std::result::Result<T, AttestationError>;

type VerificationCell = std::sync::Arc<tokio::sync::OnceCell<bool>>;
static INVOCATION_VERIFICATIONS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, VerificationCell>>,
> = std::sync::LazyLock::new(Default::default);

async fn shared_verification(
    key: String,
    verification: impl std::future::Future<Output = AttestationResult<bool>>,
) -> AttestationResult<bool> {
    let settings = mise_settings::Settings::get();
    if !settings.generate_lockfiles() || settings.force_provenance_verify() {
        return verification.await;
    }
    let cell = INVOCATION_VERIFICATIONS
        .lock()
        .unwrap()
        .entry(key)
        .or_default()
        .clone();
    cell.get_or_try_init(|| verification).await.copied()
}

#[derive(Debug)]
enum CachedAttestationVerification {
    /// These repositories (`owner/repo`) made attestations that verified.
    Verified(Vec<String>),
    Retry(Option<AttestationError>),
}

fn classify_cached_attestation_verification(
    result: AttestationResult<Vec<String>>,
) -> CachedAttestationVerification {
    match result {
        Ok(sources) if !sources.is_empty() => CachedAttestationVerification::Verified(sources),
        Ok(_) => CachedAttestationVerification::Retry(None),
        Err(err) => CachedAttestationVerification::Retry(Some(err)),
    }
}

/// Resolve a GitHub token for an optional API base URL, defaulting to [`crate::github::API_URL`].
fn resolve_token_for_wrapper(api_url: Option<&str>) -> Option<String> {
    let url = api_url.unwrap_or(crate::github::API_URL);
    crate::github::resolve_token_for_api_url(url)
}

fn routed_api_url(api_url: &str, has_credentials: bool) -> AttestationResult<String> {
    let Ok(mut url) = url::Url::parse(api_url) else {
        debug!("invalid GitHub attestation API URL, skipping url_replacements: {api_url}");
        return Ok(api_url.to_string());
    };
    let original = url.clone();
    crate::http::apply_url_replacements(&mut url);
    crate::http::ensure_secure_url_replacement(&original, &url, has_credentials)
        .map_err(|err| AttestationError::Verification(err.to_string()))?;
    if url == original {
        Ok(api_url.to_string())
    } else {
        Ok(url.to_string())
    }
}

/// Apply mise's `url_replacements` to the Sigstore public-good TUF URL.
///
/// Returns `Some(replaced)` only when a replacement actually changed the URL,
/// otherwise `None` (meaning: keep the sigstore crate's default behavior). The
/// result is pushed into `mise-sigstore` via [`mise_sigstore::set_tuf_url`] so
/// the TUF root fetch follows the same mirror as the rest of mise's traffic.
fn routed_tuf_url() -> Option<String> {
    let Ok(mut url) = url::Url::parse(mise_sigstore::DEFAULT_TUF_URL) else {
        debug!(
            "invalid Sigstore TUF URL, skipping url_replacements: {}",
            mise_sigstore::DEFAULT_TUF_URL
        );
        return None;
    };
    let original = url.clone();
    crate::http::apply_url_replacements(&mut url);
    if let Err(err) = crate::http::ensure_secure_url_replacement(&original, &url, false) {
        warn!("{err}; ignoring insecure Sigstore TUF URL replacement");
        return None;
    }
    (url != original).then(|| url.to_string())
}

/// Build a [`RetryConfig`] from mise's HTTP settings so attestation requests
/// retry and time out exactly like the rest of mise's HTTP traffic rather than
/// using a policy hardcoded in the `mise-sigstore` crate.
fn mise_retry_config() -> RetryConfig {
    let settings = mise_settings::Settings::get();
    RetryConfig {
        timeout: crate::network::http_timeout(&settings),
        retries: crate::network::http_retries(&settings).max(0) as usize,
        ..RetryConfig::default()
    }
}

fn attestation_client(api_url: &str) -> AttestationResult<AttestationClient> {
    let token = resolve_token_for_wrapper(Some(api_url));
    let base_url = routed_api_url(api_url, token.is_some())?;
    let mut builder = AttestationClient::builder()
        .base_url(&base_url)
        .retry_config(mise_retry_config());
    if let Some(token) = token.as_deref() {
        builder = builder.github_token(token);
    }
    builder.build()
}

/// Verify a GitHub artifact attestation for a file on disk.
///
/// Applies configured URL replacements to the API base URL before dispatching to
/// [`mise_sigstore::verify_github_attestation_with_base_url`].
pub async fn verify_attestation(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    expected_workflow: Option<&str>,
    api_url: Option<&str>,
    use_versions_host: bool,
) -> AttestationResult<bool> {
    if !mise_settings::Settings::get().generate_lockfiles() {
        return verify_attestation_uncached(
            artifact_path,
            owner,
            repo,
            expected_workflow,
            api_url,
            use_versions_host,
        )
        .await;
    }
    let digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
    let key = format!(
        "github:{owner}/{repo}:{digest}:{expected_workflow:?}:{api_url:?}:{use_versions_host}"
    );
    shared_verification(
        key,
        verify_attestation_uncached(
            artifact_path,
            owner,
            repo,
            expected_workflow,
            api_url,
            use_versions_host,
        ),
    )
    .await
}

async fn verify_attestation_uncached(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    expected_workflow: Option<&str>,
    api_url: Option<&str>,
    use_versions_host: bool,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    let mut digest = None;
    let mut mirror_err = None;
    if use_versions_host_for_attestations(api_url, use_versions_host) {
        let artifact_digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
        match verify_with_mirror(
            artifact_path,
            owner,
            repo,
            expected_workflow,
            None,
            &artifact_digest,
        )
        .await
        {
            MirrorVerdict::Done(result) => return result,
            MirrorVerdict::AskGithub => {}
            MirrorVerdict::RetryGithub(err) => mirror_err = Some(err),
        }
        digest = Some(artifact_digest);
    }

    let token = resolve_token_for_wrapper(api_url);
    let base_url = routed_api_url(api_url.unwrap_or(crate::github::API_URL), token.is_some())?;
    let result = if let Some(digest) = digest {
        mise_sigstore::verify_github_attestation_with_base_url_and_digest(
            mise_sigstore::GithubAttestationRequest {
                artifact_path,
                owner,
                repo,
                token: token.as_deref(),
                signer_workflow: expected_workflow,
                base_url: Some(&base_url),
                digest: Some(&digest),
                retry_config: mise_retry_config(),
            },
        )
        .await
    } else {
        mise_sigstore::verify_github_attestation_with_base_url(
            artifact_path,
            owner,
            repo,
            token.as_deref(),
            expected_workflow,
            &base_url,
            mise_retry_config(),
        )
        .await
    };
    result.map_err(|err| after_mirror_failure(err, owner, repo, mirror_err.as_ref()))
}

/// Verify a GitHub artifact attestation filtered by predicate type.
///
/// mise-versions serves every attestation for a digest, so the filter is
/// applied here, the way GitHub's `predicate_type` parameter applies it.
pub async fn verify_attestation_with_predicate_type(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    expected_workflow: Option<&str>,
    predicate_type: Option<&str>,
    api_url: Option<&str>,
    use_versions_host: bool,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    let Some(predicate_type) = predicate_type else {
        return verify_attestation(
            artifact_path,
            owner,
            repo,
            expected_workflow,
            api_url,
            use_versions_host,
        )
        .await;
    };

    let artifact_digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
    let mut mirror_err = None;
    if use_versions_host_for_attestations(api_url, use_versions_host) {
        match verify_with_mirror(
            artifact_path,
            owner,
            repo,
            expected_workflow,
            Some(predicate_type),
            &artifact_digest,
        )
        .await
        {
            MirrorVerdict::Done(result) => return result,
            MirrorVerdict::AskGithub => {}
            MirrorVerdict::RetryGithub(err) => mirror_err = Some(err),
        }
    }
    let client = attestation_client(api_url.unwrap_or(crate::github::API_URL))?;
    let attestations = client
        .fetch_attestations(FetchParams {
            owner: owner.to_string(),
            repo: Some(format!("{owner}/{repo}")),
            digest: format!("sha256:{artifact_digest}"),
            limit: 30,
            predicate_type: Some(predicate_type.to_string()),
        })
        .await
        .map_err(|err| after_mirror_failure(err, owner, repo, mirror_err.as_ref()))?;
    mise_sigstore::verify_github_attestation_with_attestations(
        artifact_path,
        &attestations,
        expected_workflow,
    )
    .await
}

/// Reason the pre-download attestation probe could not complete.
///
/// Preserved as two variants so callers can log distinct warnings for a misconfigured
/// endpoint (source creation) versus an API/network error (fetch). The original inline
/// pre-wrapper code at `src/backend/github.rs` emitted different messages for each; the
/// wrapper keeps that signal instead of flattening both into one error string.
#[derive(Debug)]
pub enum DetectError {
    /// Attestation source/client construction rejected the base URL.
    SourceCreation(AttestationError),
    /// The attestations endpoint returned an error (403 rate-limit, 5xx, network failure).
    Fetch(AttestationError),
}

impl std::fmt::Display for DetectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetectError::SourceCreation(e) => write!(f, "{e}"),
            DetectError::Fetch(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DetectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DetectError::SourceCreation(e) => Some(e),
            DetectError::Fetch(e) => Some(e),
        }
    }
}

/// Probe the GitHub attestation API for the given digest without downloading the artifact.
///
/// Returns `Ok(true)` if any attestations exist for the digest. Used at lock time to decide
/// whether `ProvenanceType::GithubAttestations` should be recorded before committing to a
/// full download + verify.
pub async fn detect_attestations(
    owner: &str,
    repo: &str,
    api_url: &str,
    digest: &str,
    use_versions_host: bool,
) -> Result<bool, DetectError> {
    let mut mirror_err = None;
    if use_versions_host_for_attestations(Some(api_url), use_versions_host) {
        match detect_with_mirror(owner, repo, digest, None).await {
            MirrorVerdict::Done(found) => return found,
            MirrorVerdict::AskGithub => {}
            MirrorVerdict::RetryGithub(err) => mirror_err = Some(err),
        }
    }

    let token = resolve_token_for_wrapper(Some(api_url));
    let base_url = routed_api_url(api_url, token.is_some()).map_err(DetectError::SourceCreation)?;
    let source = GitHubSource::with_base_url(owner, repo, token.as_deref(), &base_url)
        .map_err(DetectError::SourceCreation)?;
    let artifact_ref = ArtifactRef::from_digest(digest);
    let attestations = source
        .fetch_attestations(&artifact_ref)
        .await
        .map_err(|err| {
            DetectError::Fetch(after_mirror_failure(err, owner, repo, mirror_err.as_ref()))
        })?;
    Ok(!attestations.is_empty())
}

/// Probe the GitHub attestation API for the given digest and predicate type.
/// From mise-versions, filtered here, as in
/// [`verify_attestation_with_predicate_type`].
pub async fn detect_attestations_with_predicate_type(
    owner: &str,
    repo: &str,
    api_url: &str,
    digest: &str,
    predicate_type: Option<&str>,
    use_versions_host: bool,
) -> Result<bool, DetectError> {
    let Some(predicate_type) = predicate_type else {
        return detect_attestations(owner, repo, api_url, digest, use_versions_host).await;
    };
    let mut mirror_err = None;
    if use_versions_host_for_attestations(Some(api_url), use_versions_host) {
        match detect_with_mirror(owner, repo, digest, Some(predicate_type)).await {
            MirrorVerdict::Done(found) => return found,
            MirrorVerdict::AskGithub => {}
            MirrorVerdict::RetryGithub(err) => mirror_err = Some(err),
        }
    }

    let client = attestation_client(api_url).map_err(DetectError::SourceCreation)?;
    let digest = if digest.contains(':') {
        digest.to_string()
    } else {
        format!("sha256:{digest}")
    };
    let attestations = client
        .fetch_attestations(FetchParams {
            owner: owner.to_string(),
            repo: Some(format!("{owner}/{repo}")),
            digest,
            limit: 30,
            predicate_type: Some(predicate_type.to_string()),
        })
        .await
        .map_err(|err| {
            DetectError::Fetch(after_mirror_failure(err, owner, repo, mirror_err.as_ref()))
        })?;
    Ok(!attestations.is_empty())
}

/// Whether mise-versions saying an artifact has no attestations is taken as
/// final. A wrong "none" skips verification, and a lockfile written after it
/// records no provenance, so paranoid mode confirms it with GitHub instead,
/// at the cost of one API call per unattested artifact. "Yes" needs no
/// confirmation: it only leads to verification.
/// How long a set of attestations from mise-versions is kept on disk.
const MIRRORED_ATTESTATIONS_TTL: std::time::Duration =
    std::time::Duration::from_secs(7 * 24 * 60 * 60);

type MirroredAttestations = Option<Vec<mise_sigstore::Attestation>>;
type MirroredAttestationsCell = std::sync::Arc<tokio::sync::OnceCell<MirroredAttestations>>;
static MIRRORED_ATTESTATIONS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, MirroredAttestationsCell>>,
> = std::sync::LazyLock::new(Default::default);

/// mise-versions' attestations for `repo` at `digest`, asked for once per
/// command and kept in the cache dir.
///
/// The set for a digest only grows, and every caller verifies what it gets
/// against the artifact and asks GitHub when that fails, so a kept set
/// answers as well as a fresh one. `mise lock` asks for one digest to detect
/// provenance and again to verify it, and again for every platform sharing
/// the artifact; reinstalling a build asks again. Within a command every
/// answer is shared; only a non-empty set is written to disk, since an empty
/// one may predate the attestation. Paranoid mode always asks.
///
/// Routing is checked before either cache is read: once `url_replacements`
/// sends this repository's endpoints elsewhere, that replacement is the only
/// source, and a set kept from mise-versions earlier must not answer for it.
async fn mirrored_attestations(repo: &str, digest: &str) -> eyre::Result<MirroredAttestations> {
    if !crate::versions_host::enabled_for_github_repo(repo) {
        return Ok(None);
    }
    if mise_settings::Settings::get().paranoid.provenance() {
        return crate::versions_host::github_attestations(repo, digest).await;
    }
    let key = mirrored_attestations_key(repo, digest);
    let cell = MIRRORED_ATTESTATIONS
        .lock()
        .unwrap()
        .entry(key.clone())
        .or_default()
        .clone();
    cell.get_or_try_init(async || {
        mirrored_attestations_cache(&key)
            .get_or_try_init_async_if(
                || crate::versions_host::github_attestations(repo, digest),
                keep_mirrored_attestations,
            )
            .await
    })
    .await
    .cloned()
}

fn mirrored_attestations_key(repo: &str, digest: &str) -> String {
    format!(
        "attestations-{}",
        crate::hash::hash_to_str(&(repo.to_ascii_lowercase(), digest))
    )
}

fn mirrored_attestations_cache(key: &str) -> crate::cache::CacheManager<MirroredAttestations> {
    crate::cache::CacheManagerBuilder::new(
        crate::dirs::CACHE
            .join("github")
            .join(format!("{key}.msgpack.z")),
    )
    .with_fresh_duration(Some(MIRRORED_ATTESTATIONS_TTL))
    .build()
}

/// Only a complete set is kept: one missing a bundle is asked for again next
/// time rather than failing for a week.
fn keep_mirrored_attestations(attestations: &MirroredAttestations) -> bool {
    attestations
        .as_ref()
        .is_some_and(|a| !a.is_empty() && a.iter().all(|a| a.has_inline_bundle()))
}

/// mise-versions' attestations for `repo` at `digest`, asked for again past
/// the copy [`mirrored_attestations`] keeps, which may predate one GitHub
/// added since. A complete answer replaces that copy, on disk and for the
/// rest of this command, so later checks don't ask again.
async fn refreshed_mirrored_attestations(
    repo: &str,
    digest: &str,
) -> eyre::Result<MirroredAttestations> {
    let fresh = crate::versions_host::github_attestations(repo, digest).await?;
    if keep_mirrored_attestations(&fresh) && !mise_settings::Settings::get().paranoid.provenance() {
        let key = mirrored_attestations_key(repo, digest);
        if let Err(err) = mirrored_attestations_cache(&key).write(&fresh) {
            debug!("failed to keep refreshed attestations for {repo}: {err:#}");
        }
        MIRRORED_ATTESTATIONS.lock().unwrap().insert(
            key,
            std::sync::Arc::new(tokio::sync::OnceCell::new_with(Some(fresh.clone()))),
        );
    }
    Ok(fresh)
}

/// What mise-versions settled about an artifact's attestations: `T` is a
/// verification's result, or a probe's.
enum MirrorVerdict<T = AttestationResult<bool>> {
    /// The answer: verified, or why not.
    Done(T),
    /// GitHub has to answer: paranoid mode confirming what mise-versions
    /// said, or a repository mise-versions doesn't know (a private one) while
    /// the user has a token to read it with.
    AskGithub,
    /// mise-versions failed in a way GitHub is asked to make up for (see
    /// [`crate::github::MirrorAnswer`]). GitHub answers, and when its
    /// lookup fails too, the error names both failures; whether that fails
    /// the install is `provenance_api_failures_fatal`'s to say, as for any
    /// failed lookup.
    RetryGithub(eyre::Report),
}

/// The verdict once mise-versions failed to answer for `full`.
fn mirror_error_verdict(full: &str, err: eyre::Report) -> MirrorVerdict {
    if crate::versions_host::retries_on_github(&err) {
        return MirrorVerdict::RetryGithub(err);
    }
    MirrorVerdict::Done(Err(mirror_failure(full, &err)))
}

/// GitHub's attestation `err` for `owner/repo`, after mise-versions failed
/// with `mirror_err`: a failed lookup names both failures and stays a lookup
/// failure ([`is_api_failure`]); anything else, such as an attestation that
/// doesn't verify, is GitHub's answer alone.
fn after_mirror_failure(
    err: AttestationError,
    owner: &str,
    repo: &str,
    mirror_err: Option<&eyre::Report>,
) -> AttestationError {
    match mirror_err {
        Some(mirror_err) if is_api_failure(&err) => {
            let failed = crate::versions_host::GithubRetryFailed::new(
                &format!("GitHub attestations of {owner}/{repo}"),
                mirror_err,
                eyre::Report::new(err),
            );
            AttestationError::Api(format!("{:#}", eyre::Report::new(failed)))
        }
        _ => err,
    }
}

/// Verify `artifact_path` against mise-versions' attestations for `owner/repo`,
/// keeping only those of `predicate_type` when it is set.
///
/// Normal usage asks api.github.com, whose rate limit is 60 requests an hour
/// without a token, only when mise-versions errors
/// ([`MirrorVerdict::RetryGithub`]). A set kept from an earlier command may
/// predate an attestation GitHub added since, so one that doesn't verify is
/// asked for again before the failure stands; mise-versions refreshes its own
/// copy within half an hour of a release.
async fn verify_with_mirror(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    expected_workflow: Option<&str>,
    predicate_type: Option<&str>,
    artifact_digest: &str,
) -> MirrorVerdict {
    let full = format!("{owner}/{repo}");
    if !crate::versions_host::enabled_for_github_repo(&full) {
        // url_replacements reroutes this repository's attestations; the
        // replacement answers, not mise-versions.
        return MirrorVerdict::AskGithub;
    }
    let digest = format!("sha256:{artifact_digest}");
    for fresh in [false, true] {
        let found = if fresh {
            refreshed_mirrored_attestations(&full, &digest).await
        } else {
            mirrored_attestations(&full, &digest).await
        };
        let attestations = match found {
            Ok(Some(attestations)) => attestations,
            Ok(None) => {
                return verdict_after_attestation_404(
                    &full,
                    repo_listing(&full).await,
                    !mirror_may_answer_none(),
                    || resolve_token_for_wrapper(None).is_some(),
                );
            }
            // Paranoid mode has GitHub confirm mise-versions anyway, so it
            // asks GitHub when mise-versions can't answer.
            Err(_) if !mirror_may_answer_none() => return MirrorVerdict::AskGithub,
            Err(err) => return mirror_error_verdict(&full, err),
        };
        trace!(
            "got {} GitHub attestations for {full}@{digest} from mise-versions",
            attestations.len()
        );
        if attestations.is_empty() {
            // Not kept on disk, so already as fresh as mise-versions has it.
            return no_attestations_verdict(&full);
        }
        // Before the predicate filter, which can't read a missing bundle and
        // would pass it off as "no attestations".
        if attestations.iter().any(|a| !a.has_inline_bundle()) {
            if !fresh {
                continue;
            }
            if !mirror_may_answer_none() {
                return MirrorVerdict::AskGithub;
            }
            return MirrorVerdict::Done(Err(AttestationError::Api(format!(
                "mise-versions returned attestations for {full} without their bundles"
            ))));
        }
        let selected: Vec<_> = attestations
            .into_iter()
            .filter(|a| predicate_type.is_none_or(|p| a.matches_predicate_type(p)))
            .collect();
        if selected.is_empty() {
            if fresh {
                return no_attestations_verdict(&full);
            }
            continue;
        }
        match classify_cached_attestation_verification(
            mise_sigstore::verify_github_attestation_sources(
                artifact_path,
                &selected,
                expected_workflow,
            )
            .await,
        ) {
            // Anyone can attest any digest from their own repository;
            // GitHub's API only lists the requested repository's, but a
            // mirror could list anyone's.
            CachedAttestationVerification::Verified(sources)
                if crate::github::attested_by_repository(owner, repo, &sources).await =>
            {
                return MirrorVerdict::Done(Ok(true));
            }
            CachedAttestationVerification::Verified(sources) => {
                let made_by = sources.join(", ");
                if !mirror_may_answer_none() {
                    warn!(
                        "mise-versions returned attestations for {full} made by {made_by}; verifying with GitHub instead"
                    );
                    return MirrorVerdict::AskGithub;
                }
                return MirrorVerdict::Done(Err(AttestationError::Verification(format!(
                    "the attestations mise-versions has for {full} were made by {made_by}"
                ))));
            }
            CachedAttestationVerification::Retry(err) if fresh => {
                if !mirror_may_answer_none() {
                    debug!(
                        "mise-versions GitHub attestations did not verify for {full}; paranoid mode asks GitHub"
                    );
                    return MirrorVerdict::AskGithub;
                }
                return MirrorVerdict::Done(Err(err.unwrap_or_else(|| {
                    AttestationError::Verification(format!(
                        "no attestation mise-versions has for {full} verified this artifact"
                    ))
                })));
            }
            CachedAttestationVerification::Retry(_) => debug!(
                "GitHub attestations kept for {full} did not verify; asking mise-versions again"
            ),
        }
    }
    unreachable!("the second pass always returns")
}

/// Whether mise-versions' attestations for `owner/repo` at `digest` include
/// any of `predicate_type` (any at all when `None`), with the same verdicts
/// as [`verify_with_mirror`].
async fn detect_with_mirror(
    owner: &str,
    repo: &str,
    digest: &str,
    predicate_type: Option<&str>,
) -> MirrorVerdict<Result<bool, DetectError>> {
    let full = format!("{owner}/{repo}");
    if !crate::versions_host::enabled_for_github_repo(&full) {
        // Rerouted by url_replacements, as in `verify_with_mirror`.
        return MirrorVerdict::AskGithub;
    }
    let digest = if digest.contains(':') {
        digest.to_string()
    } else {
        format!("sha256:{digest}")
    };
    // As in `verify_with_mirror`: a set kept from earlier may predate the
    // attestation looked for, so a miss is asked of mise-versions again.
    for fresh in [false, true] {
        let found = if fresh {
            refreshed_mirrored_attestations(&full, &digest).await
        } else {
            mirrored_attestations(&full, &digest).await
        };
        let attestations = match found {
            Ok(Some(attestations)) => attestations,
            Ok(None) => {
                return detect_verdict(verdict_after_attestation_404(
                    &full,
                    repo_listing(&full).await,
                    !mirror_may_answer_none(),
                    || resolve_token_for_wrapper(None).is_some(),
                ));
            }
            Err(_) if !mirror_may_answer_none() => return MirrorVerdict::AskGithub,
            Err(err) => return detect_verdict(mirror_error_verdict(&full, err)),
        };
        trace!(
            "got {} GitHub attestation probes for {full}@{digest} from mise-versions",
            attestations.len()
        );
        // Only a predicate filter reads the bundles; whether any attestation
        // exists doesn't depend on them.
        if predicate_type.is_some() && attestations.iter().any(|a| !a.has_inline_bundle()) {
            if !fresh {
                continue;
            }
            if !mirror_may_answer_none() {
                return MirrorVerdict::AskGithub;
            }
            return MirrorVerdict::Done(Err(DetectError::Fetch(AttestationError::Api(format!(
                "mise-versions returned attestations for {full} without their bundles"
            )))));
        }
        let found = attestations
            .iter()
            .any(|a| predicate_type.is_none_or(|p| a.matches_predicate_type(p)));
        // An empty set is never kept on disk, so it is already fresh.
        if found || attestations.is_empty() || fresh {
            if !found && !mirror_may_answer_none() {
                debug!(
                    "mise-versions has no attestations for {full}@{digest}; paranoid mode asks GitHub"
                );
                return MirrorVerdict::AskGithub;
            }
            return MirrorVerdict::Done(Ok(found));
        }
    }
    unreachable!("the second pass always returns")
}

fn no_attestations_verdict(full: &str) -> MirrorVerdict {
    if mirror_may_answer_none() {
        MirrorVerdict::Done(Err(AttestationError::NoAttestations))
    } else {
        debug!("mise-versions has no GitHub attestations for {full}; paranoid mode asks GitHub");
        MirrorVerdict::AskGithub
    }
}

/// What mise-versions' release listing says about a repository whose
/// attestations it answered 404 for.
enum RepoListing {
    /// It lists the repository: a public one with no attestations for that
    /// digest.
    Listed,
    /// A 404 here too: the repository is private or gone.
    Unknown,
    /// The listing failed, so mise can't tell which.
    Failed(eyre::Report),
}

async fn repo_listing(full: &str) -> RepoListing {
    match crate::versions_host::github_releases(full, 1).await {
        Ok(Some(_)) => RepoListing::Listed,
        Ok(None) => RepoListing::Unknown,
        Err(err) => RepoListing::Failed(err),
    }
}

/// The verdict after mise-versions answered 404 for `full`'s attestations.
/// A failed listing is a failed lookup, not "no attestations": that would
/// let the install go on unchecked. It is retried on GitHub when
/// mise-versions errored ([`mirror_error_verdict`]), and
/// `provenance_api_failures_fatal` decides what a failed lookup does.
/// Paranoid mode asks GitHub instead.
fn verdict_after_attestation_404(
    full: &str,
    listing: RepoListing,
    paranoid: bool,
    has_token: impl FnOnce() -> bool,
) -> MirrorVerdict {
    match listing {
        RepoListing::Listed if paranoid => MirrorVerdict::AskGithub,
        RepoListing::Listed => MirrorVerdict::Done(Err(AttestationError::NoAttestations)),
        // Only here is a token needed, so only here is it resolved: that
        // may run the user's credential helper.
        // Paranoid mode has GitHub confirm what mise-versions can't settle.
        RepoListing::Unknown if paranoid => MirrorVerdict::AskGithub,
        RepoListing::Unknown => unknown_repo_verdict(full, has_token()),
        RepoListing::Failed(_) if paranoid => MirrorVerdict::AskGithub,
        RepoListing::Failed(err) => mirror_error_verdict(full, err),
    }
}

/// A verification's verdict for a probe: no attestations is `Ok(false)` and
/// any other failure a fetch error.
fn detect_verdict(verdict: MirrorVerdict) -> MirrorVerdict<Result<bool, DetectError>> {
    match verdict {
        MirrorVerdict::AskGithub => MirrorVerdict::AskGithub,
        MirrorVerdict::RetryGithub(err) => MirrorVerdict::RetryGithub(err),
        MirrorVerdict::Done(Err(AttestationError::NoAttestations)) => {
            MirrorVerdict::Done(Ok(false))
        }
        MirrorVerdict::Done(Err(err)) => MirrorVerdict::Done(Err(DetectError::Fetch(err))),
        MirrorVerdict::Done(Ok(found)) => MirrorVerdict::Done(Ok(found)),
    }
}

/// mise-versions doesn't serve `full`, which is private or gone. Only the
/// user's token can read a private repository's attestations; the
/// attestation client sends a token, not netrc credentials.
fn unknown_repo_verdict(full: &str, has_token: bool) -> MirrorVerdict {
    if has_token {
        debug!("mise-versions does not know {full}; asking GitHub, which may hold it privately");
        return MirrorVerdict::AskGithub;
    }
    MirrorVerdict::Done(Err(AttestationError::Api(format!(
        "GitHub repository {full} not found (404 from mise-versions.jdx.dev, which mise asks \
         before api.github.com). If the repository is private, set GITHUB_TOKEN; to ask \
         GitHub directly, set use_versions_host = false."
    ))))
}

fn mirror_failure(full: &str, err: &eyre::Report) -> AttestationError {
    AttestationError::Api(format!(
        "{} {err:#}",
        crate::github::versions_host_failure(&format!("GitHub attestations of {full}"))
    ))
}

fn mirror_may_answer_none() -> bool {
    !mise_settings::Settings::get().paranoid.provenance()
}

fn use_versions_host_for_attestations(api_url: Option<&str>, use_versions_host: bool) -> bool {
    if !use_versions_host || !crate::versions_host::enabled_for_github_metadata() {
        return false;
    }

    api_url
        .unwrap_or(crate::github::API_URL)
        .trim_end_matches('/')
        == crate::github::API_URL
}

/// Verify SLSA provenance for an already-downloaded artifact. Passthrough — no token needed.
pub async fn verify_slsa_provenance(
    artifact_path: &Path,
    provenance_path: &Path,
    min_level: u8,
    signer: SlsaSignerIdentity<'_>,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    if !mise_settings::Settings::get().generate_lockfiles() {
        return mise_sigstore::verify_slsa_provenance(
            artifact_path,
            provenance_path,
            min_level,
            signer,
        )
        .await;
    }
    let artifact_digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
    let provenance_digest = mise_sigstore::calculate_file_digest(provenance_path).await?;
    let key = format!(
        "slsa:{artifact_digest}:{provenance_digest}:{min_level}:{}:{}",
        signer.identity, signer.issuer
    );
    shared_verification(
        key,
        mise_sigstore::verify_slsa_provenance(artifact_path, provenance_path, min_level, signer),
    )
    .await
}

pub async fn verify_slsa_provenance_artifacts(
    provenance_path: &Path,
    artifacts: &[SlsaArtifact],
    min_level: u8,
    signer: SlsaSignerIdentity<'_>,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    mise_sigstore::verify_slsa_provenance_artifacts(provenance_path, artifacts, min_level, signer)
        .await
}

pub fn is_slsa_subject_mismatch(error: &AttestationError) -> bool {
    mise_sigstore::is_slsa_subject_mismatch(error)
}

pub fn is_api_failure(error: &AttestationError) -> bool {
    matches!(error, AttestationError::Api(_) | AttestationError::Http(_))
}

/// Verify a keyless Cosign signature or bundle against the expected signer
/// identity. Passthrough — no token needed.
pub async fn verify_cosign_signature(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
    identity: &CosignIdentity,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    if !mise_settings::Settings::get().generate_lockfiles() {
        return mise_sigstore::verify_cosign_signature(artifact_path, sig_or_bundle_path, identity)
            .await;
    }
    let artifact_digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
    let signature_digest = mise_sigstore::calculate_file_digest(sig_or_bundle_path).await?;
    // The identity is part of the key: a result shared across callers must
    // not let a verification with weaker constraints satisfy a stricter one.
    let key = format!("cosign:{artifact_digest}:{signature_digest}:{identity:?}");
    shared_verification(
        key,
        mise_sigstore::verify_cosign_signature(artifact_path, sig_or_bundle_path, identity),
    )
    .await
}

/// Verify a Cosign signature against a public key. Passthrough — no token needed.
pub async fn verify_cosign_signature_with_key(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
    public_key_path: &Path,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    mise_sigstore::verify_cosign_signature_with_key(
        artifact_path,
        sig_or_bundle_path,
        public_key_path,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed() -> RepoListing {
        RepoListing::Listed
    }
    fn unknown() -> RepoListing {
        RepoListing::Unknown
    }
    /// A listing that failed with `status` (`None`: no connection).
    fn failed_with(status: Option<u16>) -> RepoListing {
        RepoListing::Failed(
            crate::versions_host::VersionsHostUnavailable {
                status,
                message: format!("HTTP {status:?}"),
            }
            .into(),
        )
    }
    fn failed() -> RepoListing {
        failed_with(Some(503))
    }

    #[test]
    fn test_attestation_404_verdict_by_listing() {
        let verdict = |listing, paranoid, has_token| {
            verdict_after_attestation_404("o/r", listing, paranoid, || has_token)
        };
        // Listed: a public repository without attestations for the digest.
        assert!(matches!(
            verdict(listed(), false, false),
            MirrorVerdict::Done(Err(AttestationError::NoAttestations))
        ));
        // Unknown: private or gone; GitHub only with a token.
        assert!(matches!(
            verdict(unknown(), false, false),
            MirrorVerdict::Done(Err(AttestationError::Api(_)))
        ));
        assert!(matches!(
            verdict(unknown(), false, true),
            MirrorVerdict::AskGithub
        ));
        // Failed listing: never "no attestations". mise-versions erroring
        // (no connection, 429, 5xx) retries the lookup on GitHub, token or
        // not; a 403 (the mirror disabled) is a failed lookup.
        for listing in [failed(), failed_with(None), failed_with(Some(429))] {
            assert!(matches!(
                verdict(listing, false, false),
                MirrorVerdict::RetryGithub(_)
            ));
        }
        assert!(matches!(
            verdict(failed(), false, true),
            MirrorVerdict::RetryGithub(_)
        ));
        assert!(matches!(
            verdict(failed_with(Some(403)), false, false),
            MirrorVerdict::Done(Err(AttestationError::Api(_)))
        ));
        // Paranoid mode asks GitHub whenever mise-versions can't settle it.
        assert!(matches!(
            verdict(listed(), true, false),
            MirrorVerdict::AskGithub
        ));
        // An unlisted repository without a token: paranoid mode still asks GitHub.
        assert!(matches!(
            verdict(unknown(), true, false),
            MirrorVerdict::AskGithub
        ));
        assert!(matches!(
            verdict(failed(), true, false),
            MirrorVerdict::AskGithub
        ));
    }

    #[test]
    fn test_attestation_404_resolves_a_token_only_for_an_unknown_repo() {
        // Resolving a token may run a credential helper; a listed repository
        // or a failed listing never needs one.
        for listing in [listed(), failed()] {
            let _ = verdict_after_attestation_404("o/r", listing, false, || {
                panic!("token resolved for a repository mise-versions can answer for")
            });
        }
    }

    #[test]
    fn test_attestation_404_probe_by_listing() {
        let probe = |listing, paranoid, has_token| {
            detect_verdict(verdict_after_attestation_404(
                "o/r",
                listing,
                paranoid,
                || has_token,
            ))
        };
        assert!(matches!(
            probe(listed(), false, false),
            MirrorVerdict::Done(Ok(false))
        ));
        assert!(matches!(
            probe(unknown(), false, false),
            MirrorVerdict::Done(Err(DetectError::Fetch(AttestationError::Api(_))))
        ));
        assert!(matches!(
            probe(unknown(), false, true),
            MirrorVerdict::AskGithub
        ));
        assert!(matches!(
            probe(failed(), false, false),
            MirrorVerdict::RetryGithub(_)
        ));
        assert!(matches!(
            probe(failed_with(Some(403)), false, false),
            MirrorVerdict::Done(Err(DetectError::Fetch(AttestationError::Api(_))))
        ));
        assert!(matches!(
            probe(listed(), true, false),
            MirrorVerdict::AskGithub
        ));
        assert!(matches!(
            probe(failed(), true, false),
            MirrorVerdict::AskGithub
        ));
    }

    #[test]
    fn test_github_failure_after_mirror_failure_names_both() {
        let mirror_err: eyre::Report = crate::versions_host::VersionsHostUnavailable {
            status: Some(503),
            message: "HTTP status server error (503 Service Unavailable)".to_string(),
        }
        .into();
        let err = after_mirror_failure(
            AttestationError::Api("HTTP 403 rate limit exceeded".to_string()),
            "o",
            "r",
            Some(&mirror_err),
        );
        // Still a failed lookup, for provenance_api_failures_fatal.
        assert!(is_api_failure(&err));
        let message = err.to_string();
        assert!(message.contains("mise-versions.jdx.dev"), "{message}");
        assert!(message.contains("503 Service Unavailable"), "{message}");
        assert!(message.contains("api.github.com"), "{message}");
        assert!(message.contains("rate limit exceeded"), "{message}");
        // GitHub's own answer stands alone.
        assert!(matches!(
            after_mirror_failure(
                AttestationError::NoAttestations,
                "o",
                "r",
                Some(&mirror_err)
            ),
            AttestationError::NoAttestations
        ));
        assert!(matches!(
            after_mirror_failure(AttestationError::Api("x".into()), "o", "r", None),
            AttestationError::Api(m) if m == "x"
        ));
    }
    use crate::env as mise_env;
    use confique::Layer;

    const TOKEN_ENV_VARS: &[&str] = &[
        "MISE_GITHUB_TOKEN",
        "GITHUB_API_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_ENTERPRISE_TOKEN",
    ];

    struct SettingsGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl SettingsGuard {
        fn new(replacements: Option<indexmap::IndexMap<String, String>>) -> Self {
            Self::with_versions_host(replacements, None)
        }

        fn with_versions_host(
            replacements: Option<indexmap::IndexMap<String, String>>,
            use_versions_host: Option<bool>,
        ) -> Self {
            let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
            let mut settings = mise_settings::SettingsPartial::empty();
            settings.url_replacements = replacements;
            settings.use_versions_host = use_versions_host;
            crate::testing::reset_settings(Some(settings));
            Self { _lock: lock }
        }
    }

    impl Drop for SettingsGuard {
        fn drop(&mut self) {
            crate::testing::reset_settings(None);
        }
    }

    /// RAII guard: snapshots the tracked token env vars on construction, clears them for the
    /// test body, and restores the original values on drop — including when a test panics.
    struct TokenEnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl TokenEnvGuard {
        fn new() -> Self {
            let saved: Vec<_> = TOKEN_ENV_VARS
                .iter()
                .map(|name| (*name, std::env::var(name).ok()))
                .collect();
            for name in TOKEN_ENV_VARS {
                mise_env::remove_var(name);
            }
            Self { saved }
        }
    }

    impl Drop for TokenEnvGuard {
        fn drop(&mut self) {
            for (name, value) in std::mem::take(&mut self.saved) {
                match value {
                    Some(v) => mise_env::set_var(name, v),
                    None => mise_env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn test_resolve_token_wrapper_uses_env_var_with_default_url() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::github::TEST_ENV_LOCK);
        let _env = TokenEnvGuard::new();
        mise_env::set_var("GITHUB_TOKEN", "ghp_wrapper_default");

        let resolved = resolve_token_for_wrapper(None);
        assert_eq!(
            resolved.as_deref(),
            Some("ghp_wrapper_default"),
            "env var should flow through the wrapper with the default API URL"
        );
    }

    #[test]
    fn test_resolve_token_wrapper_uses_env_var_with_explicit_api_url() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::github::TEST_ENV_LOCK);
        let _env = TokenEnvGuard::new();
        mise_env::set_var("MISE_GITHUB_TOKEN", "ghp_explicit_api");

        let resolved = resolve_token_for_wrapper(Some(crate::github::API_URL));
        assert_eq!(
            resolved.as_deref(),
            Some("ghp_explicit_api"),
            "explicit api.github.com URL should resolve identically to the default"
        );
    }

    #[test]
    fn test_resolve_token_wrapper_respects_enterprise_api_url() {
        let _lock = crate::testing::lock_ignoring_poison(&crate::github::TEST_ENV_LOCK);
        let _env = TokenEnvGuard::new();
        mise_env::set_var("GITHUB_TOKEN", "ghp_public_only");
        mise_env::set_var("MISE_GITHUB_ENTERPRISE_TOKEN", "ghp_enterprise_only");

        // An enterprise API URL must parse and route to the enterprise token, proving the
        // wrapper passes a full URL (not a bare hostname) to `resolve_token_for_api_url`.
        let resolved =
            resolve_token_for_wrapper(Some("https://github.enterprise.example.com/api/v3"));
        assert_eq!(
            resolved.as_deref(),
            Some("ghp_enterprise_only"),
            "enterprise api_url should resolve the enterprise token, not the public one"
        );

        // And the default (None) must still pick the public token.
        let resolved_default = resolve_token_for_wrapper(None);
        assert_eq!(
            resolved_default.as_deref(),
            Some("ghp_public_only"),
            "default api_url should still resolve the public token"
        );
    }

    /// Guard that seeds the `github_tokens.toml` test override and clears it on drop.
    struct TokensFileOverrideGuard;

    impl TokensFileOverrideGuard {
        fn set(host: &str, token: &str) -> Self {
            let mut map = std::collections::HashMap::new();
            map.insert(host.to_string(), token.to_string());
            *crate::github::test_support::TOKENS_FILE_OVERRIDE
                .write()
                .unwrap() = Some(map);
            Self
        }
    }

    impl Drop for TokensFileOverrideGuard {
        fn drop(&mut self) {
            *crate::github::test_support::TOKENS_FILE_OVERRIDE
                .write()
                .unwrap() = None;
        }
    }

    #[test]
    fn test_resolve_token_wrapper_uses_github_tokens_toml_source() {
        // Proves the wrapper delegates all the way through `resolve_token` to the
        // non-env-var sources — here, the `github_tokens.toml` path (source #4). Without
        // this, a future regression could short-circuit on env vars and silently pass all
        // prior tests.
        let _lock = crate::testing::lock_ignoring_poison(&crate::github::TEST_ENV_LOCK);
        let _env = TokenEnvGuard::new();
        let _tokens_file = TokensFileOverrideGuard::set("github.com", "ghp_from_tokens_file");

        let resolved = resolve_token_for_wrapper(None);
        assert_eq!(
            resolved.as_deref(),
            Some("ghp_from_tokens_file"),
            "wrapper should resolve tokens from github_tokens.toml when env vars are empty"
        );
    }

    #[test]
    fn test_is_api_failure_excludes_malformed_payloads() {
        assert!(is_api_failure(&AttestationError::Api(
            "rate limited".into()
        )));
        assert!(!is_api_failure(&AttestationError::Json(
            serde_json::from_str::<serde_json::Value>("{").unwrap_err()
        )));
    }

    #[tokio::test]
    async fn test_mirrored_attestations_survive_the_disk_cache() {
        // The bundle is arbitrary JSON; it has to come back out of the
        // msgpack cache file as the same value for verification to use it.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("attestations.msgpack.z");
        let bundle = serde_json::json!({
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": {"tlogEntries": [{"logIndex": "1", "integratedTime": 2}]},
            "dsseEnvelope": {"payload": "e30=", "signatures": [{"sig": "AA=="}]},
        });
        let attestation: mise_sigstore::Attestation =
            serde_json::from_value(serde_json::json!({"bundle": bundle, "bundle_url": null}))
                .unwrap();
        let write = crate::cache::CacheManagerBuilder::new(&path)
            .with_fresh_duration(Some(MIRRORED_ATTESTATIONS_TTL))
            .build::<MirroredAttestations>();
        write.write(&Some(vec![attestation])).unwrap();

        let read = crate::cache::CacheManagerBuilder::new(&path)
            .with_fresh_duration(Some(MIRRORED_ATTESTATIONS_TTL))
            .build::<MirroredAttestations>();
        let cached = read
            .get_or_try_init_async_if(
                async || -> eyre::Result<MirroredAttestations> {
                    panic!("a fresh cached set must not be fetched again")
                },
                |a| a.as_ref().is_some_and(|a| !a.is_empty()),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cached.len(), 1);
        assert!(cached[0].has_inline_bundle());
        assert_eq!(
            serde_json::to_value(&cached[0]).unwrap()["bundle"],
            bundle,
            "the bundle changed on its way through the cache"
        );
    }

    #[test]
    fn test_cached_attestation_verification_accepts_only_success() {
        assert!(matches!(
            classify_cached_attestation_verification(Ok(vec!["jdx/mise".to_string()])),
            CachedAttestationVerification::Verified(sources) if sources == ["jdx/mise"]
        ));
        assert!(matches!(
            classify_cached_attestation_verification(Ok(vec![])),
            CachedAttestationVerification::Retry(None)
        ));

        let outcome = classify_cached_attestation_verification(Err(
            AttestationError::WorkflowMismatch("stale cached signer".to_string()),
        ));
        assert!(matches!(
            outcome,
            CachedAttestationVerification::Retry(Some(AttestationError::WorkflowMismatch(_)))
        ));
    }

    #[test]
    fn test_routed_api_url_applies_simple_url_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "https://github-proxy.example.com".to_string(),
        }));

        let routed = routed_api_url(crate::github::API_URL, true).unwrap();

        assert_eq!(routed, "https://github-proxy.example.com/");
    }

    #[test]
    fn test_routed_api_url_applies_regex_url_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "regex:^https://api\\.github\\.com".to_string() => "https://github-proxy.example.com/api".to_string(),
        }));

        let routed = routed_api_url(crate::github::API_URL, true).unwrap();

        assert_eq!(routed, "https://github-proxy.example.com/api/");
    }

    #[test]
    fn test_routed_api_url_keeps_original_url_without_replacement() {
        let _settings = SettingsGuard::new(None);

        let routed = routed_api_url(crate::github::API_URL, true).unwrap();

        assert_eq!(routed, crate::github::API_URL);
    }

    #[test]
    fn test_routed_api_url_rejects_credential_downgrade() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "http://github-proxy.example.com".to_string(),
        }));

        let err = routed_api_url(crate::github::API_URL, true).unwrap_err();

        assert!(err.to_string().contains("refusing to send credentials"));
        assert!(!is_api_failure(&err));
    }

    #[test]
    fn test_routed_api_url_rejects_userinfo_downgrade_without_token() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string()
                => "http://user:password@github-proxy.example.com".to_string(),
        }));

        let err = routed_api_url(crate::github::API_URL, false).unwrap_err();

        assert!(err.to_string().contains("refusing to send credentials"));
        assert!(!err.to_string().contains("password"));
        assert!(!is_api_failure(&err));
    }

    #[test]
    fn test_routed_tuf_url_applies_url_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://tuf-repo-cdn.sigstore.dev".to_string()
                => "https://tuf-mirror.example.com".to_string(),
        }));

        let routed = routed_tuf_url();

        assert_eq!(routed.as_deref(), Some("https://tuf-mirror.example.com/"));
    }

    #[test]
    fn test_routed_tuf_url_none_without_replacement() {
        let _settings = SettingsGuard::new(None);

        assert_eq!(routed_tuf_url(), None);
    }

    #[test]
    fn test_routed_tuf_url_ignores_userinfo_downgrade() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://tuf-repo-cdn.sigstore.dev".to_string()
                => "http://user:password@tuf-mirror.example.com".to_string(),
        }));

        assert_eq!(routed_tuf_url(), None);
    }

    #[test]
    fn test_use_versions_host_for_attestations_respects_setting() {
        let _settings = SettingsGuard::with_versions_host(None, Some(false));

        assert!(!use_versions_host_for_attestations(
            Some(crate::github::API_URL),
            true
        ));
    }

    #[test]
    fn test_use_versions_host_for_attestations_defers_to_url_replacements() {
        // GitHub routed elsewhere (a proxy, a mirror, a fixture): the
        // replacement is the only source of GitHub data.
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "https://github-proxy.example.com".to_string(),
        }));

        assert!(!use_versions_host_for_attestations(
            Some(crate::github::API_URL),
            true
        ));
    }

    #[tokio::test]
    async fn test_mirrored_attestations_defer_to_a_repository_url_replacement() {
        // A set this command already got from mise-versions, then a rule that
        // reroutes only this repository: the kept set must not answer for it.
        let repo = "acme/attested-tool";
        let digest = "sha256:0";
        let key = format!(
            "attestations-{}",
            crate::hash::hash_to_str(&(repo.to_ascii_lowercase(), digest))
        );
        let attestation: mise_sigstore::Attestation =
            serde_json::from_value(serde_json::json!({"bundle": {}, "bundle_url": null})).unwrap();
        let cell = MirroredAttestationsCell::default();
        cell.set(Some(vec![attestation])).unwrap();
        MIRRORED_ATTESTATIONS.lock().unwrap().insert(key, cell);

        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com/repos/acme/attested-tool/".to_string()
                => "https://github-proxy.example.com/repos/acme/attested-tool/".to_string(),
        }));
        assert!(mirrored_attestations(repo, digest).await.unwrap().is_none());
    }

    #[test]
    fn test_path_specific_url_replacements_are_detected_per_repository() {
        // A rule that only reroutes one repository's releases leaves the bare
        // GitHub URLs alone, but still takes that repository off mise-versions.
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com/repos/acme/tool/".to_string()
                => "https://github-proxy.example.com/repos/acme/tool/".to_string(),
        }));

        assert!(!crate::versions_host::github_is_url_replaced(None));
        assert!(crate::versions_host::github_is_url_replaced(Some(
            "acme/tool"
        )));
        assert!(!crate::versions_host::github_is_url_replaced(Some(
            "acme/other"
        )));
    }

    #[test]
    fn test_exact_release_url_replacements_are_honored() {
        // A rule for one release's API URL takes that release off mise-versions
        // and out of the mirrored cache entry, and leaves other tags alone.
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com/repos/acme/tool/releases/tags/v2.0.0".to_string()
                => "https://github-proxy.example.com/acme/tool/v2.0.0".to_string(),
        }));

        assert!(crate::versions_host::github_release_is_url_replaced(
            "acme/tool",
            "v2.0.0"
        ));
        assert!(!crate::versions_host::github_release_is_url_replaced(
            "acme/tool",
            "v1.0.0"
        ));
        assert!(
            crate::github::release_cache_key_for_test("acme/tool", "v2.0.0").ends_with("direct")
        );
        assert!(
            crate::github::release_cache_key_for_test("acme/tool", "v1.0.0").ends_with("hosted-2")
        );
    }

    #[test]
    fn test_download_only_url_replacements_keep_mise_versions() {
        // Mirroring release downloads (the documented Artifactory example)
        // doesn't change where release metadata comes from.
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            r"regex:^https://github\.com/([^/]+)/([^/]+)/releases/download/(.+)".to_string()
                => "https://hub.example.com/artifactory/github/$1/$2/$3".to_string(),
        }));

        assert!(!crate::versions_host::github_is_url_replaced(Some(
            "acme/tool"
        )));
    }

    #[test]
    fn test_use_versions_host_for_attestations_respects_caller_gate() {
        let _settings = SettingsGuard::with_versions_host(None, Some(true));

        assert!(!use_versions_host_for_attestations(
            Some(crate::github::API_URL),
            false
        ));
        assert!(use_versions_host_for_attestations(
            Some(crate::github::API_URL),
            true
        ));
    }
}
