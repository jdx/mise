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

pub use mise_sigstore::{AttestationError, SlsaArtifact};

/// Result alias that matches `mise_sigstore`'s internal convention.
type AttestationResult<T> = std::result::Result<T, AttestationError>;

/// Resolve a GitHub token for an optional API base URL, defaulting to [`crate::github::API_URL`].
fn resolve_token_for_wrapper(api_url: Option<&str>) -> Option<String> {
    let url = api_url.unwrap_or(crate::github::API_URL);
    crate::github::resolve_token_for_api_url(url)
}

fn routed_api_url(api_url: &str) -> String {
    let Ok(mut url) = url::Url::parse(api_url) else {
        debug!("invalid GitHub attestation API URL, skipping url_replacements: {api_url}");
        return api_url.to_string();
    };
    let original = url.clone();
    crate::http::apply_url_replacements(&mut url);
    if url == original {
        api_url.to_string()
    } else {
        url.to_string()
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
    (url != original).then(|| url.to_string())
}

/// Build a [`RetryConfig`] from mise's HTTP settings so attestation requests
/// retry and time out exactly like the rest of mise's HTTP traffic rather than
/// using a policy hardcoded in the `mise-sigstore` crate.
fn mise_retry_config() -> RetryConfig {
    let settings = crate::config::Settings::get();
    RetryConfig {
        timeout: settings.http_timeout(),
        retries: settings.http_retries.max(0) as usize,
        ..RetryConfig::default()
    }
}

fn attestation_client(api_url: &str) -> AttestationResult<AttestationClient> {
    let token = resolve_token_for_wrapper(Some(api_url));
    let base_url = routed_api_url(api_url);
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
    mise_sigstore::set_tuf_url(routed_tuf_url());
    let mut digest = None;
    if use_versions_host_for_attestations(api_url, use_versions_host) {
        let artifact_digest = mise_sigstore::calculate_file_digest(artifact_path).await?;
        match crate::versions_host::github_attestations(
            &format!("{owner}/{repo}"),
            &format!("sha256:{artifact_digest}"),
        )
        .await
        {
            Ok(Some(attestations)) => {
                trace!(
                    "got {} GitHub attestations for {owner}/{repo}@sha256:{artifact_digest} from mise-versions",
                    attestations.len()
                );
                if attestations.is_empty() {
                    return Err(AttestationError::NoAttestations);
                } else if attestations.iter().any(|a| !a.has_inline_bundle()) {
                    debug!(
                        "mise-versions returned GitHub attestations without inline bundles; falling back to GitHub API"
                    );
                } else {
                    return mise_sigstore::verify_github_attestation_with_attestations(
                        artifact_path,
                        &attestations,
                        expected_workflow,
                    )
                    .await;
                }
            }
            Ok(None) => {}
            Err(err) => debug!("mise-versions GitHub attestations lookup failed: {err:#}"),
        }
        digest = Some(artifact_digest);
    }

    let token = resolve_token_for_wrapper(api_url);
    let base_url = routed_api_url(api_url.unwrap_or(crate::github::API_URL));
    if let Some(digest) = digest {
        mise_sigstore::verify_github_attestation_with_base_url_and_digest(
            artifact_path,
            owner,
            repo,
            token.as_deref(),
            expected_workflow,
            &base_url,
            &digest,
            mise_retry_config(),
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
    }
}

/// Verify a GitHub artifact attestation filtered by predicate type.
///
/// The versions-host cache is keyed by digest only, so predicate-filtered
/// requests go directly to the GitHub attestations API.
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
    let client = attestation_client(api_url.unwrap_or(crate::github::API_URL))?;
    let attestations = client
        .fetch_attestations(FetchParams {
            owner: owner.to_string(),
            repo: Some(format!("{owner}/{repo}")),
            digest: format!("sha256:{artifact_digest}"),
            limit: 30,
            predicate_type: Some(predicate_type.to_string()),
        })
        .await?;
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
    if use_versions_host_for_attestations(Some(api_url), use_versions_host) {
        match crate::versions_host::github_attestations(&format!("{owner}/{repo}"), digest).await {
            Ok(Some(attestations)) => {
                trace!(
                    "got {} GitHub attestation probes for {owner}/{repo}@{digest} from mise-versions",
                    attestations.len()
                );
                return Ok(!attestations.is_empty());
            }
            Ok(None) => {}
            Err(err) => debug!("mise-versions GitHub attestation probe failed: {err:#}"),
        }
    }

    let token = resolve_token_for_wrapper(Some(api_url));
    let base_url = routed_api_url(api_url);
    let source = GitHubSource::with_base_url(owner, repo, token.as_deref(), &base_url)
        .map_err(DetectError::SourceCreation)?;
    let artifact_ref = ArtifactRef::from_digest(digest);
    let attestations = source
        .fetch_attestations(&artifact_ref)
        .await
        .map_err(DetectError::Fetch)?;
    Ok(!attestations.is_empty())
}

/// Probe the GitHub attestation API for the given digest and predicate type.
///
/// The versions-host cache is keyed by digest only, so predicate-filtered
/// requests go directly to the GitHub attestations API.
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
        .map_err(DetectError::Fetch)?;
    Ok(!attestations.is_empty())
}

