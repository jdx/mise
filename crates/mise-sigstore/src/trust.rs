use super::*;

/// Override the Sigstore public-good TUF URL (e.g. a mirror derived from mise's
/// `settings.url_replacements`). Passing a mirror URL still bootstraps from the
/// embedded production root ([`PRODUCTION_TUF_ROOT`]), so a mirror cannot forge
/// the chain of trust — TUF verifies all fetched metadata against that pinned
/// root. Passing `None` restores the default behavior.
pub fn set_tuf_url(url: Option<String>) {
    // Recover from a poisoned lock rather than silently dropping the override:
    // the guarded data is just a String, so a poisoned lock still holds a valid
    // value and we must still apply the (mirror) URL.
    let mut guard = TUF_URL_OVERRIDE.write().unwrap_or_else(|e| e.into_inner());
    *guard = url;
}

/// Build the [`TufConfig`] for the Sigstore public-good root, honoring an
/// optional URL override.
pub(crate) fn select_tuf_config(override_url: Option<String>) -> TufConfig {
    match override_url {
        // SECURITY: pin the embedded production root even when fetching from a
        // mirror. A custom URL has no embedded-root fallback, and the mirror
        // serves identical TUF content; bootstrapping with PRODUCTION_TUF_ROOT
        // means every metadata file is verified against the canonical root.
        Some(url) => TufConfig::custom(url).with_root(PRODUCTION_TUF_ROOT),
        // Equivalent to `TrustedRoot::production()` (which is itself
        // `from_tuf(TufConfig::production())`) — the default path is unchanged.
        None => TufConfig::production(),
    }
}

pub(crate) async fn production_trusted_root() -> Result<TrustedRoot> {
    let override_url = TUF_URL_OVERRIDE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Ok(TrustedRoot::from_tuf(select_tuf_config(override_url)).await?)
}

pub(crate) fn github_embedded_trusted_root() -> Result<TrustedRoot> {
    Ok(TrustedRoot::from_embedded(SigstoreInstance::GitHub)?)
}

pub(crate) async fn github_tuf_trusted_root() -> Result<TrustedRoot> {
    Ok(TrustedRoot::from_tuf(TufConfig::github()).await?)
}

/// Per-process cache so we only fetch the Sigstore TUF root or parse the
/// GitHub trusted roots once per `verify_*` invocation. Each is loaded lazily:
/// GitHub bundles use the embedded root first and only fetch GitHub's TUF root
/// after an embedded-root verification failure.
#[derive(Default)]
pub(crate) struct TrustRoots {
    sigstore: Option<TrustedRoot>,
    pub(crate) github_embedded: Option<TrustedRoot>,
    pub(crate) github_tuf: Option<TrustedRoot>,
}

impl TrustRoots {
    pub(crate) async fn sigstore_root(&mut self) -> Result<&TrustedRoot> {
        if self.sigstore.is_none() {
            self.sigstore = Some(production_trusted_root().await?);
        }
        Ok(self.sigstore.as_ref().unwrap())
    }

    pub(crate) fn github_embedded_root(&mut self) -> Result<&TrustedRoot> {
        if self.github_embedded.is_none() {
            self.github_embedded = Some(github_embedded_trusted_root()?);
        }
        Ok(self.github_embedded.as_ref().unwrap())
    }

    pub(crate) async fn github_tuf_root(&mut self) -> Result<&TrustedRoot> {
        if self.github_tuf.is_none() {
            self.github_tuf = Some(github_tuf_trusted_root().await?);
        }
        Ok(self.github_tuf.as_ref().unwrap())
    }
}

pub(crate) fn verify_signer_workflow_identity(
    identity: Option<&str>,
    signer_workflow: Option<&str>,
) -> Result<()> {
    let Some(expected) = signer_workflow else {
        return Ok(());
    };
    let Some(identity) = identity.filter(|identity| !identity.is_empty()) else {
        return Err(AttestationError::WorkflowMismatch(format!(
            "expected '{expected}', found no certificate identity"
        )));
    };
    if !identity.contains(expected) {
        return Err(AttestationError::WorkflowMismatch(format!(
            "expected '{expected}', found certificate identity: {identity:?}"
        )));
    }
    Ok(())
}
