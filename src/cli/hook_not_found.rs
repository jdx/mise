use std::process::exit;

use eyre::Result;

use crate::config::{Config, Settings};
use crate::shell::ShellType;
use crate::toolset::ToolsetBuilder;

/// [internal] called by shell when a command is not found
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct HookNotFound {
    /// Shell type to generate script for
    #[clap(long, short)]
    shell: Option<ShellType>,

    /// Attempted bin to run
    #[clap()]
    bin: String,
}

impl HookNotFound {
    pub async fn run(self) -> Result<()> {
        let config = Config::try_get().await?;
        let settings = Settings::try_get()?;
        if settings.not_found_auto_install {
            let mut ts = ToolsetBuilder::new().build(&config)?;
            if ts.install_missing_bin(&self.bin)?.is_some() {
                return Ok(());
            }
        }
        exit(127);
    }
}
