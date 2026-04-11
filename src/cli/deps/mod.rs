use clap::Subcommand;
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
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "dep", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Deps {
    #[clap(subcommand)]
    command: Option<Commands>,

    #[clap(flatten)]
    install: install::DepsInstall,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Add(add::DepsAdd),
    Install(install::DepsInstall),
    Remove(remove::DepsRemove),
}

impl Commands {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Add(cmd) => cmd.run().await,
            Self::Install(cmd) => cmd.run().await,
            Self::Remove(cmd) => cmd.run().await,
        }
    }
}

impl Deps {
    pub async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Install(self.install));

        cmd.run().await
    }
}

/// Parse a package spec like "npm:react" or "npm:@types/react@19" into (ecosystem, package)
pub fn parse_package_spec(spec: &str) -> Result<(&str, &str)> {
    spec.split_once(':').ok_or_else(|| {
        eyre::eyre!(
            "invalid package spec '{spec}', expected format: ecosystem:package (e.g., npm:react)"
        )
    })
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise deps</bold>                    # Install all project dependencies
    $ <bold>mise deps install</bold>            # Same as bare `mise deps`
    $ <bold>mise deps install --force</bold>    # Force reinstall even if fresh
    $ <bold>mise deps install --dry-run</bold>  # Show what would run
    $ <bold>mise deps add npm:react</bold>      # Add a dependency
    $ <bold>mise deps add -D npm:vitest</bold>  # Add a dev dependency
    $ <bold>mise deps remove npm:lodash</bold>  # Remove a dependency

<bold><underline>Configuration:</underline></bold>

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

[deps]
disable = ["npm"]        # Disable specific providers at runtime
```
"#
);
