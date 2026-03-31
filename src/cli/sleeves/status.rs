use eyre::Result;

use crate::sleeves::server::SleevesServer;
use crate::sleeves::types::ResourceStatus;

/// View project name, services, tiers, and health
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesStatus {
    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesStatus {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let state = server.status()?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&state);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        miseprintln!("Project: {}", state.name);
        if let Some(ref id) = state.account_id {
            miseprintln!("Account: {}", id);
        }

        if state.providers.is_empty() {
            miseprintln!("\nNo provider accounts linked.");
        } else {
            miseprintln!("\nProvider Accounts:");
            for p in &state.providers {
                miseprintln!("  {} ({})", p.provider, p.display_name);
            }
        }

        let active: Vec<_> = state
            .resources
            .iter()
            .filter(|r| r.status != ResourceStatus::Removed)
            .collect();

        if active.is_empty() {
            miseprintln!("\nNo services provisioned.");
        } else {
            miseprintln!("\nServices:");
            for r in &active {
                miseprintln!(
                    "  {}/{} — tier: {} — status: {}",
                    r.provider, r.service, r.tier, r.status
                );
            }
        }

        // Health
        let health = server.health()?;
        if !health.is_empty() {
            miseprintln!("\nHealth:");
            for h in &health {
                let icon = if h.healthy { "ok" } else { "!!" };
                miseprintln!("  [{}] {}", icon, h.message);
            }
        }

        Ok(())
    }
}
