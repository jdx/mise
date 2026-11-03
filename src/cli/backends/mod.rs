use eyre::Result;

mod ls;
mod switch;

#[derive(Debug, usage_rs::Args)]
#[usage(
    about = "Manage backends",
    long_about = LONG_ABOUT,
    aliases = ["b", "backend", "backend-list"],
    after_long_help = AFTER_LONG_HELP
)]
pub(crate) struct Backends {
    #[usage(subcommand)]
    command: Option<Commands>,
}

static LONG_ABOUT: &str = "Manage backends

A backend is where mise installs a tool from: a package registry such as npm or
cargo, a release host such as GitHub, or a plugin. Each tool uses one, either
written as a prefix (`npm:prettier`) or chosen by the registry. With no
subcommand, lists the built-in backends. See
https://mise.jdx.dev/dev-tools/backends/.";

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Deprecation:</underline></bold>

The `mise b` alias is deprecated and will be removed in mise 2027.4.0; use
`mise backends`.
"#
);

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Ls(ls::BackendsLs),
    Switch(switch::BackendsSwitch),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Ls(cmd) => cmd.run(),
            Self::Switch(cmd) => cmd.run().await,
        }
    }
}

impl Backends {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::BackendsLs {}));

        cmd.run().await
    }
}
