use crate::request_exit;

use eyre::Result;

use crate::config::{Config, Settings};
use crate::registry::{REGISTRY, tool_enabled};
use crate::shell::ShellType;
use crate::toolset::ToolsetBuilder;

use super::r#use::Use;

/// [internal] called by shell when a command is not found
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true)]
pub(crate) struct HookNotFound {
    /// Attempted bin to run
    #[usage()]
    bin: String,

    /// Shell type to generate script for
    #[usage(long, short, value_enum)]
    shell: Option<ShellType>,
}

impl HookNotFound {
    pub(crate) async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        let settings = Settings::try_get()?;
        if settings.not_found_auto_install {
            let mut ts = ToolsetBuilder::new().build(&config).await?;
            if ts
                .install_missing_bin(&mut config, &self.bin)
                .await?
                .is_some()
            {
                return Ok(());
            }
            if settings.not_found_auto_install_registry
                && let Some(tool) = registry_bin_provider(&self.bin, &settings)
                && !ts
                    .list_current_versions()
                    .into_iter()
                    .any(|(_, version)| version.ba().short == tool)
            {
                Use::use_global_registry_tool(tool).await?;
                return Ok(());
            }
        }
        Err(request_exit(127))
    }
}

fn registry_bin_provider(bin: &str, settings: &Settings) -> Option<&'static str> {
    let enabled = settings.enable_tools();
    let disabled = settings.disable_tools();
    let mut providers = REGISTRY
        .iter()
        .filter(|(name, tool)| {
            tool.provides_bin(bin)
                && tool_enabled(enabled.as_ref(), &disabled, &name.to_string())
                && !settings
                    .auto_install_disable_tools
                    .as_ref()
                    .is_some_and(|tools| tools.iter().any(|disabled| disabled == name))
                && !tool.backends().is_empty()
        })
        .map(|(name, _)| name);
    let provider = providers.next()?;
    providers.next().is_none().then_some(provider)
}
