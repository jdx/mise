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
//! would be silently misrouted for GitHub Enterprise Server tenants.

use std::path::Path;

use mise_sigstore::sources::github::GitHubSource;
use mise_sigstore::{ArtifactRef, AttestationSource};

pub use mise_sigstore::AttestationError;

/// Result alias that matches `mise_sigstore`'s internal convention.
type AttestationResult<T> = std::result::Result<T, AttestationError>;

/// Resolve a GitHub token for an optional API base URL, defaulting to [`crate::github::API_URL`].
fn resolve_token_for_wrapper(api_url: Option<&str>) -> Option<String> {
    let url = api_url.unwrap_or(crate::github::API_URL);
    crate::github::resolve_token_for_api_url(url)
}

/// Verify a GitHub artifact attestation for a file on disk.
///
/// Dispatches to [`mise_sigstore::verify_github_attestation_with_base_url`] when
/// `api_url` is `Some` (to support GitHub Enterprise) and to
/// [`mise_sigstore::verify_github_attestation`] otherwise.
pub async fn verify_attestation(
    artifact_path: &Path,
    owner: &str,
    repo: &str,
    expected_workflow: Option<&str>,
    api_url: Option<&str>,
) -> AttestationResult<bool> {
    let token = resolve_token_for_wrapper(api_url);
    match api_url {
        Some(base_url) => {
            mise_sigstore::verify_github_attestation_with_base_url(
                artifact_path,
                owner,
                repo,
                token.as_deref(),
                expected_workflow,
                base_url,
            )
            .await
        }
        None => {
            mise_sigstore::verify_github_attestation(
                artifact_path,
                owner,
                repo,
                token.as_deref(),
                expected_workflow,
            )
            .await
        }
    }
}

/// Reason the pre-download attestation probe could not complete.
///
/// Preserved as two variants so callers can log distinct warnings for a misconfigured
/// endpoint (source creation) versus an API/network error (fetch). The original inline
/// pre-wrapper code at `src/backend/github.rs` emitted different messages for each; the
/// wrapper keeps that signal instead of flattening both into one error string.
#[derive(Debug)]
pub enum DetectError {
    /// `GitHubSource::with_base_url` rejected the (owner, repo, api_url) tuple — usually a
    /// malformed base URL.
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
) -> Result<bool, DetectError> {
    let token = resolve_token_for_wrapper(Some(api_url));
    let source = GitHubSource::with_base_url(owner, repo, token.as_deref(), api_url)
        .map_err(DetectError::SourceCreation)?;
    let artifact_ref = ArtifactRef::from_digest(digest);
    let attestations = source
        .fetch_attestations(&artifact_ref)
        .await
        .map_err(DetectError::Fetch)?;
    Ok(!attestations.is_empty())
}

/// Verify SLSA provenance for an already-downloaded artifact. Passthrough — no token needed.
pub async fn verify_slsa_provenance(
    artifact_path: &Path,
    provenance_path: &Path,
    min_level: u8,
) -> AttestationResult<bool> {
    mise_sigstore::verify_slsa_provenance(artifact_path, provenance_path, min_level).await
}

/// Verify a keyless Cosign signature or bundle. Passthrough — no token needed.
pub async fn verify_cosign_signature(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
) -> AttestationResult<bool> {
    mise_sigstore::verify_cosign_signature(artifact_path, sig_or_bundle_path).await
}

/// Verify a Cosign signature against a public key. Passthrough — no token needed.
pub async fn verify_cosign_signature_with_key(
    artifact_path: &Path,
    sig_or_bundle_path: &Path,
    public_key_path: &Path,
) -> AttestationResult<bool> {
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

    const TOKEN_ENV_VARS: &[&str] = &[
        "MISE_GITHUB_TOKEN",
        "GITHUB_API_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_ENTERPRISE_TOKEN",
    ];

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
}
