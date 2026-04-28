//! `mise wings login` — exchange a Clerk frontend session JWT
//! for a wings session.
//!
//! ## Flow (v1, paste-token)
//!
//! v1 ships a manual paste path: the user signs into the wings
//! dashboard at `https://<wings.host>/app`, copies their Clerk
//! session token from a "CLI sign-in" page (to be added in a
//! follow-up dashboard PR), and pastes it into:
//!
//! ```sh
//! mise wings login --token <clerk-session-jwt>
//! ```
//!
//! `mise wings login` without the flag prints the URL to open
//! and the next-step hint, so the affordance is discoverable
//! even before the dashboard's CLI page lands.
//!
//! ## Flow (v2, browser-callback — future)
//!
//! The eventual primary path is a browser-callback handshake:
//!
//!   1. CLI spawns a localhost HTTP listener on a free port.
//!   2. CLI opens the dashboard at `…/cli-auth?cli_callback=…&state=…`.
//!   3. Dashboard signs the user in (Clerk SDK, browser-side).
//!   4. Dashboard POSTs `{token: <clerk-jwt>}` to the local
//!      callback URL.
//!   5. CLI receives, validates `state`, exchanges at `/auth/dev`.
//!
//! That path requires the dashboard `/cli-auth` route to land
//! first; pinning the v1 manual flow here keeps the CLI
//! shippable today and gives the dashboard time to ship its
//! half. Greptile-style: not pretending we have UX we don't.

use eyre::{Context, Result, bail};

use crate::config::Settings;
use crate::wings::{client, credentials};

/// Authenticate with mise-wings
///
/// In v1, `--token <jwt>` is required: paste the Clerk session
/// token from the wings dashboard. The flagless invocation
/// prints a hint + the dashboard URL to open. A browser-flow
/// `mise wings login` lands in a follow-up PR once the
/// dashboard's `/cli-auth` page ships.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Login {
    /// Clerk frontend session JWT, pasted from the dashboard's
    /// "CLI sign-in" page. Use `--token-stdin` to read from
    /// stdin instead of the command line — important so a
    /// secret token doesn't land in shell history.
    #[clap(long, conflicts_with = "token_stdin")]
    token: Option<String>,
    /// Read the Clerk session JWT from stdin (one line, no
    /// surrounding whitespace). Preferred over `--token` for
    /// scripts and for hands-on use — keeps the secret out of
    /// the shell's history file.
    #[clap(long)]
    token_stdin: bool,
}

impl Login {
    pub async fn run(self) -> Result<()> {
        let host = &Settings::get().wings.host;
        let token = match (self.token, self.token_stdin) {
            (Some(t), false) => t,
            (None, true) => read_token_from_stdin()?,
            (None, false) => {
                miseprintln!(
                    "To sign in, open https://app.{host}/ in your browser, copy the\n\
                     CLI session token from the \"Sign in to mise CLI\" page, then run:\n\
                     \n    mise wings login --token-stdin\n\
                     \nor pass the token directly with `mise wings login --token <jwt>`.\n\
                     \n(Browser-callback `mise wings login` is a follow-up PR pending the\n\
                     dashboard's CLI-sign-in page.)"
                );
                return Ok(());
            }
            (Some(_), true) => bail!("--token and --token-stdin are mutually exclusive"),
        };

        let token = token.trim();
        if token.is_empty() {
            bail!("Clerk session token is empty");
        }
        if token.split('.').count() != 3 {
            // JWT shape is 3 dot-separated segments. A non-JWT
            // string would 401 at the proxy anyway; failing
            // fast with a clear message saves a network round
            // trip and gives a better error than the proxy's
            // "invalid clerk session token" generic.
            bail!(
                "value passed to --token doesn't look like a JWT \
                 (expected three dot-separated segments). Make sure \
                 you copied the *session* JWT, not a publishable \
                 key or template variable."
            );
        }

        let creds = client::exchange_clerk_session(token)
            .await
            .wrap_err("exchanging Clerk session for wings session")?;
        let user_id = creds.user_id.clone();
        let org = creds.org.clone();
        credentials::store(creds)?;

        miseprintln!(
            "Signed in to mise-wings as {user_id} ({org}).\n\
             Set `wings.enabled = true` (or `MISE_WINGS_ENABLED=1`) to start \
             routing tool installs through the cache."
        );
        Ok(())
    }
}

/// Read one line from stdin, trim trailing newline + spaces.
/// Used by `--token-stdin` so secrets don't land in shell
/// history. Returns `Err` only on stdin read failure (EOF
/// returns an empty string, which the caller rejects with a
/// clearer message than "stdin closed unexpectedly").
fn read_token_from_stdin() -> Result<String> {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .wrap_err("reading stdin")?;
    Ok(line.trim().to_owned())
}