fn use_versions_host_for_attestations(api_url: Option<&str>, use_versions_host: bool) -> bool {
    let settings = crate::config::Settings::get();
    if !use_versions_host || settings.prefer_offline() || !settings.use_versions_host {
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
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    mise_sigstore::verify_slsa_provenance(artifact_path, provenance_path, min_level).await
}

pub async fn verify_slsa_provenance_artifacts(
    provenance_path: &Path,
    artifacts: &[SlsaArtifact],
    min_level: u8,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    mise_sigstore::verify_slsa_provenance_artifacts(provenance_path, artifacts, min_level).await
}

pub fn is_slsa_subject_mismatch(error: &AttestationError) -> bool {
    mise_sigstore::is_slsa_subject_mismatch(error)
}

pub fn is_api_failure(error: &AttestationError) -> bool {
    matches!(error, AttestationError::Api(_) | AttestationError::Http(_))
}

/// Verify a keyless Cosign signature or bundle. Passthrough — no token needed.
pub async fn verify_cosign_signature(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
) -> AttestationResult<bool> {
    mise_sigstore::set_tuf_url(routed_tuf_url());
    mise_sigstore::verify_cosign_signature(artifact_path, sig_or_bundle_path).await
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
    use crate::env as mise_env;
    use confique::Layer;
    use std::sync::Mutex;

    const TOKEN_ENV_VARS: &[&str] = &[
        "MISE_GITHUB_TOKEN",
        "GITHUB_API_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_ENTERPRISE_TOKEN",
    ];
    static TEST_SETTINGS_LOCK: Mutex<()> = Mutex::new(());

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
            let lock = TEST_SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut settings = crate::config::settings::SettingsPartial::empty();
            settings.url_replacements = replacements;
            settings.use_versions_host = use_versions_host;
            crate::config::Settings::reset(Some(settings));
            Self { _lock: lock }
        }
    }

    impl Drop for SettingsGuard {
        fn drop(&mut self) {
            crate::config::Settings::reset(None);
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
        let _lock = crate::github::TEST_ENV_LOCK.lock().unwrap();
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
        let _lock = crate::github::TEST_ENV_LOCK.lock().unwrap();
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
        let _lock = crate::github::TEST_ENV_LOCK.lock().unwrap();
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
        let _lock = crate::github::TEST_ENV_LOCK.lock().unwrap();
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

    #[test]
    fn test_routed_api_url_applies_simple_url_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "https://api.github.com".to_string() => "https://github-proxy.example.com".to_string(),
        }));

        let routed = routed_api_url(crate::github::API_URL);

        assert_eq!(routed, "https://github-proxy.example.com/");
    }

    #[test]
    fn test_routed_api_url_applies_regex_url_replacement() {
        let _settings = SettingsGuard::new(Some(indexmap::indexmap! {
            "regex:^https://api\\.github\\.com".to_string() => "https://github-proxy.example.com/api".to_string(),
        }));

        let routed = routed_api_url(crate::github::API_URL);

        assert_eq!(routed, "https://github-proxy.example.com/api/");
    }

    #[test]
    fn test_routed_api_url_keeps_original_url_without_replacement() {
        let _settings = SettingsGuard::new(None);

        let routed = routed_api_url(crate::github::API_URL);

        assert_eq!(routed, crate::github::API_URL);
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
    fn test_use_versions_host_for_attestations_respects_setting() {
        let _settings = SettingsGuard::with_versions_host(None, Some(false));

        assert!(!use_versions_host_for_attestations(
            Some(crate::github::API_URL),
            true
        ));
    }

    #[test]
    fn test_use_versions_host_for_attestations_respects_registry_gate() {
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
