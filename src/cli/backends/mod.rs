use clap::Subcommand;
use eyre::Result;

mod ls;

#[derive(Debug, clap::Args)]
#[clap(
    about = "Manage backends",
    aliases = ["b", "backend", "backend-list"],
    after_long_help = AFTER_LONG_HELP
)]
pub struct Backends {
    #[clap(subcommand)]
    command: Option<Commands>,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Deprecation:</underline></bold>

The `mise b` alias is deprecated and will be removed in mise 2027.4.16.
Use `mise backends` instead.
"#
);

#[derive(Debug, Subcommand)]
enum Commands {
    Ls(ls::BackendsLs),
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Ls(cmd) => cmd.run(),
        }
    }
}

impl Backends {
    pub async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::BackendsLs {}));

        cmd.run()
    }
}
