use eyre::Result;

mod ls;

#[derive(Debug, usage_rs::Args)]
#[usage(
    about = "Manage backends",
    aliases = ["b", "backend", "backend-list"],
    after_long_help = AFTER_LONG_HELP
)]
pub(crate) struct Backends {
    #[usage(subcommand)]
    command: Option<Commands>,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Deprecation:</underline></bold>

The `mise b` alias is deprecated and will be removed in mise 2027.4.0.
Use `mise backends` instead.
"#
);

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Ls(ls::BackendsLs),
}

impl Commands {
    pub(crate) fn run(self) -> Result<()> {
        match self {
            Self::Ls(cmd) => cmd.run(),
        }
    }
}

impl Backends {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::BackendsLs {}));

        cmd.run()
    }
}
