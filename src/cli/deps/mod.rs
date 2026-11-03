use eyre::Result;

mod add;
mod install;
mod remove;

/// [experimental] Manage project dependencies
///
/// Installs project dependencies such as node_modules, separately from the tools
/// `mise install` manages. Each provider is one package-manager step: a built-in
/// one such as npm or uv, or a custom `[deps.<name>]` command. A provider runs only
/// when its inputs changed or its outputs are missing; `--explain` shows why.
///
/// With no subcommand, runs `mise deps install`. Providers with `auto = true` also
/// run before `mise exec` and `mise run` unless you pass --no-deps.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "dep",
    alias = "prepare",
    verbatim_doc_comment,
    after_long_help = AFTER_LONG_HELP,
    example("mise deps", help = "Run every provider that is out of date"),
    example(
        "mise deps install npm --explain",
        help = "Show why the npm provider would or would not run"
    ),
    example(
        "mise deps --monorepo",
        help = "Include providers from every monorepo config root"
    )
)]
pub(crate) struct Deps {
    #[usage(subcommand)]
    command: Option<Commands>,

    #[usage(flatten)]
    install: install::DepsInstall,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Add(add::DepsAdd),
    Install(install::DepsInstall),
    Remove(remove::DepsRemove),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Add(cmd) => cmd.run().await,
            Self::Install(cmd) => cmd.run().await,
            Self::Remove(cmd) => cmd.run().await,
        }
    }
}

impl Deps {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Install(self.install));

        cmd.run().await
    }
}

/// Parse a package spec like "npm:react" or "npm:@types/react@19" into (ecosystem, package)
pub(super) fn parse_package_spec(spec: &str) -> Result<(&str, &str)> {
    spec.split_once(':').ok_or_else(|| {
        eyre::eyre!(
            "invalid package spec '{spec}', expected format: ecosystem:package (e.g., npm:react)"
        )
    })
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r###"<bold><underline>Configuration:</underline></bold>

```toml
# Built-in npm provider; needs package.json and package-lock.json
[deps.npm]
auto = true              # run before mise exec and mise run

# Custom provider
[deps.codegen]
auto = true
sources = ["schema/*.graphql"]
outputs = ["src/generated/"]
run = "npm run codegen"
```

See https://mise.jdx.dev/dev-tools/deps.html."###
);
