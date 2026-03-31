use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// Remove a service from your project
///
/// Removes the resource and cleans up associated environment variables.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesRemove {
    /// Service identifier: "provider/service" or resource name
    service: String,

    /// Accept confirmation prompts automatically
    #[clap(long)]
    auto_confirm: bool,

    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesRemove {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let name = server.remove(&self.service)?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&name);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            miseprintln!("Removed service '{}'", name);
            miseprintln!("Environment variables updated in .env");
        }
        Ok(())
    }
}
