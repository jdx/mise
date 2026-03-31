use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// List or sync project environment variables
///
/// Without flags, displays all project environment variables (values masked).
/// Use --pull to sync variables to your local .env file.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesEnv {
    /// Sync variables to local .env and replenish the credentials vault
    #[clap(long)]
    pull: bool,

    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesEnv {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);

        if self.pull {
            let count = server.env_pull()?;
            if self.json {
                let data = serde_json::json!({ "synced": count });
                let out = crate::sleeves::types::JsonOutput::success(&data);
                miseprintln!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                miseprintln!("Synced {} environment variables to .env", count);
            }
            return Ok(());
        }

        let vars = server.env()?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&vars);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        if vars.is_empty() {
            miseprintln!("No environment variables configured.");
            miseprintln!("Run `mise sleeves add <provider>/<service>` to provision resources.");
            return Ok(());
        }

        for v in &vars {
            miseprintln!(
                "  {} = {} (from {}/{})",
                v.key, v.masked_value, v.provider, v.service
            );
        }

        Ok(())
    }
}
