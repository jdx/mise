use clap::Subcommand;
use eyre::Result;

use crate::sleeves::server::SleevesServer;

/// Manage billing and payment methods
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesBilling {
    #[clap(subcommand)]
    command: BillingCommands,
}

#[derive(Debug, Subcommand)]
enum BillingCommands {
    /// View the current payment method on file
    Show(BillingShow),
    /// Add or replace a payment method
    Add(BillingAdd),
}

#[derive(Debug, clap::Args)]
pub struct BillingShow {
    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

#[derive(Debug, clap::Args)]
pub struct BillingAdd {
    /// Return output as structured JSON
    #[clap(long)]
    json: bool,
}

impl SleevesBilling {
    pub async fn run(self) -> Result<()> {
        match self.command {
            BillingCommands::Show(cmd) => cmd.run().await,
            BillingCommands::Add(cmd) => cmd.run().await,
        }
    }
}

impl BillingShow {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let method = server.billing_show()?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&method);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        match method {
            Some(m) => {
                miseprintln!("Payment method: {} ending in {}", m.method_type, m.last_four);
                miseprintln!("Expires: {}", m.expiry);
            }
            None => {
                miseprintln!("No payment method on file.");
                miseprintln!("Run `mise sleeves billing add` to add one.");
            }
        }
        Ok(())
    }
}

impl BillingAdd {
    pub async fn run(self) -> Result<()> {
        let root = std::env::current_dir()?;
        let server = SleevesServer::new(&root);
        let method = server.billing_add()?;

        if self.json {
            let out = crate::sleeves::types::JsonOutput::success(&method);
            miseprintln!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            miseprintln!(
                "Payment method added: {} ending in {} (expires {})",
                method.method_type, method.last_four, method.expiry
            );
        }
        Ok(())
    }
}
