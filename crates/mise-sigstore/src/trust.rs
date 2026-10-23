use super::*;

/// Override the Sigstore public-good TUF URL (e.g. a mirror derived from mise's
/// `settings.url_replacements`). Passing a mirror URL still bootstraps from the
/// embedded production root ([`SigstoreInstance::tuf_root`]), so a mirror cannot forge
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
pub(crate) fn select_tuf_config(override_url: Option<String>) -> Result<TufConfig> {
    let config = match override_url {
        // SECURITY: pin the embedded production root even when fetching from a
        // mirror. A custom URL has no embedded-root fallback, and the mirror
        // serves identical TUF content; bootstrapping with the production root
        // means every metadata file is verified against the canonical root.
        Some(url) => {
            let (url, credentials) = credentials_as_header(url)?;
            let config = TufConfig::custom(
                url,
                TufBootstrap::trusted(SigstoreInstance::PublicGood.tuf_root()),
            );
            match credentials {
                Some(credentials) => config.with_http_client(tuf_http_client(credentials)?),
                None => config,
            }
        }
        // Equivalent to `TrustedRoot::production()` (which is itself
        // `from_tuf(TufConfig::production())`) — the default path is unchanged.
        None => TufConfig::production(),
    };
    Ok(config)
}

/// Move a mirror URL's userinfo (a `url_replacements` target such as
/// `https://user:password@mirror/tuf`) into an `Authorization` header value.
/// The TUF client names its cache directory after the URL it is given and
/// repeats the URL in its errors, so it must only ever see the URL without it.
fn credentials_as_header(url: String) -> Result<(String, Option<HeaderValue>)> {
    let Ok(mut parsed) = reqwest::Url::parse(&url) else {
        return Ok((url, None));
    };
    if parsed.username().is_empty() && parsed.password().is_none() {
        return Ok((url, None));
    }
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8()
            .map(|s| s.into_owned())
            .map_err(|e| {
                AttestationError::TrustRoot(format!("invalid credentials in TUF URL: {e}"))
            })
    };
    let user = decode(parsed.username())?;
    let password = decode(parsed.password().unwrap_or_default())?;
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
    let mut value = HeaderValue::from_str(&format!("Basic {basic}"))
        .map_err(|e| AttestationError::TrustRoot(e.to_string()))?;
    value.set_sensitive(true);
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    Ok((parsed.to_string(), Some(value)))
}

/// The TUF client's own default HTTP client (the same timeouts, so a mirror
/// cannot hang the refresh), plus the mirror's credentials. reqwest drops the
/// header on a redirect to another host, as it did the userinfo.
fn tuf_http_client(credentials: HeaderValue) -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, credentials);
    mise_tls::build_client(|| {
        reqwest::Client::builder()
            .default_headers(headers.clone())
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .timeout(Duration::from_secs(120))
            .user_agent(sigstore_verify::types::USER_AGENT)
    })
    .map_err(|e| AttestationError::TrustRoot(e.to_string()))
}

pub(crate) async fn production_trusted_root() -> Result<TrustedRoot> {
    let override_url = TUF_URL_OVERRIDE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    TrustedRoot::from_tuf(select_tuf_config(override_url)?)
        .await
        .map_err(|e| AttestationError::TrustRoot(e.to_string()))
}

pub(crate) fn github_embedded_trusted_root() -> Result<TrustedRoot> {
    Ok(TrustedRoot::from_embedded(SigstoreInstance::GitHub)?)
}

pub(crate) async fn github_tuf_trusted_root() -> Result<TrustedRoot> {
    TrustedRoot::from_tuf(TufConfig::github())
        .await
        .map_err(|e| AttestationError::TrustRoot(e.to_string()))
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
    if !identity_ends_with_workflow(identity, expected) {
        return Err(AttestationError::WorkflowMismatch(format!(
            "expected '{expected}', found certificate identity: {identity:?}"
        )));
    }
    Ok(())
}

/// Whether the workflow path in a certificate identity ends with `expected`.
///
/// The identity is `https://github.com/<owner>/<repo>/<workflow path>@<ref>`.
/// `expected` may be the workflow file alone or include the repository, so it
/// must match a whole trailing run of path segments. Matching anywhere in the
/// identity would accept a longer file name (`release.yml.evil.yml`) or text in
/// the ref, which the signer controls.
fn identity_ends_with_workflow(identity: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    // A GitHub Actions ref always starts with `refs/`, and a workflow file name
    // cannot contain `/`, so the first `@refs/` is the real boundary even when the
    // file name contains `@`. Anything else falls back to the first `@`.
    let path = identity
        .split_once("@refs/")
        .or_else(|| identity.split_once('@'))
        .map_or(identity, |(path, _)| path);
    let Some(prefix) = path.strip_suffix(expected) else {
        return false;
    };
    prefix.is_empty() || prefix.ends_with('/') || expected.starts_with('/')
}
