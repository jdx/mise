use std::collections::BTreeMap;

use eyre::Result;

use crate::config::{Config, Settings};
use crate::deps::DepsEngine;
use crate::toolset::{InstallOptions, ToolsetBuilder};

use super::parse_package_spec;

/// Add packages to the project's dependencies
///
/// Runs the package manager's own add command, which updates the project's
/// manifest (for npm, package.json) and its lockfile, not mise.toml. To install a
/// CLI tool from npm for mise to manage, use `mise use npm:<package>` instead.
/// Name each package as `<ecosystem>:<package>`, such as `npm:react` or
/// `npm:@types/react@19`. See
/// https://mise.jdx.dev/dev-tools/deps.html#adding-and-removing-packages.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise deps add npm:react", help = "Add react to package.json"),
    example("mise deps add -D npm:vitest", help = "Add vitest as a dev dependency")
)]
pub(super) struct DepsAdd {
    /// Packages to add, such as `npm:react` or `npm:@types/react@19`
    #[usage(required = true)]
    pub packages: Vec<String>,

    /// Add as a development dependency
    #[usage(long, short = 'D')]
    pub dev: bool,
}

impl DepsAdd {
    pub(super) async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("deps")?;

        let mut config = Config::get().await?;

        // Build and install toolset so tools like npm are available
        let mut ts = ToolsetBuilder::new()
            .with_default_to_latest(true)
            .build(&config)
            .await?;

        let install_opts = InstallOptions {
            missing_args_only: false,
            ..Default::default()
        };
        ts.install_missing_versions(&mut config, &install_opts)
            .await?;

        let (env, env_remove) = ts.env_with_path_and_removals(&config).await?;

        let project_root = config
            .project_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        // Group packages by ecosystem for batching
        let mut by_ecosystem: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for spec in &self.packages {
            let (ecosystem, package) = parse_package_spec(spec)?;
            by_ecosystem
                .entry(ecosystem.to_string())
                .or_default()
                .push(package.to_string());
        }

        for (ecosystem, packages) in &by_ecosystem {
            let provider = crate::deps::create_provider(ecosystem, &project_root, Some(&config))?;

            let pkg_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
            let cmd = provider.add_command(&pkg_refs, self.dev)?;
            DepsEngine::execute_command(&cmd, &env, &env_remove, provider.timeout(), None, None)?;
        }

        Ok(())
    }
}
