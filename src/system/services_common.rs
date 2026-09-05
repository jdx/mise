//! Bootstrap service declarations that are parsed the same way on every
//! platform. `services` (Linux) and `services_non_linux` re-export these.

use std::path::Path;

use eyre::{Result, bail};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::config::{Config, ConfigMap};
use crate::system::resources::{ResourceId, ResourceOrigin};
use crate::system::services::ServiceRequest;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServiceState {
    #[default]
    Running,
    Stopped,
    /// User scope only: the installed service definition is removed.
    Absent,
}

impl ServiceState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Absent => "absent",
        }
    }
}

/// Which service manager a `[bootstrap.services]` entry targets.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServiceScope {
    /// An existing Linux systemd system unit (the original behaviour).
    #[default]
    System,
    /// A service mise defines for the current user: a systemd user unit on
    /// Linux, a LaunchAgent on macOS, a Scheduled Task on Windows.
    User,
}

/// Restart policy of a user-scope service, with one meaning on every platform.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServiceRestart {
    /// Restart whenever the process exits, even successfully.
    Always,
    /// Restart only after a failure.
    #[default]
    OnFailure,
    /// Never restart automatically.
    Never,
}

impl ServiceRestart {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::OnFailure => "on-failure",
            Self::Never => "never",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServiceChangeAction {
    Reload,
    Restart,
    #[default]
    ReloadOrRestart,
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct ServiceTomlConfig {
    #[serde(default)]
    pub state: ServiceState,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub masked: bool,
    #[serde(default)]
    pub on_change: ServiceChangeAction,
    /// `"system"` (default) or `"user"`. A `builtin` implies `"user"`.
    #[serde(default)]
    pub scope: Option<ServiceScope>,
    /// A service definition mise supplies (for example `"history-watch"`).
    #[serde(default)]
    pub builtin: Option<String>,
    /// User scope: the command line to run.
    #[serde(default)]
    pub command: Option<String>,
    /// User scope: a human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// User scope: restart policy (default `"on-failure"`).
    #[serde(default)]
    pub restart: Option<ServiceRestart>,
    /// User scope: environment variables for the process.
    #[serde(default)]
    pub environment: IndexMap<String, String>,
    /// User scope: working directory (`~` is expanded).
    #[serde(default)]
    pub working_directory: Option<String>,
    /// User scope: converge after `[tools]` are installed.
    #[serde(default)]
    pub requires_tools: bool,
}

impl Default for ServiceTomlConfig {
    fn default() -> Self {
        Self {
            state: ServiceState::Running,
            enabled: true,
            masked: false,
            on_change: ServiceChangeAction::default(),
            scope: None,
            builtin: None,
            command: None,
            description: None,
            restart: None,
            environment: IndexMap::new(),
            working_directory: None,
            requires_tools: false,
        }
    }
}

impl ServiceTomlConfig {
    /// The effective scope: explicit `scope`, else `"user"` when a `builtin`
    /// is named, else `"system"`.
    pub(crate) fn scope(&self) -> ServiceScope {
        self.scope.unwrap_or(if self.builtin.is_some() {
            ServiceScope::User
        } else {
            ServiceScope::System
        })
    }

    /// Fields that only apply to user-scope services, when set.
    fn user_only_fields(&self) -> Vec<&'static str> {
        [
            (self.builtin.is_some(), "builtin"),
            (self.command.is_some(), "command"),
            (self.description.is_some(), "description"),
            (self.restart.is_some(), "restart"),
            (!self.environment.is_empty(), "environment"),
            (self.working_directory.is_some(), "working_directory"),
            (self.requires_tools, "requires_tools"),
            (self.state == ServiceState::Absent, "state = \"absent\""),
        ]
        .into_iter()
        .filter_map(|(is_set, field)| is_set.then_some(field))
        .collect()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ServiceNotifications {
    pub(super) sources: IndexMap<String, IndexSet<ResourceId>>,
}

impl ServiceNotifications {
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
pub(crate) fn compose_user_declarations(
    config: &Config,
) -> Result<IndexMap<String, (ServiceTomlConfig, ResourceOrigin)>> {
    Ok(compose_declarations(config)?
        .into_iter()
        .filter(|(_, (declaration, _))| declaration.scope() == ServiceScope::User)
        .collect())
}

/// Names of every user-scope service, for notification validation.
pub(crate) fn user_service_names(config: &Config) -> Result<Vec<String>> {
    Ok(compose_user_declarations(config)?.into_keys().collect())
}

/// `[bootstrap.services]` from every config map, rejecting redeclarations that
/// disagree.
pub(crate) fn compose_declarations(
    config: &Config,
) -> Result<IndexMap<String, (ServiceTomlConfig, ResourceOrigin)>> {
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

pub(crate) fn validate_notifications(
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

fn default_true() -> bool {
    true
}
