use std::path::Path;

use eyre::{Result, bail};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::config::{Config, ConfigMap};
use crate::system::resources::{ResourceId, ResourceOrigin, ResourcePlan};

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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
    let mut composed: IndexMap<String, (ServiceTomlConfig, ResourceOrigin)> = IndexMap::new();
    for config_files in config.bootstrap_config_maps() {
        for (name, declaration) in services_from_config_files(config_files) {
            if let Some(existing) = composed.get(&name) {
                if existing.0 == declaration.0 {
                    continue;
                }
                bail!(
                    "conflicting bootstrap service declarations for {name}\n\n  first:\n    {}\n\n  second:\n    {}",
                    existing.1.conflict_description(),
                    declaration.1.conflict_description(),
                );
            }
            composed.insert(name, declaration);
        }
    }
    Ok(composed
        .into_iter()
        .map(|(name, _)| ServiceRequest { name })
        .collect())
}

fn services_from_config_files(
    config_files: &ConfigMap,
) -> IndexMap<String, (ServiceTomlConfig, ResourceOrigin)> {
    let mut merged = IndexMap::new();
    for (path, cf) in config_files {
        if let Some(bootstrap) = cf.bootstrap_config() {
            let origin = ResourceOrigin {
                config: path.clone(),
                config_root: cf.config_root(),
                environment: crate::config::environments_for_config_path(path),
                source: None,
            };
            for (name, service) in bootstrap.services {
                merged
                    .entry(name)
                    .or_insert_with(|| (service, origin.clone()));
            }
        }
    }
    merged
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
