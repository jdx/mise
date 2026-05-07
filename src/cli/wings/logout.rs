//! `mise wings logout` — revoke every active wings session for
//! the calling user, then delete the local credentials file.
//!
//! ## Why two steps
//!
//! The local file delete is unconditional; the proxy revoke
//! requires the Clerk session JWT (the same one used at login
//! time, since revoke is gated on Clerk-frontend identity, not
//! the wings session — a compromised wings session shouldn't be
//! able to lock the legitimate user out via a self-revoke).
//!
//! v1 doesn't keep the original Clerk session around, so the
//! proxy revoke needs the user to paste a fresh Clerk session
//! token (same UX as login). For convenience, `mise wings
//! logout --local-only` skips the server call and just deletes
//! the local file — the wings session JWT remains valid until
//! its `exp`, but the user can't refresh it from this machine
//! anymore.
//!
//! After both steps:
//!
//!   - Local installs stop authenticating to the wings catalog
//!     and registry (no credentials file to load).
//!   - The Redis revocation flag on the proxy makes any
//!     in-flight wings session 401 within seconds.
//!   - Refresh tokens are bulk-revoked on the proxy side, so
//!     `/auth/dev/refresh` 401s for this user across every
//!     machine they were signed in on.

use eyre::{Result, bail};

use crate::cli::wings::read_token_from_stdin;
use crate::wings::{client, credentials, device::DeviceKey};

/// Sign out of mise-wings.
///
/// Deletes the local credentials file. With `--token-stdin`
/// or `--token`, also POSTs to `/auth/dev/revoke` to invalidate
/// every wings session belonging to the calling user
/// (including ones on other machines).
///
/// Examples:
///
/// ```sh
/// $ mise wings logout --local-only
/// Local mise-wings credentials cleared.
/// ```
///
/// Revoke all server-side sessions for the current user:
///
/// ```sh
/// $ pbpaste | mise wings logout --token-stdin
/// Revoked every active mise-wings session for your user.
/// ```
///
/// Without a token, logout still clears local credentials:
///
/// ```sh
/// $ mise wings logout
/// Local mise-wings credentials cleared. Server-side revoke skipped (no Clerk session token).
/// To revoke every session for your user (including other machines), run:
///
/// mise wings logout --token-stdin
/// ```
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Logout {
    /// Skip the server-side revoke; only delete the local
    /// credentials file. Use this when you don't have a fresh
    /// Clerk session JWT handy and just want this machine
    /// signed out — the wings session JWT remains valid on
    /// the server until its `exp` (24 h default).
    #[clap(long, conflicts_with_all = ["token", "token_stdin"])]
    local_only: bool,
    /// Clerk session JWT for the server-side revoke. Same
    /// shape as `mise wings login --token`.
    #[clap(long, conflicts_with = "token_stdin")]
    token: Option<String>,
    /// Read the Clerk session JWT from stdin (avoids shell
    /// history). Same shape as `mise wings login --token-stdin`.
    #[clap(long)]
    token_stdin: bool,
}

impl Logout {
    pub async fn run(self) -> Result<()> {
        // Always delete local credentials, even if the server-
        // side revoke fails (or is skipped) — "logged out
        // locally" is the principal contract of the command.
        let had_local = credentials::cached().is_some();
        credentials::clear()?;
        DeviceKey::delete()?;

        if self.local_only {
            if had_local {
                miseprintln!("Local mise-wings credentials cleared.");
            } else {
                miseprintln!("No local mise-wings credentials to clear.");
            }
            return Ok(());
        }

        // `clap`'s `conflicts_with` rejects `--token` +
        // `--token-stdin` before `run()` — same shape as the
        // login subcommand. Greptile flagged the unreachable
        // `(Some(_), true)` arm on PR review.
        let token = match (self.token, self.token_stdin) {
            (Some(t), _) => t,
            (None, true) => read_token_from_stdin()?,
            (None, false) => {
                // No token supplied → can't do server revoke.
                // Treat as `--local-only`. Print a hint so the
                // user knows what they got.
                if had_local {
                    miseprintln!(
                        "Local mise-wings credentials cleared. \
                         Server-side revoke skipped (no Clerk session token).\n\
                         To revoke every session for your user (including other \
                         machines), run:\n\
                         \n    mise wings logout --token-stdin\n"
                    );
                } else {
                    miseprintln!("No local mise-wings credentials to clear.");
                }
                return Ok(());
            }
        };

        let token = token.trim();
        if token.is_empty() {
            bail!("Clerk session token is empty");
        }

        // Surface revoke errors but don't propagate as a
        // non-zero exit — the local clear above is the
        // principal effect; a flaky revoke should be visible
        // in the log without making `logout` fail loudly.
        match client::revoke(token).await {
            Ok(()) => miseprintln!("Revoked every active mise-wings session for your user."),
            Err(e) => miseprintln!(
                "Local credentials cleared, but server revoke failed: {e:#}.\n\
                 Re-run `mise wings logout --token-stdin` once you have a \
                 fresh Clerk session token."
            ),
        }
        Ok(())
    }
}
