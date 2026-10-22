//! Bootstrap service declarations that are parsed the same way on every
//! platform. `services` (Linux) and `services_non_linux` re-export these.

use std::path::Path;

use eyre::{Result, bail};
use indexmap::{IndexMap, IndexSet};

use crate::host::{Config, ConfigMap};
use crate::resources::{ResourceId, ResourceOrigin};
use crate::services::ServiceRequest;
pub use mise_config::services_common::{
    ServiceChangeAction, ServiceRestart, ServiceScope, ServiceState, ServiceTomlConfig,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServiceNotifications {
    pub(super) sources: IndexMap<String, IndexSet<ResourceId>>,
}

impl ServiceNotifications {
    pub fn extend(&mut self, other: Self) {
        for (service, sources) in other.sources {
            self.sources.entry(service).or_default().extend(sources);
        }
    }

    pub(crate) fn notify_file(&mut self, path: &Path, services: &[String]) {
        self.notify(ResourceId::new("file", path.to_string_lossy()), services);
    }

    pub(crate) fn notify_directory(&mut self, path: &Path, services: &[String]) {
        self.notify(
            ResourceId::new("directory", path.to_string_lossy()),
            services,
        );
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, service: &str) -> bool {
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

/// The system-scope entries of `[bootstrap.services]`, validated: fields that
/// only apply to user services are rejected here so a typo in `scope` cannot
/// silently turn a user service into a lookup of a system unit.
pub(crate) fn compose_system_declarations(
    config: &Config,
) -> Result<IndexMap<String, (ServiceTomlConfig, ResourceOrigin)>> {
    let mut out = IndexMap::new();
    for (name, (declaration, origin)) in compose_declarations(config)? {
        if declaration.scope() != ServiceScope::System {
            continue;
        }
        let user_only = declaration.user_only_fields();
        if !user_only.is_empty() {
            bail!(
                "bootstrap service '{name}' sets {}, which only applies to `scope = \"user\"` services",
                user_only.join(", ")
            );
        }
        out.insert(name, (declaration, origin));
    }
    Ok(out)
}

/// The user-scope entries of `[bootstrap.services]`.
pub fn compose_user_declarations(
    config: &Config,
) -> Result<IndexMap<String, (ServiceTomlConfig, ResourceOrigin)>> {
    Ok(compose_declarations(config)?
        .into_iter()
        .filter(|(_, (declaration, _))| declaration.scope() == ServiceScope::User)
        .collect())
}

/// Names of every user-scope service, for notification validation.
pub fn user_service_names(config: &Config) -> Result<Vec<String>> {
    Ok(compose_user_declarations(config)?.into_keys().collect())
}

/// `[bootstrap.services]` from every config map, rejecting redeclarations that
/// disagree.
pub(crate) fn compose_declarations(
    config: &Config,
) -> Result<IndexMap<String, (ServiceTomlConfig, ResourceOrigin)>> {
    let mut composed: IndexMap<String, (ServiceTomlConfig, ResourceOrigin)> = IndexMap::new();
    for config_files in config.bootstrap_config_maps() {
        for (name, declaration) in services_from_config_files(&config_files) {
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
    Ok(composed)
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
                environment: crate::host::environments_for_config_path(path),
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

pub fn validate_notifications(
    files: &[super::managed_files::ManagedFileRequest],
    directories: &[super::managed_files::ManagedDirectoryRequest],
    services: &[ServiceRequest],
    user_services: &[String],
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
        if user_services.iter().any(|name| name == notification) {
            bail!(
                "managed path '{}' notifies user-scope bootstrap service '{}'; notifications apply to system services only",
                resource.display(),
                notification
            );
        }
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

#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn merges_notifications_from_both_file_phases() {
        let mut early = ServiceNotifications::default();
        early.notify_file(Path::new("/etc/early"), &["web".into()]);
        let mut late = ServiceNotifications::default();
        late.notify_file(Path::new("/etc/late"), &["web".into(), "worker".into()]);
        early.extend(late);
        assert_eq!(early.sources["web"].len(), 2);
        assert_eq!(early.sources["worker"].len(), 1);
    }
}
