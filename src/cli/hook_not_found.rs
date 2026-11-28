use crate::request_exit;

use eyre::Result;

use crate::config::config_file::is_path_trusted;
use crate::config::{Config, ConfigMap, Settings, is_global_config};
use crate::registry::{REGISTRY, tool_enabled};
use crate::shell::ShellType;
use crate::toolset::ToolsetBuilder;

use super::r#use::Use;

/// [internal] Handle a command the shell could not find
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
        // A mistyped command is not a request to run this directory's config, so an untrusted
        // config is left out and the trusted ones (global config) still decide what to
        // auto-install. No warning: hook-env already warns once on entering the directory.
        crate::config::skip_untrusted_configs();
        let mut config = Config::get().await?;
        let settings = Settings::try_get()?;
        if settings.not_found_auto_install {
            // A plain tool list loads without trust, but it is no consent to install, so only
            // trusted config decides which version is asked for. Merged in, an untrusted file's
            // request would take the place of global config's and leave nothing to install.
            let trusted: ConfigMap = config
                .config_files
                .iter()
                .filter(|(path, _)| is_global_config(path) || is_path_trusted(path))
                .map(|(path, cf)| (path.clone(), cf.clone()))
                .collect();
            let mut ts = ToolsetBuilder::new()
                .with_config_files(trusted)
                .build(&config)
                .await?;
            if ts
                .install_missing_bin(&mut config, &self.bin, false)
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
            *name == tool.short
                && tool.provides_bin(bin)
                && tool_enabled(enabled.as_ref(), &disabled, &tool.short.to_string())
                && !settings
                    .auto_install_disable_tools
                    .as_ref()
                    .is_some_and(|tools| tools.iter().any(|disabled| disabled == tool.short))
                && tool.is_supported_os()
                && !tool.backends().is_empty()
        })
        .map(|(_, tool)| tool.short);
    let provider = providers.next()?;
    providers.next().is_none().then_some(provider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_bin_provider_counts_aliases_once() {
        // ripgrep also has the `rg` registry alias, but both keys name one tool.
        assert_eq!(
            registry_bin_provider("rg", &Settings::default()),
            Some("ripgrep")
        );

        let settings = Settings {
            auto_install_disable_tools: Some(vec!["ripgrep".to_string()]),
            ..Default::default()
        };
        assert_eq!(registry_bin_provider("rg", &settings), None);

        // A registry backend can exist even when the tool excludes this OS.
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            registry_bin_provider("tfc-agent-core", &Settings::default()),
            None
        );
    }
}
