use eyre::{Result, bail};
use serde::Deserialize;

use crate::config::Config;
use crate::system::resources::{ResourceAction, ResourceId, ResourcePlan};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FirewallTomlConfig {
    #[serde(flatten)]
    values: std::collections::HashMap<String, toml::Value>,
}

#[derive(Clone, Debug)]
pub struct FirewallRequest;

/// `mise bootstrap` and `mise bootstrap plan` skip the Linux-only firewall
/// section, so one config can be shared across hosts. Status commands still
/// report it as unsupported.
pub fn prepare_request_from_config(config: &Config) -> Result<Option<FirewallRequest>> {
    if configured(config) {
        warn_once!("ignoring [bootstrap.linux.firewall] on non-Linux host");
    }
    Ok(None)
}

/// The explicit `mise bootstrap firewall apply` still refuses to run here.
pub fn request_from_config(config: &Config) -> Result<Option<FirewallRequest>> {
    if configured(config) {
        bail!("bootstrap firewall management is only supported on Linux");
    }
    Ok(None)
}

pub fn status_request_from_config(config: &Config) -> Result<Option<FirewallRequest>> {
    if configured(config) {
        Ok(Some(FirewallRequest))
    } else {
        Ok(None)
    }
}

pub fn validate_request(_request: &FirewallRequest) -> Result<()> {
    Ok(())
}

pub fn inspect_request(_request: &mut FirewallRequest) -> Result<()> {
    Ok(())
}

impl FirewallRequest {
    pub fn plans(&self) -> Vec<ResourcePlan> {
        vec![ResourcePlan::new(
            ResourceId::new("firewall", "linux"),
            "unsupported platform",
            "configured Linux firewall",
            ResourceAction::Unknown,
        )]
    }
}

pub fn apply(_request: &FirewallRequest, _dry_run: bool, _yes: bool) -> Result<()> {
    bail!("bootstrap firewall management is only supported on Linux")
}

pub fn inspect_privileged_plan_from_stdin() -> Result<()> {
    bail!("bootstrap firewall management is only supported on Linux")
}

pub fn apply_privileged_plan_from_stdin() -> Result<()> {
    bail!("bootstrap firewall management is only supported on Linux")
}

fn configured(config: &Config) -> bool {
    config.config_files.values().any(|cf| {
        cf.bootstrap_config()
            .and_then(|bootstrap| bootstrap.linux.firewall)
            .is_some_and(|firewall| {
                let _ = firewall.values.len();
                true
            })
    })
}
