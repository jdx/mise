use eyre::{Result, bail};

use crate::config::Config;
use crate::system::resources::ResourcePlan;

pub(crate) use super::services_common::*;

#[derive(Clone, Debug)]
pub(crate) struct ServiceRequest {
    pub(super) name: String,
}

pub(crate) fn prepare_requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    Ok(compose_declarations(config)?
        .into_iter()
        .map(|(name, _)| ServiceRequest { name })
        .collect())
}

pub(crate) fn requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    reject_configured(config)
}

pub(crate) fn status_requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    prepare_requests_from_config(config)
}

pub(crate) fn inspect_requests(_requests: &mut [ServiceRequest]) {}

pub(crate) fn plans_with_notifications(
    _requests: &[ServiceRequest],
    _notifications: &ServiceNotifications,
) -> Vec<ResourcePlan> {
    vec![]
}

pub(crate) fn apply(_requests: &[ServiceRequest], _dry_run: bool, _yes: bool) -> Result<()> {
    Ok(())
}

pub(crate) fn apply_with_notifications(
    _requests: &[ServiceRequest],
    _notifications: &ServiceNotifications,
    _dry_run: bool,
    _yes: bool,
) -> Result<()> {
    Ok(())
}

pub(crate) fn apply_privileged_plan_from_stdin() -> Result<()> {
    bail!("bootstrap system services are only supported on Linux")
}

fn reject_configured(config: &Config) -> Result<Vec<ServiceRequest>> {
    let configured = config.bootstrap_config_maps().any(|config_files| {
        config_files.values().any(|cf| {
            cf.bootstrap_config()
                .is_some_and(|bootstrap| !bootstrap.services.is_empty())
        })
    });
    if configured {
        bail!("bootstrap system services are only supported on Linux");
    }
    Ok(vec![])
}
