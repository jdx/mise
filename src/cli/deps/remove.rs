use std::collections::BTreeMap;

use eyre::Result;

use crate::config::{Config, Settings};
use crate::deps::DepsEngine;
use crate::toolset::{InstallOptions, ToolsetBuilder};

use super::parse_package_spec;

/// Remove a dependency
///
/// Removes one or more packages from the project using the appropriate package manager.
/// Package specs use the format `ecosystem:package`, e.g., `npm:lodash`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct DepsRemove {
    /// Package(s) to remove (e.g., npm:lodash)
    #[clap(required = true)]
    pub packages: Vec<String>,
}

impl DepsRemove {
    pub async fn run(self) -> Result<()> {
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

        let env = ts.env_with_path(&config).await?;

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
            let cmd = provider.remove_command(&pkg_refs)?;
            DepsEngine::execute_command(&cmd, &env, provider.timeout(), None, None)?;
        }

        Ok(())
    }
}
