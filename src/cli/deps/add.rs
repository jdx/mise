use eyre::Result;

use crate::config::{Config, Settings};
use crate::deps::DepsEngine;
use crate::toolset::{InstallOptions, ToolsetBuilder};

use super::parse_package_spec;

/// Add a dependency
///
/// Adds one or more packages to the project using the appropriate package manager.
/// Package specs use the format `ecosystem:package`, e.g., `npm:react` or `npm:@types/react@19`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct DepsAdd {
    /// Package(s) to add (e.g., npm:react, npm:@types/react@19)
    #[clap(required = true)]
    pub packages: Vec<String>,

    /// Add as a development dependency
    #[clap(long, short = 'D')]
    pub dev: bool,
}

impl DepsAdd {
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

        for spec in &self.packages {
            let (ecosystem, package) = parse_package_spec(spec)?;
            let provider = crate::deps::create_provider(ecosystem, &project_root)?;

            let cmd = provider.add_command(package, self.dev)?;
            DepsEngine::execute_command(&cmd, &env, None)?;
        }

        Ok(())
    }
}
