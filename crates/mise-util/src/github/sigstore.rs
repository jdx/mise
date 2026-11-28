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
//! requests follow the same trusted proxy/cache replacements as normal mise HTTP requests. A
//! replacement that changes the host drops the token, which was resolved for the original host.

use std::path::Path;

use mise_sigstore::sources::github::GitHubSource;
use mise_sigstore::{ArtifactRef, AttestationClient, AttestationSource, FetchParams, RetryConfig};

pub use mise_sigstore::{
    Attestation, AttestationError, CosignIdentity, SlsaArtifact, SlsaSignerIdentity,
};

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

/// Apply `url_replacements` to the attestation API base URL with the credential rules
/// [`crate::http`] applies to any rewritten request: credentials resolved for the original
/// host are dropped when the rule changes the host, then an HTTPS-to-HTTP downgrade is
/// refused for whatever credentials are left.
///
/// `token` is the token resolved for `api_url`. Returns the URL to use and the token that
/// may still be sent to it.
fn routed_api_url(
    api_url: &str,
    token: Option<String>,
) -> AttestationResult<(String, Option<String>)> {
    let Ok(mut url) = url::Url::parse(api_url) else {
        debug!("invalid GitHub attestation API URL, skipping url_replacements: {api_url}");
        return Ok((api_url.to_string(), token));
    };
    let original = url.clone();
    crate::http::apply_url_replacements(&mut url);
    // mise-sigstore builds the Authorization header itself, so the token is scoped here;
    // `clear_cross_host_credentials` only has inherited userinfo left to strip.
    let token = token.filter(|_| url.host() == original.host());
    crate::http::clear_cross_host_credentials(
        &mut reqwest::header::HeaderMap::new(),
        &original,
        &mut url,
    );
    crate::http::ensure_secure_url_replacement(&original, &url, token.is_some())
        .map_err(|err| AttestationError::Verification(err.to_string()))?;
    if url == original {
        Ok((api_url.to_string(), token))
    } else {
        Ok((url.to_string(), token))
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
    let (base_url, token) = routed_api_url(api_url, resolve_token_for_wrapper(Some(api_url)))?;
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
/// [`mise_sigstore::verify_github_attestation_with_base_url_and_digest`].
///
/// `tags` are the tags the release that `artifact_path` was downloaded for may
/// have; an attestation that names another tag does not count. Empty skips
/// that check.
pub async fn verify_attestation(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    tags: &[String],
    expected_workflow: Option<&str>,
    api_url: Option<&str>,
    use_versions_host: bool,
) -> AttestationResult<bool> {
    if !mise_settings::Settings::get().generate_lockfiles() {
        return verify_attestation_uncached(
            artifact_path,
            owner,
            repo,
            tags,
            expected_workflow,
            api_url,
            use_versions_host,
        )
        .await;
    }
    let digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
    let key = format!(
        "github:{owner}/{repo}:{tags:?}:{digest}:{expected_workflow:?}:{api_url:?}:{use_versions_host}"
    );
    shared_verification(
        key,
        verify_attestation_uncached(
            artifact_path,
            owner,
            repo,
            tags,
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
    tags: &[String],
    expected_workflow: Option<&str>,
    api_url: Option<&str>,
    use_versions_host: bool,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    let mut digest = None;
    let mut mirror_err = None;
    let mut rejected = None;
    let mut confirming = None;
    if use_versions_host_for_attestations(api_url, use_versions_host) {
        let artifact_digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
        match verify_with_mirror(
            artifact_path,
            owner,
            repo,
            tags,
            expected_workflow,
            None,
            &artifact_digest,
        )
        .await
        {
            MirrorVerdict::Done(result) => return result,
            MirrorVerdict::AskGithub => {}
            MirrorVerdict::ConfirmWithGithub(err) => {
                rejected = Some(err);
                confirming = Some(KeptRejection::new(
                    &format!("{owner}/{repo}"),
                    &artifact_digest,
                    tags,
                    expected_workflow,
                    None,
                ));
            }
            MirrorVerdict::RetryGithub(err) => mirror_err = Some(err),
        }
        digest = Some(artifact_digest);
    }

    let (base_url, token) = routed_api_url(
        api_url.unwrap_or(crate::github::API_URL),
        resolve_token_for_wrapper(api_url),
    )?;
    let result = mise_sigstore::verify_github_attestation_with_base_url_and_digest(
        mise_sigstore::GithubAttestationRequest {
            artifact_path,
            owner,
            repo,
            tags,
            token: token.as_deref(),
            signer_workflow: expected_workflow,
            base_url: Some(&base_url),
            digest: digest.as_deref(),
            retry_config: mise_retry_config(),
        },
    )
    .await;
    let result = accept_renamed_repository(owner, repo, api_url, result).await;
    if let Some(kept) = &confirming {
        kept.settle(&result);
    }
    result.map_err(|err| {
        rejection_stands(
            after_mirror_failure(err, owner, repo, mirror_err.as_ref()),
            rejected,
        )
    })
}

/// Attestations name the repository as it was when they were made, so after
/// a rename or transfer GitHub lists some under a name that was not
/// requested. Accept those when GitHub confirms both names are one
/// repository. [`crate::github::canonical_repo`] refuses to answer when
/// `url_replacements` reroutes the API: the host that picked the
/// attestations must not also vouch for the rename.
async fn accept_renamed_repository(
    owner: &str,
    repo: &str,
    api_url: Option<&str>,
    result: AttestationResult<bool>,
) -> AttestationResult<bool> {
    let on_github = api_url.is_none_or(|url| url.trim_end_matches('/') == crate::github::API_URL);
    match result {
        Err(AttestationError::RepositoryMismatch { found, .. })
            if on_github && crate::github::attested_by_repository(owner, repo, &found).await =>
        {
            Ok(true)
        }
        result => result,
    }
}

/// What [`verify_attestation_with_predicate_type`] verifies: the arguments of
/// [`verify_attestation`], and the predicate type to keep.
pub struct PredicateAttestationRequest<'a> {
    pub artifact_path: &'a Path,
    pub owner: &'a str,
    pub repo: &'a str,
    pub tags: &'a [String],
    pub expected_workflow: Option<&'a str>,
    pub predicate_type: Option<&'a str>,
    pub api_url: Option<&'a str>,
    pub use_versions_host: bool,
}

/// Verify a GitHub artifact attestation filtered by predicate type.
///
/// mise-versions serves every attestation for a digest, so the filter is
/// applied here, the way GitHub's `predicate_type` parameter applies it.
pub async fn verify_attestation_with_predicate_type(
    request: PredicateAttestationRequest<'_>,
) -> AttestationResult<bool> {
    let PredicateAttestationRequest {
        artifact_path,
        owner,
        repo,
        tags,
        expected_workflow,
        predicate_type,
        api_url,
        use_versions_host,
    } = request;
    mise_sigstore::set_tuf_url(routed_tuf_url());
    let Some(predicate_type) = predicate_type else {
        return verify_attestation(
            artifact_path,
            owner,
            repo,
            tags,
            expected_workflow,
            api_url,
            use_versions_host,
        )
        .await;
    };

    let artifact_digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
    let mut mirror_err = None;
    let mut rejected = None;
    let mut confirming = None;
    if use_versions_host_for_attestations(api_url, use_versions_host) {
        match verify_with_mirror(
            artifact_path,
            owner,
            repo,
            tags,
            expected_workflow,
            Some(predicate_type),
            &artifact_digest,
        )
        .await
        {
            MirrorVerdict::Done(result) => return result,
            MirrorVerdict::AskGithub => {}
            MirrorVerdict::ConfirmWithGithub(err) => {
                rejected = Some(err);
                confirming = Some(KeptRejection::new(
                    &format!("{owner}/{repo}"),
                    &artifact_digest,
                    tags,
                    expected_workflow,
                    Some(predicate_type),
                ));
            }
            MirrorVerdict::RetryGithub(err) => mirror_err = Some(err),
        }
    }
    let client = attestation_client(api_url.unwrap_or(crate::github::API_URL))?;
    let result = match client
        .fetch_attestations(FetchParams {
            owner: owner.to_string(),
            repo: Some(format!("{owner}/{repo}")),
            digest: format!("sha256:{artifact_digest}"),
            limit: 30,
            predicate_type: Some(predicate_type.to_string()),
        })
        .await
    {
        Ok(attestations) => {
            let result = mise_sigstore::verify_github_attestation_with_attestations(
                artifact_path,
                &attestations,
                owner,
                repo,
                tags,
                expected_workflow,
            )
            .await;
            accept_renamed_repository(owner, repo, api_url, result).await
        }
        Err(err) => Err(rejection_stands(
            after_mirror_failure(err, owner, repo, mirror_err.as_ref()),
            rejected,
        )),
    };
    if let Some(kept) = &confirming {
        kept.settle(&result);
    }
    result
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
    let mut rejected = None;
    if use_versions_host_for_attestations(Some(api_url), use_versions_host) {
        match detect_with_mirror(owner, repo, digest, None).await {
            MirrorVerdict::Done(found) => return found,
            MirrorVerdict::AskGithub => {}
            MirrorVerdict::ConfirmWithGithub(err) => rejected = Some(err),
            MirrorVerdict::RetryGithub(err) => mirror_err = Some(err),
        }
    }

    let found = async {
        let (base_url, token) = routed_api_url(api_url, resolve_token_for_wrapper(Some(api_url)))
            .map_err(DetectError::SourceCreation)?;
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
    .await;
    github_probe_verdict(found, rejected, owner, repo, digest)
}

/// GitHub's answer to a probe, `found`, once mise-versions' answer was
/// `rejected` ([`MirrorVerdict::ConfirmWithGithub`]): GitHub confirming that
/// there are no attestations settles the artifact for probes, and the
/// rejection stands when GitHub cannot answer ([`rejection_stands`]).
fn github_probe_verdict(
    found: Result<bool, DetectError>,
    rejected: Option<AttestationError>,
    owner: &str,
    repo: &str,
    digest: &str,
) -> Result<bool, DetectError> {
    let Some(rejected) = rejected else {
        return found;
    };
    match found {
        Ok(found) => {
            if !found {
                KeptRejection::for_probe(&format!("{owner}/{repo}"), &sha256_digest(digest))
                    .forget();
            }
            Ok(found)
        }
        Err(DetectError::SourceCreation(err)) => Err(DetectError::SourceCreation(
            rejection_stands(err, Some(rejected)),
        )),
        Err(DetectError::Fetch(err)) => {
            Err(DetectError::Fetch(rejection_stands(err, Some(rejected))))
        }
    }
}

/// `digest` with the `sha256:` prefix GitHub's API takes.
fn sha256_digest(digest: &str) -> String {
    if digest.contains(':') {
        digest.to_string()
    } else {
        format!("sha256:{digest}")
    }
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
    let mut rejected = None;
    if use_versions_host_for_attestations(Some(api_url), use_versions_host) {
        match detect_with_mirror(owner, repo, digest, Some(predicate_type)).await {
            MirrorVerdict::Done(found) => return found,
            MirrorVerdict::AskGithub => {}
            MirrorVerdict::ConfirmWithGithub(err) => rejected = Some(err),
            MirrorVerdict::RetryGithub(err) => mirror_err = Some(err),
        }
    }

    let found = async {
        let client = attestation_client(api_url).map_err(DetectError::SourceCreation)?;
        let attestations = client
            .fetch_attestations(FetchParams {
                owner: owner.to_string(),
                repo: Some(format!("{owner}/{repo}")),
                digest: sha256_digest(digest),
                limit: 30,
                predicate_type: Some(predicate_type.to_string()),
            })
            .await
            .map_err(|err| {
                DetectError::Fetch(after_mirror_failure(err, owner, repo, mirror_err.as_ref()))
            })?;
        Ok(!attestations.is_empty())
    }
    .await;
    github_probe_verdict(found, rejected, owner, repo, digest)
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
        attestations_cache_dir().join(format!("{key}.msgpack.z")),
    )
    .with_fresh_duration(Some(MIRRORED_ATTESTATIONS_TTL))
    .build()
}

/// Where kept attestations and rejections ([`KeptRejection`]) are written:
/// the cache dir, or in a test the temporary one it set up.
fn attestations_cache_dir() -> std::path::PathBuf {
    #[cfg(test)]
    if let Some(dir) = tests::TEST_CACHE_DIR.with_borrow(Clone::clone) {
        return dir;
    }
    crate::dirs::CACHE.join("github")
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
/// rest of this command, so later checks don't ask again. Any other answer
/// drops it: the copy has just failed its caller, and left in place it would
/// go on telling probes of attestations mise-versions no longer has.
async fn refreshed_mirrored_attestations(
    repo: &str,
    digest: &str,
) -> eyre::Result<MirroredAttestations> {
    let fresh = crate::versions_host::github_attestations(repo, digest).await?;
    if mise_settings::Settings::get().paranoid.provenance() {
        return Ok(fresh);
    }
    if keep_mirrored_attestations(&fresh) {
        let key = mirrored_attestations_key(repo, digest);
        if let Err(err) = mirrored_attestations_cache(&key).write(&fresh) {
            debug!("failed to keep refreshed attestations for {repo}: {err:#}");
        }
        MIRRORED_ATTESTATIONS.lock().unwrap().insert(
            key,
            std::sync::Arc::new(tokio::sync::OnceCell::new_with(Some(fresh.clone()))),
        );
    } else {
        forget_mirrored_attestations(repo, digest);
    }
    Ok(fresh)
}

/// Drop the copy [`mirrored_attestations`] keeps for `repo` at `digest`, on
/// disk and for the rest of this command, so that neither a probe nor a later
/// command answers from it.
fn forget_mirrored_attestations(repo: &str, digest: &str) {
    let key = mirrored_attestations_key(repo, digest);
    if let Err(err) = mirrored_attestations_cache(&key).clear() {
        debug!("failed to drop kept attestations for {repo}: {err:#}");
    }
    MIRRORED_ATTESTATIONS.lock().unwrap().remove(&key);
}

/// That a set of attestations mise-versions served for one artifact was
/// rejected under one policy (the tags, signer workflow and predicate type
/// asked for), kept on disk until an answer settles the artifact.
///
/// A rejected set is dropped, and mise-versions asked again
/// ([`refreshed_mirrored_attestations`]), so without this the next command
/// would find nothing kept and take a "none" from mise-versions as final.
/// While one is kept, a "none" or a failed lookup from mise-versions has
/// GitHub confirm the digest ([`unless_rejected`]), and the rejection stands
/// when GitHub cannot answer ([`rejection_stands`]). A set that verifies, or
/// GitHub confirming that there are no attestations, forgets it. Only a set
/// that was checked and refused is kept ([`is_rejection`]): one left
/// unchecked because a trust root could not be loaded rejects nothing.
///
/// Each rejection is also kept for the digest alone, under any policy: the
/// lock-time probe ([`detect_with_mirror`]) asks only whether an artifact
/// has attestations, with no tags or signer to match, and must not take a
/// "none" from mise-versions as final for an artifact whose set was
/// rejected either ([`KeptRejection::for_probe`]).
struct KeptRejection {
    full: String,
    /// The record [`KeptRejection::load`] reads.
    key: String,
    /// The record for the digest under any policy, written and dropped
    /// with `key`.
    digest_key: String,
}

impl KeptRejection {
    fn new(
        full: &str,
        artifact_digest: &str,
        tags: &[String],
        expected_workflow: Option<&str>,
        predicate_type: Option<&str>,
    ) -> Self {
        let digest = format!("sha256:{artifact_digest}");
        let policy = (
            full.to_ascii_lowercase(),
            &digest,
            tags,
            expected_workflow,
            predicate_type,
        );
        Self {
            full: full.to_string(),
            key: format!(
                "attestation-rejection-{}",
                crate::hash::hash_to_str(&policy)
            ),
            digest_key: Self::digest_key(full, &digest),
        }
    }

    /// The rejection kept for `full` at `digest` (`sha256:<hex>`) under any
    /// policy, as the lock-time probe reads it.
    fn for_probe(full: &str, digest: &str) -> Self {
        let digest_key = Self::digest_key(full, digest);
        Self {
            full: full.to_string(),
            key: digest_key.clone(),
            digest_key,
        }
    }

    fn digest_key(full: &str, digest: &str) -> String {
        let artifact = (full.to_ascii_lowercase(), digest.to_ascii_lowercase());
        format!(
            "attestation-rejection-digest-{}",
            crate::hash::hash_to_str(&artifact)
        )
    }

    fn cache(key: &str) -> crate::cache::CacheManager<String> {
        crate::cache::CacheManagerBuilder::new(
            attestations_cache_dir().join(format!("{key}.msgpack.z")),
        )
        .build()
    }

    /// The rejection a command before this one kept.
    fn load(&self) -> Option<AttestationError> {
        let cache = Self::cache(&self.key);
        if !cache.has_fresh_value() {
            return None;
        }
        match cache.get_cached() {
            Ok(rejection) => Some(AttestationError::Verification(format!(
                "attestations mise-versions served for {} were rejected, and nothing has settled this artifact since: {rejection}",
                self.full
            ))),
            Err(err) => {
                debug!(
                    "failed to read the attestation rejection kept for {}: {err:#}",
                    self.full
                );
                None
            }
        }
    }

    /// Both records, or the one a probe's holds.
    fn keys(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.key.as_str())
            .chain((self.digest_key != self.key).then_some(self.digest_key.as_str()))
    }

    fn keep(&self, rejection: &AttestationError) {
        let rejection = rejection.to_string();
        for key in self.keys() {
            if let Err(err) = Self::cache(key).write(&rejection) {
                debug!(
                    "failed to keep the attestation rejection for {}: {err:#}",
                    self.full
                );
            }
        }
    }

    fn forget(&self) {
        for key in self.keys() {
            if let Err(err) = Self::cache(key).clear() {
                debug!(
                    "failed to drop the attestation rejection kept for {}: {err:#}",
                    self.full
                );
            }
        }
    }

    /// Record mise-versions' `verdict` once `rejection_is_new` says whether
    /// the rejection it carries, if any, was made in this command.
    fn record(&self, verdict: &MirrorVerdict, rejection_is_new: bool) {
        match verdict {
            MirrorVerdict::Done(Ok(_)) => self.forget(),
            MirrorVerdict::Done(Err(err)) if is_rejection(err) => self.keep(err),
            MirrorVerdict::ConfirmWithGithub(err) if rejection_is_new => self.keep(err),
            _ => {}
        }
    }

    /// GitHub's `result` once it was asked to confirm the artifact
    /// ([`MirrorVerdict::ConfirmWithGithub`]): a verification or "no
    /// attestations" settles it; anything else leaves the rejection kept.
    fn settle(&self, result: &AttestationResult<bool>) {
        if matches!(result, Ok(_) | Err(AttestationError::NoAttestations)) {
            self.forget();
        }
    }
}

/// The verdict on attestations from mise-versions for `full` that verify but
/// were made by `made_by`, or `None` to ask mise-versions again. A kept set
/// is dropped and asked for again, since one wrong answer would otherwise
/// fail the artifact for as long as it is kept, with nobody asked; a fresh
/// one is refused. Paranoid mode has GitHub answer instead.
fn foreign_attestations_verdict(
    full: &str,
    digest: &str,
    made_by: &str,
    fresh: bool,
) -> Option<MirrorVerdict> {
    if !mirror_may_answer_none() {
        warn!(
            "mise-versions returned attestations for {full} made by {made_by}; verifying with GitHub instead"
        );
        return Some(MirrorVerdict::AskGithub);
    }
    forget_mirrored_attestations(full, digest);
    if !fresh {
        debug!(
            "GitHub attestations kept for {full} were made by {made_by}; asking mise-versions again"
        );
        return None;
    }
    Some(MirrorVerdict::Done(Err(AttestationError::Verification(
        format!("the attestations mise-versions has for {full} were made by {made_by}"),
    ))))
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
    /// mise-versions has no attestations for an artifact whose earlier set
    /// it served was rejected ([`foreign_attestations_verdict`],
    /// [`kept_set_rejection`]). GitHub has
    /// to confirm that there are none; if GitHub cannot answer, the rejection
    /// stands ([`rejection_stands`]).
    ConfirmWithGithub(AttestationError),
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

/// mise-versions' answer after it was asked again because the set it served
/// before was `rejected`. Only a verification, or another rejection
/// ([`is_rejection`]), replaces that rejection. "No attestations", a failed
/// lookup (a 404 for an unknown repository, a set without its bundles, an
/// error), a set left unchecked (a trust root that could not be loaded, an
/// artifact that could not be read) and a referral to GitHub do not erase
/// it: GitHub has to confirm the digest
/// ([`MirrorVerdict::ConfirmWithGithub`]), and the rejection stands when
/// GitHub cannot answer. Otherwise a failure [`KeptRejection::record`] does
/// not keep would take the rejection's place, and the next command would
/// find nothing kept.
fn unless_rejected(
    verdict: MirrorVerdict,
    rejected: &mut Option<AttestationError>,
) -> MirrorVerdict {
    let Some(rejection) = rejected.take() else {
        return verdict;
    };
    match verdict {
        MirrorVerdict::Done(Ok(verified)) => MirrorVerdict::Done(Ok(verified)),
        MirrorVerdict::Done(Err(err)) if is_rejection(&err) => MirrorVerdict::Done(Err(err)),
        MirrorVerdict::Done(Err(_))
        | MirrorVerdict::AskGithub
        | MirrorVerdict::ConfirmWithGithub(_)
        | MirrorVerdict::RetryGithub(_) => MirrorVerdict::ConfirmWithGithub(rejection),
    }
}

/// Why a set mise-versions served earlier was rejected, from `err`, the
/// reason it did not verify: one naming another release's tag, say. The
/// refresh asked for then has to settle the digest ([`unless_rejected`]).
/// Only a rejection counts ([`is_rejection`]).
fn kept_set_rejection(err: Option<AttestationError>) -> Option<AttestationError> {
    err.filter(is_rejection)
}

/// Whether `err` says attestations were checked and refused. "No
/// attestations", a failed lookup, a trust root that could not be loaded
/// (so no signature was checked) and an artifact that could not be read
/// reject nothing.
fn is_rejection(err: &AttestationError) -> bool {
    !matches!(
        err,
        AttestationError::NoAttestations | AttestationError::TrustRoot(_) | AttestationError::Io(_)
    ) && !is_api_failure(err)
}

/// GitHub's `err` when it was asked to confirm that an artifact whose set
/// mise-versions served was `rejected` has no attestations: if GitHub could
/// not answer, the rejection stands.
fn rejection_stands(err: AttestationError, rejected: Option<AttestationError>) -> AttestationError {
    match rejected {
        Some(rejected) if is_api_failure(&err) => rejected,
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
/// copy within half an hour of a release. So is one that verifies but was
/// made by another repository ([`foreign_attestations_verdict`]).
async fn verify_with_mirror(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    tags: &[String],
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
    // Why the kept set was rejected, if it was: then only a fresh answer that
    // settles the digest replaces that rejection ([`unless_rejected`]).
    let mut rejected = None;
    let verdict = async {
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
                    tags,
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
                    if let Some(verdict) =
                        foreign_attestations_verdict(&full, &digest, &made_by, fresh)
                    {
                        return verdict;
                    }
                    rejected = Some(AttestationError::Verification(format!(
                        "the attestations mise-versions had for {full} were made by {made_by}"
                    )));
                }
                CachedAttestationVerification::Retry(err) if fresh => {
                    if !mirror_may_answer_none() {
                        debug!(
                            "mise-versions GitHub attestations did not verify for {full}; \
                             paranoid mode asks GitHub"
                        );
                        return MirrorVerdict::AskGithub;
                    }
                    return MirrorVerdict::Done(Err(err.unwrap_or_else(|| {
                        AttestationError::Verification(format!(
                            "no attestation mise-versions has for {full} verified this artifact"
                        ))
                    })));
                }
                CachedAttestationVerification::Retry(err) => {
                    debug!(
                        "GitHub attestations kept for {full} did not verify; asking mise-versions again"
                    );
                    rejected = kept_set_rejection(err);
                }
            }
        }
        unreachable!("the second pass always returns")
    }
    .await;
    // A rejection made in an earlier command stands until something settles
    // the artifact ([`KeptRejection`]).
    let kept = KeptRejection::new(
        &full,
        artifact_digest,
        tags,
        expected_workflow,
        predicate_type,
    );
    let rejection_is_new = rejected.is_some();
    if !rejection_is_new {
        rejected = kept.load();
    }
    let verdict = unless_rejected(verdict, &mut rejected);
    kept.record(&verdict, rejection_is_new);
    verdict
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
    let digest = sha256_digest(digest);
    let verdict = probe_mirror(&full, &digest, predicate_type).await;
    // A set mise-versions served for this artifact was rejected, in this
    // command or an earlier one ([`KeptRejection`]): only a "yes" stands,
    // which leads to verification. Taking a "none" as final would let
    // `mise lock` write the entry without provenance.
    match KeptRejection::for_probe(&full, &digest).load() {
        Some(rejection) => probe_unless_rejected(verdict, rejection),
        None => verdict,
    }
}

/// [`unless_rejected`] for a probe: once a set mise-versions served for the
/// artifact was rejected, anything but finding attestations has GitHub
/// confirm the digest, and the rejection stands when GitHub cannot answer.
fn probe_unless_rejected(
    verdict: MirrorVerdict<Result<bool, DetectError>>,
    rejection: AttestationError,
) -> MirrorVerdict<Result<bool, DetectError>> {
    match verdict {
        MirrorVerdict::Done(Ok(true)) => MirrorVerdict::Done(Ok(true)),
        MirrorVerdict::Done(Err(DetectError::Fetch(err))) if is_rejection(&err) => {
            MirrorVerdict::Done(Err(DetectError::Fetch(err)))
        }
        _ => MirrorVerdict::ConfirmWithGithub(rejection),
    }
}

/// What mise-versions says about whether `full` has attestations at
/// `digest`, before any kept rejection is consulted.
async fn probe_mirror(
    full: &str,
    digest: &str,
    predicate_type: Option<&str>,
) -> MirrorVerdict<Result<bool, DetectError>> {
    // As in `verify_with_mirror`: a set kept from earlier may predate the
    // attestation looked for, so a miss is asked of mise-versions again.
    for fresh in [false, true] {
        let found = if fresh {
            refreshed_mirrored_attestations(full, digest).await
        } else {
            mirrored_attestations(full, digest).await
        };
        let attestations = match found {
            Ok(Some(attestations)) => attestations,
            Ok(None) => {
                return detect_verdict(verdict_after_attestation_404(
                    full,
                    repo_listing(full).await,
                    !mirror_may_answer_none(),
                    || resolve_token_for_wrapper(None).is_some(),
                ));
            }
            Err(_) if !mirror_may_answer_none() => return MirrorVerdict::AskGithub,
            Err(err) => return detect_verdict(mirror_error_verdict(full, err)),
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
        MirrorVerdict::ConfirmWithGithub(err) => MirrorVerdict::ConfirmWithGithub(err),
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
    let verify = async {
        let result = mise_sigstore::verify_slsa_provenance(
            artifact_path,
            provenance_path,
            min_level,
            signer,
        )
        .await;
        match renamed_slsa_source(&result, provenance_path, signer, None).await {
            Some(renamed) => {
                mise_sigstore::verify_slsa_provenance(
                    artifact_path,
                    provenance_path,
                    min_level,
                    SlsaSignerIdentity {
                        source_repository: Some(&renamed),
                        ..signer
                    },
                )
                .await
            }
            None => result,
        }
    };
    if !mise_settings::Settings::get().generate_lockfiles() {
        return verify.await;
    }
    let artifact_digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
    let provenance_digest = mise_sigstore::calculate_file_digest(provenance_path).await?;
    let key = format!(
        "slsa:{artifact_digest}:{provenance_digest}:{min_level}:{}:{}:{:?}:{:?}",
        signer.identity, signer.issuer, signer.source_repository, signer.source_tag
    );
    shared_verification(key, verify).await
}

pub async fn verify_slsa_provenance_artifacts(
    provenance_path: &Path,
    artifacts: &[SlsaArtifact],
    min_level: u8,
    signer: SlsaSignerIdentity<'_>,
) -> AttestationResult<bool> {
    verify_slsa_provenance_artifacts_with(provenance_path, artifacts, min_level, signer, None).await
}

/// Stands in for GitHub's answer to "are these one repository?" in tests.
type ConfirmRename<'a> = Option<&'a (dyn Fn(&str, &str, &str) -> bool + Sync)>;

async fn verify_slsa_provenance_artifacts_with(
    provenance_path: &Path,
    artifacts: &[SlsaArtifact],
    min_level: u8,
    signer: SlsaSignerIdentity<'_>,
    confirm: ConfirmRename<'_>,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    let result = mise_sigstore::verify_slsa_provenance_artifacts(
        provenance_path,
        artifacts,
        min_level,
        signer,
    )
    .await;
    match renamed_slsa_source(&result, provenance_path, signer, confirm).await {
        Some(renamed) => {
            mise_sigstore::verify_slsa_provenance_artifacts(
                provenance_path,
                artifacts,
                min_level,
                SlsaSignerIdentity {
                    source_repository: Some(&renamed),
                    ..signer
                },
            )
            .await
        }
        None => result,
    }
}

/// When `result` failed because the provenance was built in a repository
/// `signer` doesn't accept, the name `signer.source_repository` had when it
/// was built, if GitHub confirms that both names are one repository (renamed
/// or transferred since). Provenance names the repository as it was when it
/// was signed. The caller verifies again in full with that name.
///
/// Only api.github.com itself can confirm a rename:
/// [`crate::github::canonical_repo`] refuses when `url_replacements`
/// reroutes it, since that host may have served the provenance.
async fn renamed_slsa_source(
    result: &AttestationResult<bool>,
    provenance_path: &Path,
    signer: SlsaSignerIdentity<'_>,
    confirm: ConfirmRename<'_>,
) -> Option<String> {
    if result.is_ok() {
        return None;
    }
    let (owner, repo) = signer
        .source_repository
        .and_then(mise_sigstore::normalize_github_repository)?
        .split_once('/')?;
    let unaccepted =
        mise_sigstore::slsa_provenance_unaccepted_source_repositories(provenance_path, signer)
            .await
            .ok()?;
    // A handful at most; each one costs GitHub API requests.
    for other in unaccepted.into_iter().take(3) {
        let confirmed = match confirm {
            Some(confirm) => confirm(owner, repo, &other),
            None => {
                crate::github::attested_by_repository(owner, repo, std::slice::from_ref(&other))
                    .await
            }
        };
        if confirmed {
            debug!("SLSA provenance for {owner}/{repo} was built in {other}, its former name");
            return Some(other);
        }
    }
    None
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

/// Verify attestations against a sha512 digest and return the `owner/repo`
/// each verified one was built from. Passthrough — no token needed, but the TUF
/// root follows `url_replacements` like every other check here.
pub async fn verify_github_attestation_sources_for_sha512(
    sha512: &[u8],
    attestations: &[Attestation],
    signer_workflow: Option<&str>,
) -> AttestationResult<Vec<String>> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    mise_sigstore::verify_github_attestation_sources_for_sha512(
        sha512,
        attestations,
        signer_workflow,
    )
    .await
}

/// [`verify_github_attestation_sources_for_sha512`] for GitHub Actions or
/// gitlab.com CI provenance; each source is `github.com/owner/repo` or
/// `gitlab.com/group/project`.
pub async fn verify_forge_attestation_sources_for_sha512(
    sha512: &[u8],
    attestations: &[Attestation],
) -> AttestationResult<Vec<String>> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    mise_sigstore::verify_forge_attestation_sources_for_sha512(sha512, attestations).await
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// The cache dir [`attestations_cache_dir`] answers with in this
        /// test, set by [`temp_cache`].
        pub(super) static TEST_CACHE_DIR: std::cell::RefCell<Option<std::path::PathBuf>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Kept attestations and rejections go to a temporary dir until this is
    /// dropped, instead of the cache dir of whoever runs the tests.
    struct TempCache {
        _dir: tempfile::TempDir,
    }

    impl Drop for TempCache {
        fn drop(&mut self) {
            TEST_CACHE_DIR.set(None);
        }
    }

    fn temp_cache() -> TempCache {
        let dir = tempfile::tempdir().unwrap();
        TEST_CACHE_DIR.set(Some(dir.path().to_path_buf()));
        TempCache { _dir: dir }
    }

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

    /// slsa-github-generator provenance for sops v3.9.0, built in
    /// getsops/sops, which was mozilla/sops until its transfer.
    fn sops_provenance() -> &'static Path {
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../mise-sigstore/tests/fixtures/sops_v3_9_0.intoto.jsonl"
        ))
    }

    fn sops_artifact() -> SlsaArtifact {
        SlsaArtifact {
            name: "sops-v3.9.0.linux.amd64".to_string(),
            sha256: "0d65660fbe785647ff4f1764d7f69edf598f79d6d79ebbef4a501909b6ff6b82".to_string(),
        }
    }

    /// The generator's identity, expecting sops under its former name.
    const SOPS_SIGNER_BEFORE_TRANSFER: SlsaSignerIdentity<'static> = SlsaSignerIdentity {
        identity: "https://github.com/slsa-framework/slsa-github-generator/.github/workflows/generator_generic_slsa3.yml@refs/tags/v2.0.0",
        issuer: "https://token.actions.githubusercontent.com",
        source_repository: Some("mozilla/sops"),
        source_tag: None,
    };

    /// Verifies the sops provenance against `SOPS_SIGNER_BEFORE_TRANSFER`,
    /// with `confirm` standing in for api.github.com's answer to whether two
    /// names are one repository. Each verification is a full Sigstore check,
    /// so each test runs only one to stay within the per-test time limit.
    fn verify_sops_rename(
        confirm: &(dyn Fn(&str, &str, &str) -> bool + Sync),
    ) -> AttestationResult<bool> {
        let _settings = SettingsGuard::new(None);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(verify_slsa_provenance_artifacts_with(
                sops_provenance(),
                &[sops_artifact()],
                1,
                SOPS_SIGNER_BEFORE_TRANSFER,
                Some(confirm),
            ))
    }

    #[test]
    fn test_slsa_refuses_a_rename_github_does_not_confirm() {
        // Without GitHub's word, provenance built in getsops/sops is not
        // mozilla/sops's.
        let err = verify_sops_rename(&|_, _, _| false).unwrap_err();
        assert!(err.to_string().contains("built in getsops/sops"), "{err}");
    }

    #[test]
    fn test_slsa_accepts_a_rename_github_confirms() {
        // api.github.com says getsops/sops is mozilla/sops, transferred.
        let confirmed = verify_sops_rename(&|owner, repo, other| {
            (owner, repo, other) == ("mozilla", "sops", "getsops/sops")
        });
        assert!(confirmed.unwrap());
    }

    fn verify_sops_v3_9_0_as(tag: &str) -> AttestationResult<bool> {
        let _settings = SettingsGuard::new(None);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(verify_slsa_provenance_artifacts(
                sops_provenance(),
                &[sops_artifact()],
                1,
                SlsaSignerIdentity {
                    source_repository: Some("getsops/sops"),
                    source_tag: Some(tag),
                    ..SOPS_SIGNER_BEFORE_TRANSFER
                },
            ))
    }

    #[test]
    fn test_slsa_rejects_provenance_of_another_release() {
        // A host serving sops v3.9.0's genuine artifact and provenance as
        // v3.10.0: the generator signs every release with one identity.
        let err = verify_sops_v3_9_0_as("v3.10.0").unwrap_err();
        assert!(
            err.to_string()
                .contains("built from refs/tags/v3.9.0, expected refs/tags/v3.10.0"),
            "{err}"
        );
    }

    #[test]
    fn test_slsa_accepts_provenance_of_its_release() {
        assert!(verify_sops_v3_9_0_as("v3.9.0").unwrap());
    }

    #[test]
    fn test_slsa_rename_cannot_be_confirmed_by_a_mirror() {
        // A host standing in for api.github.com claims both names are one
        // repository. It must not get to vouch for the provenance it may
        // have served.
        let mut mirror = mockito::Server::new();
        let lookups = ["mozilla/sops", "getsops/sops"].map(|repo| {
            mirror
                .mock("GET", format!("/repos/{repo}").as_str())
                .with_header("content-type", "application/json")
                .with_body(r#"{"id": 1, "full_name": "getsops/sops"}"#)
                .expect(0)
                .create()
        });
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => mirror.url(),
        }));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let err = runtime
            .block_on(verify_slsa_provenance_artifacts(
                sops_provenance(),
                &[sops_artifact()],
                1,
                SOPS_SIGNER_BEFORE_TRANSFER,
            ))
            .unwrap_err();
        assert!(err.to_string().contains("built in getsops/sops"), "{err}");
        for lookup in lookups {
            lookup.assert();
        }
    }

    #[test]
    fn test_rerouted_api_cannot_confirm_a_rename() {
        // The host that stands in for api.github.com also picked the
        // attestations, so it must not get to say that the repository that
        // made them is the requested one under an older name.
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "https://github-proxy.invalid".to_string(),
        }));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let err = runtime
            .block_on(crate::github::canonical_repo("erlang/rebar3"))
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("only GitHub can confirm a repository rename"),
            "{err}"
        );

        let result = runtime.block_on(accept_renamed_repository(
            "erlang",
            "rebar3",
            None,
            Err(AttestationError::RepositoryMismatch {
                expected: "erlang/rebar3".to_string(),
                found: vec!["jdx/mise".to_string()],
            }),
        ));
        assert!(
            matches!(result, Err(AttestationError::RepositoryMismatch { .. })),
            "{result:?}"
        );
    }

    fn original_host_token() -> Option<String> {
        Some("ghp_original_host".to_string())
    }

    #[test]
    fn test_routed_api_url_applies_simple_url_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "https://github-proxy.example.com".to_string(),
        }));

        let (routed, token) =
            routed_api_url(crate::github::API_URL, original_host_token()).unwrap();

        assert_eq!(routed, "https://github-proxy.example.com/");
        // The token was resolved for api.github.com; the proxy does not get it.
        assert_eq!(token, None);
    }

    #[test]
    fn test_routed_api_url_applies_regex_url_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "regex:^https://api\\.github\\.com".to_string() => "https://github-proxy.example.com/api".to_string(),
        }));

        let (routed, token) =
            routed_api_url(crate::github::API_URL, original_host_token()).unwrap();

        assert_eq!(routed, "https://github-proxy.example.com/api/");
        assert_eq!(token, None);
    }

    #[test]
    fn test_routed_api_url_keeps_original_url_without_replacement() {
        let _settings = SettingsGuard::new(None);

        let (routed, token) =
            routed_api_url(crate::github::API_URL, original_host_token()).unwrap();

        assert_eq!(routed, crate::github::API_URL);
        assert_eq!(token, original_host_token());
    }

    #[test]
    fn test_routed_api_url_keeps_token_on_same_host_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "https://api.github.com/proxy".to_string(),
        }));

        let (routed, token) =
            routed_api_url(crate::github::API_URL, original_host_token()).unwrap();

        assert_eq!(routed, "https://api.github.com/proxy/");
        assert_eq!(token, original_host_token());
    }

    #[test]
    fn test_routed_api_url_drops_inherited_userinfo_on_cross_host_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "regex:^https://(.*)ghe\\.example\\.com".to_string()
                => "https://${1}github-proxy.example.com".to_string(),
        }));

        let (routed, token) = routed_api_url(
            "https://user:password@ghe.example.com/api/v3",
            original_host_token(),
        )
        .unwrap();

        assert_eq!(routed, "https://github-proxy.example.com/api/v3");
        assert_eq!(token, None);
    }

    #[test]
    fn test_routed_api_url_rejects_credential_downgrade() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "http://api.github.com".to_string(),
        }));

        let err = routed_api_url(crate::github::API_URL, original_host_token()).unwrap_err();

        assert!(err.to_string().contains("refusing to send credentials"));
        assert!(!is_api_failure(&err));
    }

    #[test]
    fn test_routed_api_url_drops_token_on_cross_host_downgrade() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "http://github-proxy.example.com".to_string(),
        }));

        let (routed, token) =
            routed_api_url(crate::github::API_URL, original_host_token()).unwrap();

        assert_eq!(routed, "http://github-proxy.example.com/");
        assert_eq!(token, None);
    }

    #[test]
    fn test_routed_api_url_rejects_userinfo_downgrade_without_token() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string()
                => "http://user:password@github-proxy.example.com".to_string(),
        }));

        let err = routed_api_url(crate::github::API_URL, None).unwrap_err();

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

    /// Keep `attestations` for `repo` at `digest` as [`mirrored_attestations`]
    /// does, on disk and for this command.
    fn keep_for_test(repo: &str, digest: &str, attestations: MirroredAttestations) {
        assert!(
            TEST_CACHE_DIR.with_borrow(Option::is_some),
            "set up a temp_cache() before keeping attestations"
        );
        let key = mirrored_attestations_key(repo, digest);
        mirrored_attestations_cache(&key)
            .write(&attestations)
            .unwrap();
        let cell = MirroredAttestationsCell::default();
        cell.set(attestations).unwrap();
        MIRRORED_ATTESTATIONS.lock().unwrap().insert(key, cell);
    }

    fn kept_on_disk(repo: &str, digest: &str) -> bool {
        mirrored_attestations_cache(&mirrored_attestations_key(repo, digest)).has_fresh_value()
    }

    fn kept_in_this_command(repo: &str, digest: &str) -> bool {
        MIRRORED_ATTESTATIONS
            .lock()
            .unwrap()
            .contains_key(&mirrored_attestations_key(repo, digest))
    }

    #[test]
    fn test_kept_attestations_of_another_repository_are_dropped_and_asked_again() {
        let _settings = SettingsGuard::new(None);
        let _cache = temp_cache();
        let repo = "acme/kept-by-another";
        let digest = "sha256:1";
        let attestation: mise_sigstore::Attestation =
            serde_json::from_value(serde_json::json!({"bundle": {}, "bundle_url": null})).unwrap();
        keep_for_test(repo, digest, Some(vec![attestation]));
        assert!(kept_on_disk(repo, digest));

        // the set kept from an earlier command: asked for again, and gone
        assert!(foreign_attestations_verdict(repo, digest, "evil/tool", false).is_none());
        assert!(
            !kept_on_disk(repo, digest),
            "the foreign set stayed on disk"
        );
        assert!(!kept_in_this_command(repo, digest));

        // the answer mise-versions gives now is refused
        let verdict = foreign_attestations_verdict(repo, digest, "evil/tool", true);
        assert!(
            matches!(
                verdict,
                Some(MirrorVerdict::Done(Err(AttestationError::Verification(ref msg))))
                    if msg.contains("made by evil/tool")
            ),
            "a fresh set made by another repository must fail"
        );
    }

    /// After a kept set made by another repository is rejected, mise-versions
    /// answering "none" when asked again does not let an optional verification
    /// pass: GitHub has to confirm it, and the rejection stands when GitHub
    /// cannot answer.
    #[test]
    fn test_an_empty_refresh_does_not_erase_a_rejected_set() {
        let rejection = || AttestationError::Verification("made by evil/tool".to_string());
        let none = || MirrorVerdict::Done(Err(AttestationError::NoAttestations));

        let mut rejected = Some(rejection());
        assert!(matches!(
            unless_rejected(none(), &mut rejected),
            MirrorVerdict::ConfirmWithGithub(AttestationError::Verification(ref msg))
                if msg.contains("evil/tool")
        ));
        // with nothing rejected, "none" stays mise-versions' answer
        assert!(matches!(
            unless_rejected(none(), &mut None),
            MirrorVerdict::Done(Err(AttestationError::NoAttestations))
        ));
        // a verification is kept
        assert!(matches!(
            unless_rejected(MirrorVerdict::Done(Ok(true)), &mut Some(rejection())),
            MirrorVerdict::Done(Ok(true))
        ));
        // so is a failure that cannot be skipped
        assert!(matches!(
            unless_rejected(
                MirrorVerdict::Done(Err(AttestationError::Verification("other".to_string()))),
                &mut Some(rejection())
            ),
            MirrorVerdict::Done(Err(AttestationError::Verification(ref msg))) if msg == "other"
        ));
        // a set left unchecked, because the signing keys could not be loaded
        // or the artifact could not be read, rejects nothing and so does not
        // replace the rejection, which `KeptRejection::record` would not keep
        let unchecked = [
            AttestationError::TrustRoot("tuf-repo-cdn.sigstore.dev unreachable".to_string()),
            AttestationError::Io(std::io::Error::other("read failed")),
        ];
        for err in unchecked {
            let what = err.to_string();
            assert!(
                matches!(
                    unless_rejected(MirrorVerdict::Done(Err(err)), &mut Some(rejection())),
                    MirrorVerdict::ConfirmWithGithub(AttestationError::Verification(ref msg))
                        if msg.contains("evil/tool")
                ),
                "{what} replaced the rejection"
            );
        }
    }

    /// A refresh that fails to look the digest up, after a kept set made by
    /// another repository was rejected, does not turn that rejection into a
    /// failed lookup that optional verification skips: GitHub has to confirm
    /// the digest.
    #[test]
    fn test_a_failed_refresh_does_not_erase_a_rejected_set() {
        let rejection = || AttestationError::Verification("made by evil/tool".to_string());
        let full = "owner/tool";
        let refreshes = [
            (
                "a 404 for a repository mise-versions does not list",
                verdict_after_attestation_404(full, RepoListing::Unknown, false, || false),
            ),
            (
                "a 404 GitHub is asked about with a token",
                verdict_after_attestation_404(full, RepoListing::Unknown, false, || true),
            ),
            (
                "a failed repository listing",
                verdict_after_attestation_404(
                    full,
                    RepoListing::Failed(eyre::eyre!("connection reset")),
                    false,
                    || false,
                ),
            ),
            (
                "attestations without their bundles",
                MirrorVerdict::Done(Err(AttestationError::Api(format!(
                    "mise-versions returned attestations for {full} without their bundles"
                )))),
            ),
            (
                "a lookup error",
                MirrorVerdict::Done(Err(mirror_failure(full, &eyre::eyre!("502")))),
            ),
            (
                "a lookup error GitHub is asked to make up for",
                MirrorVerdict::RetryGithub(eyre::eyre!("503")),
            ),
        ];
        for (what, verdict) in refreshes {
            assert!(
                matches!(
                    unless_rejected(verdict, &mut Some(rejection())),
                    MirrorVerdict::ConfirmWithGithub(AttestationError::Verification(ref msg))
                        if msg.contains("evil/tool")
                ),
                "{what} must keep the rejection"
            );
        }

        // with nothing rejected, a failed lookup stays a failed lookup
        assert!(matches!(
            unless_rejected(
                verdict_after_attestation_404(full, RepoListing::Unknown, false, || false),
                &mut None
            ),
            MirrorVerdict::Done(Err(AttestationError::Api(_)))
        ));
    }

    /// A kept set that fails to verify, such as one naming another release's
    /// tag, is a rejection a refresh with none does not erase; "none" and a
    /// failed lookup reject nothing.
    #[test]
    fn test_a_kept_set_that_failed_is_not_erased_by_an_empty_refresh() {
        let wrong_tag = || {
            AttestationError::Verification(
                "the attestation was made for refs/tags/v1.0.0, expected refs/tags/v2.0.0"
                    .to_string(),
            )
        };
        let mut rejected = kept_set_rejection(Some(wrong_tag()));
        assert!(matches!(
            unless_rejected(
                MirrorVerdict::Done(Err(AttestationError::NoAttestations)),
                &mut rejected
            ),
            MirrorVerdict::ConfirmWithGithub(AttestationError::Verification(ref msg))
                if msg.contains("refs/tags/v1.0.0")
        ));
        let mut rejected = kept_set_rejection(Some(AttestationError::WorkflowMismatch(
            "release.yml".to_string(),
        )));
        assert!(matches!(
            unless_rejected(
                MirrorVerdict::RetryGithub(eyre::eyre!("503")),
                &mut rejected
            ),
            MirrorVerdict::ConfirmWithGithub(AttestationError::WorkflowMismatch(_))
        ));

        assert!(kept_set_rejection(None).is_none());
        assert!(kept_set_rejection(Some(AttestationError::NoAttestations)).is_none());
        assert!(kept_set_rejection(Some(AttestationError::Api("502".to_string()))).is_none());
        // a set left unchecked because its signing keys could not be loaded,
        // or an artifact that could not be read, rejects nothing either
        assert!(
            kept_set_rejection(Some(AttestationError::TrustRoot(
                "tuf-repo-cdn.sigstore.dev unreachable".to_string()
            )))
            .is_none()
        );
        assert!(
            kept_set_rejection(Some(AttestationError::Io(std::io::Error::other(
                "read failed"
            ))))
            .is_none()
        );
    }

    #[test]
    fn test_github_failing_to_confirm_keeps_the_rejection() {
        let rejection = || AttestationError::Verification("made by evil/tool".to_string());

        // GitHub could not answer: the rejection stands
        let err = rejection_stands(AttestationError::Api("503".to_string()), Some(rejection()));
        assert!(
            matches!(err, AttestationError::Verification(ref msg) if msg.contains("evil/tool"))
        );
        // GitHub confirmed there are none
        assert!(matches!(
            rejection_stands(AttestationError::NoAttestations, Some(rejection())),
            AttestationError::NoAttestations
        ));
        // nothing was rejected: GitHub's failure is the error
        assert!(matches!(
            rejection_stands(AttestationError::Api("503".to_string()), None),
            AttestationError::Api(_)
        ));
    }

    /// A mise-versions stand-in that has no attestations for any digest, with
    /// the settings that route mise-versions to it.
    fn empty_mise_versions() -> SettingsGuard {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.local_addr().unwrap());
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for mut stream in server.incoming().flatten() {
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                let body = r#"{"attestations":[]}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        // Set explicitly, so MISE_USE_VERSIONS_HOST=false cannot skip the test.
        SettingsGuard::with_versions_host(
            Some(indexmap::indexmap! {
                "https://mise-versions.jdx.dev".to_string() => url,
            }),
            Some(true),
        )
    }

    #[tokio::test]
    async fn test_a_refresh_that_keeps_nothing_drops_the_kept_set() {
        let _settings = empty_mise_versions();
        let _cache = temp_cache();
        let repo = "acme/refreshed-empty";
        let digest = "sha256:2";
        let attestation: mise_sigstore::Attestation =
            serde_json::from_value(serde_json::json!({"bundle": {}, "bundle_url": null})).unwrap();
        keep_for_test(repo, digest, Some(vec![attestation]));
        assert!(kept_on_disk(repo, digest));

        let fresh = refreshed_mirrored_attestations(repo, digest).await.unwrap();
        assert_eq!(fresh.map(|a| a.len()), Some(0));
        assert!(
            !kept_on_disk(repo, digest),
            "the superseded set stayed on disk"
        );
        assert!(!kept_in_this_command(repo, digest));
    }

    /// A rejection outlives the command that made it: the next one finds
    /// nothing kept and mise-versions answering "none", and still has GitHub
    /// confirm the digest instead of passing an optional verification.
    #[tokio::test]
    async fn test_a_rejection_outlives_the_command_that_made_it() {
        let _settings = empty_mise_versions();
        let _cache = temp_cache();
        let owner = "acme";
        let repo = "rejected-before";
        let full = format!("{owner}/{repo}");
        let digest = "3".repeat(64);
        let tags = vec!["v1.0.0".to_string()];
        let kept = KeptRejection::new(&full, &digest, &tags, None, None);

        // the command that rejected the set, then had GitHub confirm
        let rejection = AttestationError::Verification("made by evil/tool".to_string());
        kept.record(&MirrorVerdict::ConfirmWithGithub(rejection), true);

        let verdict =
            verify_with_mirror(Path::new("unread"), owner, repo, &tags, None, None, &digest).await;
        let MirrorVerdict::ConfirmWithGithub(err) = verdict else {
            panic!("mise-versions answering none erased a kept rejection");
        };
        assert!(
            matches!(err, AttestationError::Verification(ref msg) if msg.contains("evil/tool")),
            "{err}"
        );
        // GitHub cannot answer: the rejection stands, and stays kept
        assert!(matches!(
            rejection_stands(AttestationError::Api("503".to_string()), Some(err)),
            AttestationError::Verification(_)
        ));
        kept.settle(&Err(AttestationError::Api("503".to_string())));
        assert!(kept.load().is_some());

        // another policy (another release's tag) was not rejected
        let other_tag = vec!["v2.0.0".to_string()];
        assert!(matches!(
            verify_with_mirror(
                Path::new("unread"),
                owner,
                repo,
                &other_tag,
                None,
                None,
                &digest
            )
            .await,
            MirrorVerdict::Done(Err(AttestationError::NoAttestations))
        ));

        // GitHub confirming that there are none settles the artifact
        kept.settle(&Err(AttestationError::NoAttestations));
        assert!(kept.load().is_none());
        assert!(matches!(
            verify_with_mirror(Path::new("unread"), owner, repo, &tags, None, None, &digest).await,
            MirrorVerdict::Done(Err(AttestationError::NoAttestations))
        ));
    }

    /// `mise lock` probes an artifact after a set mise-versions served for it
    /// failed to verify, and mise-versions has none on the retry: the probe
    /// has GitHub confirm the digest instead of writing the entry without
    /// provenance, and the rejection stands when GitHub cannot answer.
    #[tokio::test]
    async fn test_a_probe_after_a_rejection_does_not_take_none_as_final() {
        let _settings = empty_mise_versions();
        let _cache = temp_cache();
        let owner = "acme";
        let repo = "probed-after-rejection";
        let full = format!("{owner}/{repo}");
        let digest = "5".repeat(64);

        // nothing rejected yet: mise-versions' "none" is the answer
        assert!(matches!(
            detect_with_mirror(owner, repo, &digest, None).await,
            MirrorVerdict::Done(Ok(false))
        ));

        // a fresh set that failed its check, as `verify_with_mirror` keeps it
        let tags = vec!["v1.0.0".to_string()];
        KeptRejection::new(&full, &digest, &tags, Some("release.yml"), None).record(
            &MirrorVerdict::Done(Err(AttestationError::Verification(
                "made for refs/tags/v0.9.0".to_string(),
            ))),
            false,
        );

        // the probe matches no policy, with or without a predicate type, and
        // reads the digest in either spelling
        for (probed, predicate_type) in [
            (digest.clone(), None),
            (format!("sha256:{digest}"), None),
            (digest.clone(), Some("https://slsa.dev/provenance/v1")),
        ] {
            let MirrorVerdict::ConfirmWithGithub(rejection) =
                detect_with_mirror(owner, repo, &probed, predicate_type).await
            else {
                panic!("an empty mirror erased the rejection for {probed} ({predicate_type:?})");
            };
            assert!(
                rejection.to_string().contains("refs/tags/v0.9.0"),
                "{rejection}"
            );

            // GitHub cannot answer: the rejection stands, so the lockfile
            // is not written without provenance
            let err = github_probe_verdict(
                Err(DetectError::Fetch(AttestationError::Api("503".to_string()))),
                Some(rejection),
                owner,
                repo,
                &probed,
            )
            .unwrap_err();
            let DetectError::Fetch(err) = err else {
                panic!("{err}");
            };
            assert!(!is_api_failure(&err), "{err}");
        }

        // another digest of the same repository was not rejected
        assert!(matches!(
            detect_with_mirror(owner, repo, &"6".repeat(64), None).await,
            MirrorVerdict::Done(Ok(false))
        ));

        // GitHub finding attestations leaves the rejection for verification
        let MirrorVerdict::ConfirmWithGithub(rejection) =
            detect_with_mirror(owner, repo, &digest, None).await
        else {
            panic!("the rejection was not kept");
        };
        assert!(github_probe_verdict(Ok(true), Some(rejection), owner, repo, &digest).unwrap());
        let MirrorVerdict::ConfirmWithGithub(rejection) =
            detect_with_mirror(owner, repo, &digest, None).await
        else {
            panic!("finding attestations settled the probe");
        };

        // GitHub confirming that there are none settles it for probes
        assert!(!github_probe_verdict(Ok(false), Some(rejection), owner, repo, &digest).unwrap());
        assert!(matches!(
            detect_with_mirror(owner, repo, &digest, None).await,
            MirrorVerdict::Done(Ok(false))
        ));
    }

    /// A verification forgets the rejection for probes too.
    #[test]
    fn test_a_verification_forgets_the_rejection_for_probes() {
        let _settings = SettingsGuard::new(None);
        let _cache = temp_cache();
        let full = "acme/verified-after-rejection";
        let digest = "7".repeat(64);
        let kept = KeptRejection::new(full, &digest, &[], None, None);
        let probe = KeptRejection::for_probe(full, &format!("sha256:{digest}"));

        kept.record(
            &MirrorVerdict::Done(Err(AttestationError::WorkflowMismatch(
                "release.yml".to_string(),
            ))),
            false,
        );
        assert!(probe.load().is_some());
        kept.settle(&Ok(true));
        assert!(kept.load().is_none());
        assert!(probe.load().is_none());
    }

    #[test]
    fn test_only_a_rejection_is_kept_and_a_verification_forgets_it() {
        let _settings = SettingsGuard::new(None);
        let _cache = temp_cache();
        let kept = KeptRejection::new("acme/kept-rejection", &"4".repeat(64), &[], None, None);

        // a set left unchecked because the signing keys could not be loaded
        kept.record(
            &MirrorVerdict::Done(Err(AttestationError::TrustRoot(
                "tuf-repo-cdn.sigstore.dev unreachable".to_string(),
            ))),
            false,
        );
        kept.record(
            &MirrorVerdict::Done(Err(AttestationError::NoAttestations)),
            false,
        );
        assert!(kept.load().is_none());

        // a fresh set that fails is kept, and so is one rejected in this
        // command that GitHub is asked to confirm
        kept.record(
            &MirrorVerdict::Done(Err(AttestationError::Verification(
                "made for refs/tags/v0.9.0".to_string(),
            ))),
            false,
        );
        assert!(
            matches!(kept.load(), Some(AttestationError::Verification(ref msg)) if msg.contains("refs/tags/v0.9.0"))
        );
        // one loaded from an earlier command is not written again
        let loaded = kept.load().unwrap();
        kept.record(&MirrorVerdict::ConfirmWithGithub(loaded), false);
        let reloaded = kept.load().unwrap().to_string();
        assert_eq!(reloaded.matches("were rejected").count(), 1, "{reloaded}");

        // a set that verifies forgets it, and so does GitHub verifying
        kept.record(&MirrorVerdict::Done(Ok(true)), false);
        assert!(kept.load().is_none());
        kept.record(
            &MirrorVerdict::ConfirmWithGithub(AttestationError::WorkflowMismatch(
                "release.yml".to_string(),
            )),
            true,
        );
        assert!(kept.load().is_some());
        kept.settle(&Ok(true));
        assert!(kept.load().is_none());
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
