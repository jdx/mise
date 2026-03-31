use clap::Subcommand;
use eyre::Result;

mod add;
mod billing;
mod catalog;
mod env;
mod init;
mod link;
mod llm_context;
mod open;
mod remove;
mod rotate;
mod status;
mod upgrade;

/// AlteredCarbon Sleeves — manage third-party service accounts, provision resources, and sync credentials
///
/// Associate provider accounts, provision databases, auth, analytics, and more.
/// Credentials are stored in the vault and synced to your .env automatically.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Sleeves {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Add(add::SleevesAdd),
    Billing(billing::SleevesBilling),
    Catalog(catalog::SleevesCatalog),
    Env(env::SleevesEnv),
    Init(init::SleevesInit),
    Link(link::SleevesLink),
    LlmContext(llm_context::SleevesLlmContext),
    Open(open::SleevesOpen),
    Remove(remove::SleevesRemove),
    Rotate(rotate::SleevesRotate),
    Status(status::SleevesStatus),
    Upgrade(upgrade::SleevesUpgrade),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Add(cmd) => cmd.run().await,
            Self::Billing(cmd) => cmd.run().await,
            Self::Catalog(cmd) => cmd.run().await,
            Self::Env(cmd) => cmd.run().await,
            Self::Init(cmd) => cmd.run().await,
            Self::Link(cmd) => cmd.run().await,
            Self::LlmContext(cmd) => cmd.run().await,
            Self::Open(cmd) => cmd.run().await,
            Self::Remove(cmd) => cmd.run().await,
            Self::Rotate(cmd) => cmd.run().await,
            Self::Status(cmd) => cmd.run().await,
            Self::Upgrade(cmd) => cmd.run().await,
        }
    }
}

impl Sleeves {
    pub async fn run(self) -> Result<()> {
        self.command.run().await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise sleeves init my-app</bold>              Create a new project
    $ <bold>mise sleeves link vercel</bold>               Associate a provider account
    $ <bold>mise sleeves add clerk/auth</bold>            Add authentication service
    $ <bold>mise sleeves add posthog/analytics</bold>     Add analytics service
    $ <bold>mise sleeves status</bold>                    View project status
    $ <bold>mise sleeves env</bold>                       List environment variables
    $ <bold>mise sleeves catalog</bold>                   Browse available providers
    $ <bold>mise sleeves upgrade clerk/auth</bold>        Upgrade a service tier
"#
);
