use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// Change the tier of a service
///
/// Upgrade (or downgrade) a provisioned resource to a different plan tier.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesUpgrade {
    /// Service identifier: "provider/service" or resource name
    service: String,

    /// Target tier name (e.g., pro, scaler, enterprise)
    #[clap(long, short)]
    tier: Option<String>,

    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesUpgrade {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let tier = self.tier.as_deref().unwrap_or("pro");
        let resource = server.upgrade(&self.service, tier)?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&resource);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            miseprintln!(
                "Upgraded '{}' to tier '{}'",
                resource.name, resource.tier
            );
        }
        Ok(())
    }
}
