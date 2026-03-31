use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// Generate combined LLM context from project and provider data
///
/// Writes a local file that combines your project context with all
/// provider-supplied LLM context files.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "llm", verbatim_doc_comment)]
pub struct SleevesLlmContext {
    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesLlmContext {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let ctx = server.llm_context()?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&ctx);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            miseprintln!("{}", ctx);
            miseprintln!("Context written to .projects/llm-context.md");
        }
        Ok(())
    }
}
