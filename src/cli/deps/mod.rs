use eyre::Result;

mod add;
mod install;
mod remove;

/// [experimental] Manage project dependencies
///
/// Runs all applicable dependency install steps for the current project.
/// This checks if dependency lockfiles are newer than installed outputs
/// (e.g., package-lock.json vs node_modules/) and runs install commands
/// if needed.
///
/// Providers with `auto = true` are automatically invoked before `mise x` and `mise run`
/// unless skipped with the --no-deps flag.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "dep",
    alias = "prepare",
    verbatim_doc_comment,
    after_long_help = AFTER_LONG_HELP,
    example(r###"mise deps                    # Install all project dependencies
mise deps install            # Same as bare `mise deps`
mise deps install --force    # Force reinstall even if fresh
mise deps install --dry-run  # Show what would run
mise deps --monorepo         # Install deps from explicit monorepo config roots
mise deps add npm:react      # Add a dependency
mise deps add -D npm:vitest  # Add a dev dependency
mise deps remove npm:lodash  # Remove a dependency"###)
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
# Built-in npm provider (auto-detects lockfile)
[deps.npm]
auto = true              # Auto-run before mise x/run

# Custom provider
[deps.codegen]
auto = true
sources = ["schema/*.graphql"]
outputs = ["src/generated/"]
run = "npm run codegen"

# To disable npm instead, add `disable = ["npm"]` under [deps].
```"###
);
