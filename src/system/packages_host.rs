//! Registers the pieces of mise the system package managers call back into: provisioning ruby,
//! finding a configured tool, and loading package plugins.

use std::path::PathBuf;
use std::sync::Arc;

use eyre::{Result, eyre};

use crate::config::Config;
use crate::system::package_plugin::PackagePluginManager;
use crate::system::packages::SystemPackageManager;
use crate::system::packages::host::{self, Host};
use crate::toolset::{InstallOptions, Toolset, ToolsetBuilder};

pub fn register() {
    host::register(Host {
        provision_ruby: || Box::pin(provision_ruby()),
        installed_ruby: || Box::pin(installed_ruby()),
        tool_bin: |name| {
            Box::pin(async move {
                let config = Config::get().await?;
                crate::backend::configured_toolset_or_path_which(&config, [name.to_string()], name)
                    .await
            })
        },
        package_plugin_names,
        load_package_plugin,
    });
}

fn package_plugin_names() -> Vec<String> {
    let Some(plugins) = crate::toolset::install_state::try_list_plugins() else {
        return Vec::new();
    };
    plugins
        .iter()
        .filter(|(_, plugin_type)| **plugin_type == crate::plugins::PluginType::Package)
        .map(|(name, _)| name.clone())
        .collect()
}

fn load_package_plugin(name: &str) -> Result<Arc<dyn SystemPackageManager>> {
    Ok(Arc::new(PackagePluginManager::new(name.to_string())?))
}

/// Ensure a mise-managed ruby is installed (precompiled by default) and
/// return the path to its `ruby` executable.
async fn provision_ruby() -> Result<PathBuf> {
    let mut config = Config::get().await?;
    let tool: crate::args::ToolArg = "ruby".parse()?;
    let mut ts = ToolsetBuilder::new()
        .with_args(&[tool])
        .with_default_to_latest(true)
        .build(&config)
        .await?;
    ts.install_missing_versions(
        &mut config,
        &InstallOptions {
            // only ruby — never drag the rest of the config's toolset in
            missing_args_only: true,
            reason: "brew source build".to_string(),
            ..Default::default()
        },
    )
    .await?;
    find_ruby_bin(&config, &ts).await?.ok_or_else(|| {
        eyre!("failed to provision ruby for building from source (try `mise install ruby`)")
    })
}

async fn installed_ruby() -> Result<Option<PathBuf>> {
    let config = Config::get().await?;
    let tool: crate::args::ToolArg = "ruby".parse()?;
    let ts = ToolsetBuilder::new()
        .with_args(&[tool])
        .with_default_to_latest(true)
        .build(&config)
        .await?;
    find_ruby_bin(&config, &ts).await
}

async fn find_ruby_bin(config: &Arc<Config>, ts: &Toolset) -> Result<Option<PathBuf>> {
    for (backend, tv) in ts.list_current_versions() {
        if tv.ba().short != "ruby" {
            continue;
        }
        for bin_dir in backend.list_bin_paths(config, &tv).await? {
            let ruby = bin_dir.join("ruby");
            if ruby.is_file() {
                return Ok(Some(ruby));
            }
        }
    }
    Ok(None)
}
