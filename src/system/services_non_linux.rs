use std::path::Path;

use eyre::{Result, bail};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::system::resources::{ResourceId, ResourcePlan};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    #[default]
    Running,
    Stopped,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceChangeAction {
    Reload,
    Restart,
    #[default]
    ReloadOrRestart,
    None,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServiceTomlConfig {
    #[serde(default)]
    pub state: ServiceState,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub masked: bool,
    #[serde(default)]
    pub on_change: ServiceChangeAction,
}

#[derive(Clone, Debug)]
pub struct ServiceRequest {
    name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServiceNotifications {
    sources: IndexMap<String, IndexSet<ResourceId>>,
}

impl ServiceNotifications {
    pub fn notify_file(&mut self, path: &Path, services: &[String]) {
        self.notify(ResourceId::new("file", path.to_string_lossy()), services);
    }

    pub fn notify_directory(&mut self, path: &Path, services: &[String]) {
        self.notify(
            ResourceId::new("directory", path.to_string_lossy()),
            services,
        );
    }

    #[cfg(test)]
    pub fn contains(&self, service: &str) -> bool {
        self.sources.contains_key(service)
    }

    fn notify(&mut self, source: ResourceId, services: &[String]) {
        for service in services {
            self.sources
                .entry(service.clone())
                .or_default()
                .insert(source.clone());
        }
    }
}

pub fn prepare_requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    let mut names = IndexSet::new();
    for cf in config.config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            for (name, service) in bootstrap.services {
                let _ = (
                    service.state,
                    service.enabled,
                    service.masked,
                    service.on_change,
                );
                names.insert(name);
            }
        }
    }
    Ok(names
        .into_iter()
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

pub fn validate_notifications(
    files: &[super::managed_files::ManagedFileRequest],
    directories: &[super::managed_files::ManagedDirectoryRequest],
    services: &[ServiceRequest],
) -> Result<()> {
    let configured = services
        .iter()
        .map(|service| service.name.as_str())
        .collect::<IndexSet<_>>();
    for (resource, notification) in files
        .iter()
        .flat_map(|file| {
            file.notify
                .iter()
                .map(move |notification| (file.path.as_path(), notification))
        })
        .chain(directories.iter().flat_map(|directory| {
            directory
                .notify
                .iter()
                .map(move |notification| (directory.path.as_path(), notification))
        }))
    {
        if !configured.contains(notification.as_str()) {
            bail!(
                "managed path '{}' notifies unconfigured bootstrap service '{}'",
                resource.display(),
                notification
            );
        }
    }
    Ok(())
}

pub fn apply_privileged_plan_from_stdin() -> Result<()> {
    bail!("bootstrap system services are only supported on Linux")
}

fn reject_configured(config: &Config) -> Result<Vec<ServiceRequest>> {
    let configured = config.config_files.values().any(|cf| {
        cf.bootstrap_config()
            .is_some_and(|bootstrap| !bootstrap.services.is_empty())
    });
    if configured {
        bail!("bootstrap system services are only supported on Linux");
    }
    Ok(vec![])
}

fn default_true() -> bool {
    true
}
