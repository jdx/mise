use eyre::{Result, bail};

use crate::sleeves::server::SleevesServer;

/// Add a service to your project
///
/// Provisions a resource in your provider account (e.g., database, auth, analytics).
/// If the provider is not yet linked, it will be linked automatically.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesAdd {
    /// Service to add in "provider/service" format (e.g., clerk/auth, posthog/analytics)
    service: String,

    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesAdd {
    pub async fn run(self) -> Result<()> {
        let (provider, service) = match self.service.split_once('/') {
            Some((p, s)) => (p.to_string(), s.to_string()),
            None => bail!(
                "Service must be in 'provider/service' format (e.g., clerk/auth).\n\
                 Run `mise sleeves catalog {}` to see available services.",
                self.service
            ),
        };

        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let resource = server.add(&provider, &service)?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&resource);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            miseprintln!("Added {}/{} (tier: {})", provider, service, resource.tier);
            miseprintln!("Resource: {} ({})", resource.name, resource.resource_id);
            miseprintln!("\nEnvironment variables synced to .env:");
            for key in resource.env_vars.keys() {
                miseprintln!("  {}", key);
            }
        }
        Ok(())
    }
}
