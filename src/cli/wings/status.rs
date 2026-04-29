//! `mise wings status` — health check for the wings setup.
//!
//! Three things printed in order:
//!
//!   1. **Setting:** is `wings.enabled` true (opt-in), and
//!      which deployment is mise pointed at (prod / staging)?
//!   2. **Credentials:** are local credentials present?
//!      Expired? Refresh window expired? Or is this a CI run
//!      with GHA OIDC auto-detection in play?
//!   3. **Connectivity (best-effort):** hit
//!      `https://api.<host>/health` and report status. No
//!      401 surface here — the health endpoint is
//!      unauthenticated.
//!
//! The checks are read-only — running `status` doesn't trigger
//! a refresh or any other state mutation. A user troubleshooting
//! "wings doesn't seem to be active" gets every layer's view in
//! one place.

use eyre::Result;

use crate::config::Settings;
use crate::wings::{ci, credentials};

/// Show the current mise-wings configuration + auth state.
#[derive(Debug, Default, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Status {}

impl Status {
    pub async fn run(self) -> Result<()> {
        let settings = Settings::get();
        let wings = &settings.wings;

        // 1. Setting
        let enabled = if wings.enabled { "yes" } else { "no" };
        let host = crate::wings::host();
        let env_label = if wings.staging { " (staging)" } else { "" };
        miseprintln!("wings.enabled: {enabled}");
        miseprintln!("host:          {host}{env_label}");

        // 2. Credentials
        let dev_creds = credentials::cached();
        let ci_runner = ci::gha_runner_present();
        match (&dev_creds, ci_runner) {
            (Some(creds), _) => {
                miseprintln!(
                    "credentials:   user={} org={} (dev login)",
                    creds.user_id,
                    creds.org
                );
                if creds.refresh_token_expired() {
                    miseprintln!("               refresh token expired — re-login required");
                } else if creds.should_refresh(0) {
                    miseprintln!(
                        "               access token expired \
                         (will be auto-refreshed on next request)"
                    );
                } else {
                    let leeway_secs = 5 * 60;
                    if creds.should_refresh(leeway_secs) {
                        miseprintln!(
                            "               access token within refresh leeway \
                             ({}m) — will auto-refresh shortly",
                            leeway_secs / 60,
                        );
                    } else {
                        miseprintln!("               access token live");
                    }
                }
            }
            (None, true) => {
                miseprintln!(
                    "credentials:   GHA OIDC available (will auto-mint a CI session on first request)"
                );
            }
            (None, false) => {
                miseprintln!("credentials:   none");
                miseprintln!(
                    "\nRun `mise wings login` to sign in for local dev, or run \
                     this from a GitHub Actions workflow with \
                     `permissions: id-token: write` for CI auth."
                );
                return Ok(());
            }
        }

        // 3. Connectivity (best-effort)
        if wings.enabled {
            let url = format!("https://api.{host}/health");
            let client = reqwest::Client::builder()
                .timeout(settings.http_timeout())
                .user_agent(format!("mise/{}", env!("CARGO_PKG_VERSION")))
                .build()?;
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    miseprintln!("connectivity:  OK ({})", resp.status());
                }
                Ok(resp) => {
                    miseprintln!("connectivity:  reachable but {} ({url})", resp.status());
                }
                Err(e) => {
                    miseprintln!("connectivity:  ERROR — {e:#}");
                }
            }
        } else {
            miseprintln!(
                "connectivity:  skipped (wings.enabled = false). \
                 Set `MISE_WINGS_ENABLED=1` (or `wings.enabled = true` \
                 in `mise.toml`) to activate."
            );
        }

        Ok(())
    }
}
