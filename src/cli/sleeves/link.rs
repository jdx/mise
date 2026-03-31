use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// Connect a provider to your project without provisioning a resource
///
/// Useful in agent-driven workflows to establish a connection before provisioning.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesLink {
    /// Provider name (e.g., vercel, supabase, clerk)
    provider: String,

    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesLink {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let account = server.link(&self.provider)?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&account);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            miseprintln!("Linked provider '{}' (account: {})", account.provider, account.account_id);
        }
        Ok(())
    }
}
