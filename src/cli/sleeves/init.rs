use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// Create a project and initialize AlteredCarbon Sleeves
///
/// Initializes a .projects/ directory to track provider accounts,
/// provisioned resources, and local project configuration.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesInit {
    /// Project name (defaults to current directory name)
    name: Option<String>,

    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesInit {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let name = self.name.unwrap_or_else(|| {
            root.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "my-app".to_string())
        });

        let server = SleevesServer::new(&root);
        let state = server.init(&name)?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&state);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            miseprintln!("Initialized project '{}'", state.name);
            miseprintln!("  State written to .projects/state.json");
            miseprintln!("  Run `mise sleeves add <provider>/<service>` to provision resources.");
        }
        Ok(())
    }
}
