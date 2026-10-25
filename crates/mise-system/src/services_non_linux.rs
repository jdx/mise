use eyre::{Result, bail};

use crate::host::Config;
use crate::resources::ResourcePlan;

pub use super::services_common::*;

#[derive(Clone, Debug)]
pub struct ServiceRequest {
    pub(super) name: String,
}

pub fn prepare_requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    reject_existing_user_units(config)?;
    Ok(compose_system_declarations(config)?
        .into_keys()
        .map(|name| ServiceRequest { name })
        .collect())
}

pub fn requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    reject_configured(config)
}

pub fn status_requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    prepare_requests_from_config(config)
}

pub fn inspect_requests(_requests: &mut [ServiceRequest]) {}

pub fn plans_with_notifications(
    _requests: &[ServiceRequest],
    _notifications: &ServiceNotifications,
) -> Vec<ResourcePlan> {
    vec![]
}

pub fn apply(_requests: &[ServiceRequest], _dry_run: bool, _yes: bool) -> Result<()> {
    Ok(())
}

pub fn apply_with_notifications(
    _requests: &[ServiceRequest],
    _notifications: &ServiceNotifications,
    _dry_run: bool,
    _yes: bool,
) -> Result<()> {
    Ok(())
}

pub fn apply_privileged_plan_from_stdin() -> Result<()> {
    bail!("bootstrap system services are only supported on Linux")
}

fn reject_configured(config: &Config) -> Result<Vec<ServiceRequest>> {
    // validated like on Linux first, so a forgotten `scope = "user"` is
    // reported as such rather than as a platform limitation
    if !compose_system_declarations(config)?.is_empty() {
        bail!("bootstrap system services are only supported on Linux");
    }
    reject_existing_user_units(config)?;
    Ok(vec![])
}

fn reject_existing_user_units(config: &Config) -> Result<()> {
    if !compose_existing_user_unit_declarations(config)?.is_empty() {
        bail!(
            "managing an existing systemd user unit is only supported on Linux; set `command` to define a user service"
        );
    }
    Ok(())
}
