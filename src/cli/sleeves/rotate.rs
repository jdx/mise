use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// Rotate credentials for a service
///
/// Generates new credentials and updates the .env file.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesRotate {
    /// Service identifier: "provider/service" or resource name
    service: String,

    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesRotate {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let resource = server.rotate(&self.service)?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&resource);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            miseprintln!("Rotated credentials for '{}'", resource.name);
            miseprintln!("Updated environment variables:");
            for key in resource.env_vars.keys() {
                miseprintln!("  {}", key);
            }
        }
        Ok(())
    }
}
