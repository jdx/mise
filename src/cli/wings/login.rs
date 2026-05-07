//! `mise wings login` — enroll a device with mise-wings.
//!
//! The primary flow is device-code auth: mise creates a local
//! signing key, sends the public key to the API, then asks the user
//! to approve the short code in the dashboard. Refreshes are then
//! bound to signatures from that enrolled key.

use eyre::{Context, Result, bail};

use crate::cli::wings::read_token_from_stdin;
use crate::wings::{client, credentials, device::DeviceKey};

/// Authenticate with mise-wings
///
/// By default, starts device-code auth and stores a device-bound
/// credential. `--token` remains as a legacy/manual fallback for
/// internal debugging.
///
/// Examples:
///
/// ```sh
/// $ mise wings login
/// To sign in to mise-wings, open:
///
/// https://app.mise-wings.en.dev/cli-device?code=AB12CD34
///
/// Enter code: AB12-CD34
/// Waiting for browser approval...
/// Signed in to mise-wings as user_123 (acme).
/// ```
///
/// Preferred token flow:
///
/// ```sh
/// $ pbpaste | mise wings login --token-stdin
/// Signed in to mise-wings as user_123 (acme).
/// Set `wings.enabled = true` (or `MISE_WINGS_ENABLED=1`) to start routing tool installs through the cache.
/// ```
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
    /// surrounding whitespace). Preferred over `--token` because
    /// the secret won't show up in shell history.
    #[clap(long)]
    token_stdin: bool,
}

impl Login {
    pub async fn run(self) -> Result<()> {
        // `clap`'s `conflicts_with` rejects `--token` +
        // `--token-stdin` before we ever reach `run()`, so
        // the only three reachable shapes are: token only,
        // stdin only, neither. Greptile flagged the previous
        // unreachable `(Some(_), true)` arm on PR review.
        let token = match (self.token, self.token_stdin) {
            (Some(t), _) => t,
            (None, true) => read_token_from_stdin()?,
            (None, false) => {
                return run_device_login().await;
            }
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

async fn run_device_login() -> Result<()> {
    let key = DeviceKey::load_or_generate()?;
    let start = client::start_device_login(&key)
        .await
        .wrap_err("starting wings device login")?;
    miseprintln!(
        "To sign in to mise-wings, open:\n\n{}\n\nIf needed, open {} and enter code: {}\nWaiting for browser approval...",
        start.verification_uri_complete,
        start.verification_uri,
        format_user_code(&start.user_code),
    );

    let started = tokio::time::Instant::now();
    let timeout = std::time::Duration::from_secs(start.expires_in);
    let interval = std::time::Duration::from_secs(start.interval.max(1));
    loop {
        if crate::ui::ctrlc::is_cancelled() {
            bail!("wings device login cancelled");
        }
        if started.elapsed() >= timeout {
            bail!("wings device login expired; run `mise wings login` again");
        }
        match client::poll_device_login(&start.device_code).await {
            Ok(Some(creds)) => {
                let user_id = creds.user_id.clone();
                let org = creds.org.clone();
                credentials::store(creds)?;
                miseprintln!(
                    "Signed in to mise-wings as {user_id} ({org}).\n\
                     Set `wings.enabled = true` (or `MISE_WINGS_ENABLED=1`) to start \
                     routing tool installs through the cache."
                );
                return Ok(());
            }
            Ok(None) => {
                tokio::time::sleep(interval).await;
            }
            Err(e) => return Err(e).wrap_err("polling wings device login"),
        }
    }
}

fn format_user_code(code: &str) -> String {
    let compact: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if compact.len() == 8 {
        format!("{}-{}", &compact[..4], &compact[4..])
    } else {
        code.to_string()
    }
}
