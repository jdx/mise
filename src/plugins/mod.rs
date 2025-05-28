use crate::errors::Error::PluginNotInstalled;
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{config::Config, dirs};
use async_trait::async_trait;
use clap::Command;
use eyre::{Result, eyre};
use heck::ToKebabCase;
use regex::Regex;
pub use script_manager::{Script, ScriptManager};
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use std::vec;
use std::{
    fmt::{Debug, Display},
    sync::Arc,
};

pub mod asdf_plugin;
pub mod core;
pub mod mise_plugin_toml;
pub mod script_manager;
pub mod vfox_plugin;

#[derive(Debug, Clone, Copy, PartialEq, strum::EnumString, strum::Display)]
pub enum PluginType {
    Asdf,
    Vfox,
}

#[derive(Debug)]
pub enum PluginEnum {
    Asdf(Arc<AsdfPlugin>),
    Vfox(Arc<VfoxPlugin>),
}

impl PluginEnum {
    pub fn name(&self) -> &str {
        match self {
            PluginEnum::Asdf(plugin) => plugin.name(),
            PluginEnum::Vfox(plugin) => plugin.name(),
        }
    }

    pub fn path(&self) -> PathBuf {
        match self {
            PluginEnum::Asdf(plugin) => plugin.path(),
            PluginEnum::Vfox(plugin) => plugin.path(),
        }
    }

    pub fn get_plugin_type(&self) -> PluginType {
        match self {
            PluginEnum::Asdf(_) => PluginType::Asdf,
            PluginEnum::Vfox(_) => PluginType::Vfox,
        }
    }

    pub fn get_remote_url(&self) -> eyre::Result<Option<String>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.get_remote_url(),
            PluginEnum::Vfox(plugin) => plugin.get_remote_url(),
        }
    }

    pub fn set_remote_url(&self, url: String) {
        match self {
            PluginEnum::Asdf(plugin) => plugin.set_remote_url(url),
            PluginEnum::Vfox(plugin) => plugin.set_remote_url(url),
        }
    }

    pub fn current_abbrev_ref(&self) -> eyre::Result<Option<String>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.current_abbrev_ref(),
            PluginEnum::Vfox(plugin) => plugin.current_abbrev_ref(),
        }
    }

    pub fn current_sha_short(&self) -> eyre::Result<Option<String>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.current_sha_short(),
            PluginEnum::Vfox(plugin) => plugin.current_sha_short(),
        }
    }

    pub fn external_commands(&self) -> eyre::Result<Vec<Command>> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.external_commands(),
            PluginEnum::Vfox(plugin) => plugin.external_commands(),
        }
    }

    pub fn execute_external_command(&self, command: &str, args: Vec<String>) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.execute_external_command(command, args),
            PluginEnum::Vfox(plugin) => plugin.execute_external_command(command, args),
        }
    }

    pub async fn update(
        &self,
        pr: &Box<dyn SingleReport>,
        gitref: Option<String>,
    ) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.update(pr, gitref).await,
            PluginEnum::Vfox(plugin) => plugin.update(pr, gitref).await,
        }
    }

    pub async fn uninstall(&self, pr: &Box<dyn SingleReport>) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.uninstall(pr).await,
            PluginEnum::Vfox(plugin) => plugin.uninstall(pr).await,
        }
    }

    pub async fn install(
        &self,
        config: &Arc<Config>,
        pr: &Box<dyn SingleReport>,
    ) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.install(config, pr).await,
            PluginEnum::Vfox(plugin) => plugin.install(config, pr).await,
        }
    }

    pub fn is_installed(&self) -> bool {
        match self {
            PluginEnum::Asdf(plugin) => plugin.is_installed(),
            PluginEnum::Vfox(plugin) => plugin.is_installed(),
        }
    }

    pub fn is_installed_err(&self) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.is_installed_err(),
            PluginEnum::Vfox(plugin) => plugin.is_installed_err(),
        }
    }

    pub async fn ensure_installed(
        &self,
        config: &Arc<Config>,
        mpr: &MultiProgressReport,
        force: bool,
    ) -> eyre::Result<()> {
        match self {
            PluginEnum::Asdf(plugin) => plugin.ensure_installed(config, mpr, force).await,
            PluginEnum::Vfox(plugin) => plugin.ensure_installed(config, mpr, force).await,
        }
    }
}

impl PluginType {
    pub fn from_full(full: &str) -> eyre::Result<Self> {
        match full.split(':').next() {
            Some("asdf") => Ok(Self::Asdf),
            Some("vfox") => Ok(Self::Vfox),
            _ => Err(eyre!("unknown plugin type: {full}")),
        }
    }

    pub fn plugin(&self, short: String) -> PluginEnum {
        let path = dirs::PLUGINS.join(short.to_kebab_case());
        match self {
            PluginType::Asdf => PluginEnum::Asdf(Arc::new(AsdfPlugin::new(short, path))),
            PluginType::Vfox => PluginEnum::Vfox(Arc::new(VfoxPlugin::new(short, path))),
        }
    }
}

pub static VERSION_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|([abc])[0-9]+|snapshot|SNAPSHOT|master)"
    )
        .unwrap()
});

pub fn get(short: &str) -> Result<PluginEnum> {
    let (name, full) = short.split_once(':').unwrap_or((short, short));
    let plugin_type = if let Some(plugin_type) = install_state::list_plugins().get(short) {
        *plugin_type
    } else {
        PluginType::from_full(full)?
    };
    Ok(plugin_type.plugin(name.to_string()))
}

#[allow(unused_variables)]
#[async_trait]
pub trait Plugin: Debug + Send {
    fn name(&self) -> &str;
    fn path(&self) -> PathBuf;
    fn get_remote_url(&self) -> eyre::Result<Option<String>>;
    fn set_remote_url(&self, url: String) {}
    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>>;
    fn current_sha_short(&self) -> eyre::Result<Option<String>>;
    fn is_installed(&self) -> bool {
        true
    }
    fn is_installed_err(&self) -> eyre::Result<()> {
        if !self.is_installed() {
            return Err(PluginNotInstalled(self.name().to_string()).into());
        }
        Ok(())
    }

    async fn ensure_installed(
        &self,
        _config: &Arc<Config>,
        _mpr: &MultiProgressReport,
        _force: bool,
    ) -> eyre::Result<()> {
        Ok(())
    }
    async fn update(
        &self,
        _pr: &Box<dyn SingleReport>,
        _gitref: Option<String>,
    ) -> eyre::Result<()> {
        Ok(())
    }
    async fn uninstall(&self, _pr: &Box<dyn SingleReport>) -> eyre::Result<()> {
        Ok(())
    }
    async fn install(
        &self,
        _config: &Arc<Config>,
        _pr: &Box<dyn SingleReport>,
    ) -> eyre::Result<()> {
        Ok(())
    }
    fn external_commands(&self) -> eyre::Result<Vec<Command>> {
        Ok(vec![])
    }
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn execute_external_command(&self, _command: &str, _args: Vec<String>) -> eyre::Result<()> {
        unimplemented!(
            "execute_external_command not implemented for {}",
            self.name()
        )
    }
}

impl Ord for PluginEnum {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name().cmp(other.name())
    }
}

impl PartialOrd for PluginEnum {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for PluginEnum {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for PluginEnum {}

impl Display for PluginEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}
