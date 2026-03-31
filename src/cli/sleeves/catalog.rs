use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// List available providers, categories, and services
///
/// Browse the service catalog to see providers, plan tiers, and pricing.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesCatalog {
    /// Filter by provider name or category
    filter: Option<String>,

    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesCatalog {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let providers = server.catalog(self.filter.as_deref());

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&providers);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        if providers.is_empty() {
            miseprintln!("No providers found matching '{}'.", self.filter.unwrap_or_default());
            return Ok(());
        }

        for p in &providers {
            miseprintln!(
                "{} — {}",
                p.name,
                p.categories.join(", ")
            );
            for s in &p.services {
                miseprintln!("  {}/{} — {}", p.name, s.service, s.description);
                for tier in &s.tiers {
                    miseprintln!(
                        "    [{}] {} — {}",
                        tier.name,
                        tier.price,
                        tier.features.join(", ")
                    );
                }
            }
        }

        Ok(())
    }
}
