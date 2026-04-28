//! `mise wings whoami` — print the active wings credentials.
//!
//! Reads `MISE_STATE_DIR/wings/credentials.json` (loaded
//! lazily into the in-memory cache) and prints the user / org
//! / token expiries. No network calls — purely local. Useful
//! for confirming "yes I'm signed in" without hitting the
//! cache, and for debugging which Clerk org the local
//! credentials are scoped to.

use eyre::Result;

use crate::config::Settings;
use crate::wings::credentials;

/// Print the active mise-wings identity.
#[derive(Debug, Default, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Whoami {}

impl Whoami {
    pub async fn run(self) -> Result<()> {
        let Some(creds) = credentials::cached() else {
            miseprintln!("Not signed in to mise-wings. Run `mise wings login` to sign in.");
            return Ok(());
        };

        let host_setting = &Settings::get().wings.host;
        let host_note = if &creds.host == host_setting {
            String::new()
        } else {
            // Stamped credentials don't match the configured
            // wings host — typically because the user changed
            // `wings.host` between login and now. The token
            // won't validate against the new host's signing
            // key; surface this prominently so the user knows
            // to re-login.
            format!(
                "  (stamped against {}, but `wings.host` is now {} — re-login to refresh)",
                creds.host, host_setting,
            )
        };

        let access_remaining = humanize_remaining(creds.expires_at);
        let refresh_remaining = humanize_remaining(creds.refresh_expires_at);

        miseprintln!(
            "Signed in to mise-wings\n\
             \n  user:    {}\n  org:     {}\n  host:    {}{host_note}\n\
             \n  access:  expires in {access_remaining}\n  refresh: expires in {refresh_remaining}\
             ",
            creds.user_id,
            creds.org,
            creds.host,
        );
        Ok(())
    }
}

/// Format a unix-seconds expiry as a human-readable
/// "in 23 minutes" / "in 4 days" / "expired Xs ago" string.
/// Approximate by design — `whoami` is a status surface, not
/// an audit log, so 23 m vs 23 m 17 s noise isn't useful.
fn humanize_remaining(expires_at: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = expires_at - now;
    if delta < 0 {
        return format!("expired {} ago", humanize_duration(-delta));
    }
    humanize_duration(delta)
}

fn humanize_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_duration_picks_largest_unit() {
        assert_eq!(humanize_duration(45), "45s");
        assert_eq!(humanize_duration(60), "1m");
        assert_eq!(humanize_duration(120), "2m");
        assert_eq!(humanize_duration(3600), "1h");
        assert_eq!(humanize_duration(86_400), "1d");
        assert_eq!(humanize_duration(86_400 * 30), "30d");
    }
}
