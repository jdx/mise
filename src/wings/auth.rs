//! Shared wings session-token acquisition.
//!
//! Used by the catalog/registry install path and by any future
//! wings-specific API caller. A missing login/subscription returns
//! `Ok(None)` so `wings.enabled = true` stays an explicit opt-in
//! that only activates when mise can authenticate.

use eyre::Result;

use crate::wings::{client, credentials};

/// Leeway (seconds) before a wings access token's `exp` at which
/// auto-refresh triggers.
pub(crate) const REFRESH_LEEWAY_SECS: i64 = 5 * 60;

/// Return a usable wings session token for the current host.
pub async fn session_token() -> Result<Option<String>> {
    if !crate::config::Settings::get().wings.enabled {
        return Ok(None);
    }

    session_token_from_credentials().await
}

/// Return a usable wings session token for explicit CLI commands.
///
/// Unlike the install path, this intentionally does not check
/// `wings.enabled`: inspection commands are debugging tools, and
/// should still work after `mise wings login` when Wings installs
/// are disabled.
pub async fn session_token_for_cli() -> Result<Option<String>> {
    session_token_from_credentials().await
}

async fn session_token_from_credentials() -> Result<Option<String>> {
    let host = crate::wings::host();
    if let Some(creds) = credentials::cached() {
        if creds.host != host {
            log::debug!(
                "wings: stored credentials are for {}, but current host is {}; ignoring cached credentials",
                creds.host,
                host,
            );
        } else {
            return usable_access_token(creds).await;
        }
    }

    if crate::wings::ci::gha_runner_present() {
        return Ok(crate::wings::ci::cached_ci_token().await);
    }

    Ok(None)
}

async fn maybe_refresh(stale: &credentials::Credentials) -> Result<credentials::Credentials> {
    if stale.refresh_token_expired() {
        eyre::bail!(
            "wings refresh token expired ({}s ago); run `mise wings login`",
            crate::wings::now_unix() - stale.refresh_expires_at,
        );
    }
    let _guard = credentials::lock_refresh().await;

    let Some(current) = credentials::cached() else {
        eyre::bail!("wings credentials were cleared during refresh; skipping refresh");
    };
    if !current.should_refresh(REFRESH_LEEWAY_SECS) {
        return Ok(current);
    }
    if current.refresh_token_expired() {
        eyre::bail!(
            "wings refresh token expired ({}s ago); run `mise wings login`",
            crate::wings::now_unix() - current.refresh_expires_at,
        );
    }

    let next = client::refresh(&current).await?;
    credentials::store(next.clone())?;
    Ok(next)
}

async fn usable_access_token(creds: credentials::Credentials) -> Result<Option<String>> {
    if !creds.should_refresh(REFRESH_LEEWAY_SECS) {
        return Ok(Some(creds.access_token));
    }

    match maybe_refresh(&creds).await {
        Ok(fresh) => Ok(Some(fresh.access_token)),
        Err(e) => {
            log::warn!("wings: auto-refresh failed; skipping wings. Error: {e:#}");
            Ok(None)
        }
    }
}
