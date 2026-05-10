//! `mise wings login` — enroll a device with mise-wings.
//!
//! The primary flow is device-code auth: mise creates a local
//! signing key, sends the public key to the API, then asks the user
//! to approve the short code in the dashboard. Refreshes are then
//! bound to signatures from that enrolled key.

use eyre::{Context, Result, bail};

use crate::wings::{client, credentials, device::DeviceKey};

/// Authenticate with mise-wings
///
/// By default, starts device-code auth and stores a device-bound
/// credential.
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
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Login {}

impl Login {
    pub async fn run(self) -> Result<()> {
        run_device_login().await
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
    let mut interval = std::time::Duration::from_secs(start.interval.max(1));
    loop {
        if crate::ui::ctrlc::is_cancelled() {
            bail!("wings device login cancelled");
        }
        if started.elapsed() >= timeout {
            bail!("wings device login expired; run `mise wings login` again");
        }
        match client::poll_device_login(&start.device_code).await {
            Ok(client::DevicePoll::Authorized(creds)) => {
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
            Ok(client::DevicePoll::Pending) => {
                tokio::time::sleep(interval).await;
            }
            Ok(client::DevicePoll::SlowDown) => {
                interval += std::time::Duration::from_secs(5);
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
