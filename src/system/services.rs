use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use eyre::{Result, bail, eyre};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::system::resources::{ResourceAction, ResourceId, ResourcePlan};

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

impl Default for ServiceTomlConfig {
    fn default() -> Self {
        Self {
            state: ServiceState::Running,
            enabled: true,
            masked: false,
            on_change: ServiceChangeAction::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServiceRequest {
    pub name: String,
    pub unit: String,
    pub state: ServiceState,
    pub enabled: bool,
    pub masked: bool,
    pub on_change: ServiceChangeAction,
    inspection: Option<ServiceInspection>,
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

    fn change_for(&self, request: &ServiceRequest) -> ServiceChange {
        let Some(sources) = self.sources.get(&request.name) else {
            return ServiceChange::default();
        };
        ServiceChange {
            notified: true,
            provides_unit: sources
                .iter()
                .any(|source| request.is_managed_unit_file(source)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ServiceChange {
    notified: bool,
    provides_unit: bool,
}

#[derive(Clone, Debug)]
enum ServiceInspection {
    Missing,
    Unavailable(String),
    Present {
        active_state: String,
        unit_file_state: String,
        need_daemon_reload: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ServiceAction {
    unit: String,
    state: ServiceState,
    enabled: bool,
    masked: bool,
    on_change: ServiceChangeAction,
    dependency_changed: bool,
    notified: bool,
    active: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ServicePlan {
    actions: Vec<ServiceAction>,
}

pub fn prepare_requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    let mut merged = IndexMap::new();
    for cf in config.config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            for (name, service) in bootstrap.services {
                merged.entry(name).or_insert(service);
            }
        }
    }
    merged
        .into_iter()
        .map(|(name, config)| ServiceRequest::from_toml(name, config))
        .collect()
}

pub fn requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    let mut requests = prepare_requests_from_config(config)?;
    inspect_requests(&mut requests);
    Ok(requests)
}

pub fn status_requests_from_config(config: &Config) -> Result<Vec<ServiceRequest>> {
    requests_from_config(config)
}

pub fn inspect_requests(requests: &mut [ServiceRequest]) {
    if requests.is_empty() {
        return;
    }
    let Some(systemctl) = systemctl_path() else {
        for request in requests {
            request.inspection = Some(ServiceInspection::Unavailable(
                "required command 'systemctl' was not found".to_string(),
            ));
        }
        return;
    };
    for request in requests {
        request.inspection = Some(inspect_service(&systemctl, &request.unit));
    }
}

impl ServiceRequest {
    fn from_toml(name: String, config: ServiceTomlConfig) -> Result<Self> {
        let unit = normalize_unit_name(&name)?;
        if config.masked && config.state == ServiceState::Running {
            bail!("bootstrap service '{name}' cannot be both masked and running");
        }
        if config.masked && config.enabled {
            bail!("bootstrap service '{name}' cannot be both masked and enabled");
        }
        Ok(Self {
            name,
            unit,
            state: config.state,
            enabled: config.enabled,
            masked: config.masked,
            on_change: config.on_change,
            inspection: None,
        })
    }

    pub fn plan(&self) -> ResourcePlan {
        let id = ResourceId::new("service", &self.name);
        let desired = self.desired();
        let Some(inspection) = &self.inspection else {
            return ResourcePlan::new(id, "not inspected", desired, ResourceAction::Unknown);
        };
        match inspection {
            ServiceInspection::Missing => {
                ResourcePlan::new(id, "unit not found", desired, ResourceAction::Unknown)
            }
            ServiceInspection::Unavailable(reason) => ResourcePlan::new(
                id,
                format!("unavailable: {reason}"),
                desired,
                ResourceAction::Unknown,
            ),
            ServiceInspection::Present {
                active_state,
                unit_file_state,
                need_daemon_reload,
            } => {
                let state_matches = active_state_matches(self.state, active_state);
                let enabled = unit_file_state_is_enabled(unit_file_state);
                let masked = unit_file_state_is_masked(unit_file_state);
                let enabled_matches = enabled == self.enabled;
                let masked_matches = masked == self.masked;
                let cannot_enable = self.enabled
                    && !enabled
                    && !masked
                    && !unit_file_state_is_enableable(unit_file_state);
                ResourcePlan::new(
                    id,
                    describe_current(active_state, unit_file_state, *need_daemon_reload),
                    desired,
                    if cannot_enable {
                        ResourceAction::Unknown
                    } else if state_matches
                        && enabled_matches
                        && masked_matches
                        && !need_daemon_reload
                    {
                        ResourceAction::Noop
                    } else {
                        ResourceAction::Update
                    },
                )
            }
        }
    }

    fn plan_with_change(&self, change: ServiceChange) -> ResourcePlan {
        let mut plan = self.plan();
        if change.provides_unit && matches!(self.inspection, Some(ServiceInspection::Missing)) {
            plan.current = "unit not found; pending managed unit-file change".to_string();
            plan.action = ResourceAction::Update;
            return plan;
        }
        if change.notified
            && self.state == ServiceState::Running
            && self.on_change != ServiceChangeAction::None
            && plan.action == ResourceAction::Noop
        {
            plan.action = ResourceAction::Update;
        }
        plan
    }

    fn desired(&self) -> String {
        format!(
            "{}; {}; {}; on change {}",
            match self.state {
                ServiceState::Running => "running",
                ServiceState::Stopped => "stopped",
            },
            if self.enabled { "enabled" } else { "disabled" },
            if self.masked { "masked" } else { "unmasked" },
            match self.on_change {
                ServiceChangeAction::Reload => "reload",
                ServiceChangeAction::Restart => "restart",
                ServiceChangeAction::ReloadOrRestart => "reload-or-restart",
                ServiceChangeAction::None => "none",
            },
        )
    }

    fn action(&self, change: ServiceChange) -> Result<Option<ServiceAction>> {
        let changed_dependency = change.notified;
        let missing = matches!(self.inspection, Some(ServiceInspection::Missing));
        let notified = self.state == ServiceState::Running
            && changed_dependency
            && self.on_change != ServiceChangeAction::None;
        let dependency_changed =
            changed_dependency && (notified || missing || self.needs_daemon_reload());
        match self.plan().action {
            ResourceAction::Noop if !notified || self.on_change == ServiceChangeAction::None => {
                Ok(None)
            }
            ResourceAction::Noop | ResourceAction::Update => Ok(Some(ServiceAction {
                unit: self.unit.clone(),
                state: self.state,
                enabled: self.enabled,
                masked: self.masked,
                on_change: self.on_change,
                dependency_changed,
                notified,
                active: self.is_active(),
            })),
            ResourceAction::Unknown if change.provides_unit && missing => Ok(Some(ServiceAction {
                unit: self.unit.clone(),
                state: self.state,
                enabled: self.enabled,
                masked: self.masked,
                on_change: self.on_change,
                dependency_changed,
                notified,
                active: false,
            })),
            ResourceAction::Unknown => bail!(
                "refusing unsafe change to bootstrap service '{}'; inspect `mise bootstrap plan`",
                self.name
            ),
            ResourceAction::Create | ResourceAction::Remove => {
                unreachable!("service lifecycle requests do not create or remove units")
            }
        }
    }

    fn from_action(action: &ServiceAction) -> Self {
        Self {
            name: action.unit.clone(),
            unit: action.unit.clone(),
            state: action.state,
            enabled: action.enabled,
            masked: action.masked,
            on_change: action.on_change,
            inspection: None,
        }
    }

    fn is_active(&self) -> bool {
        matches!(
            &self.inspection,
            Some(ServiceInspection::Present { active_state, .. })
                if matches!(active_state.as_str(), "active" | "reloading")
        )
    }

    fn needs_daemon_reload(&self) -> bool {
        matches!(
            self.inspection,
            Some(ServiceInspection::Present {
                need_daemon_reload: true,
                ..
            })
        )
    }

    fn is_managed_unit_file(&self, source: &ResourceId) -> bool {
        if source.kind != "file" {
            return false;
        }
        let path = Path::new(&source.name);
        let Some(parent) = path.parent() else {
            return false;
        };
        if !SYSTEM_UNIT_PATHS
            .iter()
            .any(|candidate| parent == *candidate)
        {
            return false;
        }
        path.file_name().is_some_and(|name| {
            name == self.unit.as_str()
                || instantiated_unit_template(&self.unit)
                    .is_some_and(|template| name == template.as_str())
        })
    }

    fn commands_after_reload(&self, notified: bool) -> Result<Vec<Vec<String>>> {
        if self.plan().action == ResourceAction::Unknown {
            bail!(
                "refusing unsafe change to bootstrap service '{}'; current state remains unknown after systemctl daemon-reload",
                self.name
            );
        }
        Ok(self
            .action(ServiceChange {
                notified,
                provides_unit: false,
            })?
            .map(|action| action.commands())
            .unwrap_or_default())
    }
}

const SYSTEM_UNIT_PATHS: &[&str] = &[
    "/etc/systemd/system.control",
    "/run/systemd/system.control",
    "/run/systemd/transient",
    "/run/systemd/generator.early",
    "/etc/systemd/system",
    "/etc/systemd/system.attached",
    "/run/systemd/system",
    "/run/systemd/system.attached",
    "/run/systemd/generator",
    "/usr/local/lib/systemd/system",
    "/usr/local/share/systemd/system",
    "/usr/lib/systemd/system",
    "/usr/share/systemd/system",
    "/lib/systemd/system",
    "/run/systemd/generator.late",
];

fn instantiated_unit_template(unit: &str) -> Option<String> {
    let (prefix, instance) = unit.split_once('@')?;
    let suffix = instance.find('.').map(|index| &instance[index..])?;
    (!instance.starts_with('.')).then(|| format!("{prefix}@{suffix}"))
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

pub fn plans_with_notifications(
    requests: &[ServiceRequest],
    notifications: &ServiceNotifications,
) -> Vec<ResourcePlan> {
    requests
        .iter()
        .map(|request| request.plan_with_change(notifications.change_for(request)))
        .collect()
}

pub fn apply(requests: &[ServiceRequest], dry_run: bool, yes: bool) -> Result<()> {
    apply_with_notifications(requests, &ServiceNotifications::default(), dry_run, yes)
}

pub fn apply_with_notifications(
    requests: &[ServiceRequest],
    notifications: &ServiceNotifications,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let mut actions = vec![];
    let mut unknown = vec![];
    for request in requests {
        let change = notifications.change_for(request);
        let plan = request.plan_with_change(change);
        if dry_run && plan.action == ResourceAction::Unknown {
            unknown.push(plan);
            continue;
        }
        if let Some(action) = request.action(change)? {
            actions.push(action);
        }
    }
    if dry_run {
        let has_unknown = !unknown.is_empty();
        if !actions.is_empty() {
            miseprintln!("would run systemctl daemon-reload");
        }
        for action in &actions {
            for command in action.commands() {
                miseprintln!("would run systemctl {}", shell_words::join(command));
            }
        }
        for resource in unknown {
            warn!(
                "would not change {}: current {}, desired {} (manual action required)",
                resource.id, resource.current, resource.desired
            );
        }
        if actions.is_empty() && !has_unknown {
            info!("services: already converged");
        }
        return Ok(());
    }
    if actions.is_empty() {
        info!("services: already converged");
        return Ok(());
    }
    let independent_actions = actions
        .iter()
        .filter(|action| !action.dependency_changed)
        .count();
    if !yes
        && independent_actions > 0
        && console::user_attended_stderr()
        && !crate::ui::prompt::confirm(format!(
            "services: apply {independent_actions} change(s) not triggered by managed files?"
        ))?
    {
        actions.retain(|action| action.dependency_changed);
        if actions.is_empty() {
            info!("services: skipped");
            return Ok(());
        }
        info!("services: skipped {independent_actions} independent change(s)");
    }
    let input = serde_json::to_vec(&ServicePlan { actions })?;
    let executable = std::env::current_exe()?.to_string_lossy().to_string();
    crate::system::sudo::run_with_input(
        &executable,
        &[
            "--no-config".to_string(),
            "--no-env".to_string(),
            "--no-hooks".to_string(),
            "bootstrap".to_string(),
            "__apply-service-plan".to_string(),
        ],
        &input,
    )?;
    info!("services: applied changes");
    Ok(())
}

pub fn apply_privileged_plan_from_stdin() -> Result<()> {
    let plan: ServicePlan = serde_json::from_reader(std::io::stdin().lock())?;
    if plan.actions.is_empty() {
        return Ok(());
    }
    let systemctl =
        systemctl_path().ok_or_else(|| eyre!("required command 'systemctl' was not found"))?;
    run_systemctl(&systemctl, &["daemon-reload".to_string()])?;
    let mut commands = vec![];
    for action in &plan.actions {
        let mut request = ServiceRequest::from_action(action);
        request.inspection = Some(inspect_service(&systemctl, &request.unit));
        commands.push(request.commands_after_reload(action.notified)?);
    }
    for service_commands in commands {
        for command in service_commands {
            run_systemctl(&systemctl, &command)?;
        }
    }
    Ok(())
}

impl ServiceAction {
    fn commands(&self) -> Vec<Vec<String>> {
        let unit = self.unit.clone();
        if self.masked {
            return vec![
                vec!["stop".to_string(), unit.clone()],
                vec!["disable".to_string(), unit.clone()],
                vec!["mask".to_string(), unit],
            ];
        }
        let mut commands = vec![
            vec!["unmask".to_string(), unit.clone()],
            vec![
                if self.enabled { "enable" } else { "disable" }.to_string(),
                unit.clone(),
            ],
        ];
        if let Some(command) = self.state_command() {
            commands.push(vec![command.to_string(), unit]);
        }
        commands
    }

    fn state_command(&self) -> Option<&'static str> {
        match self.state {
            ServiceState::Stopped => Some("stop"),
            ServiceState::Running if self.notified && self.active => match self.on_change {
                ServiceChangeAction::Reload => Some("reload"),
                ServiceChangeAction::Restart => Some("restart"),
                ServiceChangeAction::ReloadOrRestart => Some("reload-or-restart"),
                ServiceChangeAction::None => None,
            },
            ServiceState::Running => Some("start"),
        }
    }
}

fn inspect_service(systemctl: &std::path::Path, unit: &str) -> ServiceInspection {
    let args = [
        "show".to_string(),
        "--no-pager".to_string(),
        "--property=LoadState".to_string(),
        "--property=ActiveState".to_string(),
        "--property=UnitFileState".to_string(),
        "--property=NeedDaemonReload".to_string(),
        "--".to_string(),
        unit.to_string(),
    ];
    let output = match systemctl_output(systemctl, &args) {
        Ok(output) => output,
        Err(error) => return ServiceInspection::Unavailable(error.to_string()),
    };
    if !output.status.success() {
        return ServiceInspection::Unavailable(output_error(&output));
    }
    parse_inspection(&String::from_utf8_lossy(&output.stdout))
}

fn parse_inspection(output: &str) -> ServiceInspection {
    let properties = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<IndexMap<_, _>>();
    let load_state = properties.get("LoadState").copied().unwrap_or("not-found");
    if load_state == "not-found" {
        return ServiceInspection::Missing;
    }
    ServiceInspection::Present {
        active_state: properties
            .get("ActiveState")
            .copied()
            .unwrap_or("unknown")
            .to_string(),
        unit_file_state: properties
            .get("UnitFileState")
            .copied()
            .unwrap_or("unknown")
            .to_string(),
        need_daemon_reload: properties.get("NeedDaemonReload") == Some(&"yes"),
    }
}

fn run_systemctl(systemctl: &std::path::Path, args: &[String]) -> Result<()> {
    info!("$ {} {}", systemctl.display(), shell_words::join(args));
    let output = systemctl_output(systemctl, args)?;
    if !output.status.success() {
        bail!(
            "systemctl {} failed: {}",
            shell_words::join(args),
            output_error(&output)
        );
    }
    Ok(())
}

fn systemctl_output(systemctl: &std::path::Path, args: &[String]) -> Result<Output> {
    Ok(Command::new(systemctl)
        .args(args)
        .env("LC_ALL", "C")
        .env("SYSTEMD_PAGER", "")
        .output()?)
}

fn output_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("systemctl exited with {}", output.status)
    } else {
        stderr
    }
}

fn systemctl_path() -> Option<PathBuf> {
    [
        "/usr/bin/systemctl",
        "/bin/systemctl",
        "/run/current-system/sw/bin/systemctl",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

fn normalize_unit_name(name: &str) -> Result<String> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('-')
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_.@:-".contains(character))
    {
        bail!(
            "invalid bootstrap service name '{name}': use at most 255 ASCII letters, digits, '.', '_', '@', ':', or '-'"
        );
    }
    if name.contains('.') {
        Ok(name.to_string())
    } else {
        Ok(format!("{name}.service"))
    }
}

fn describe_current(active_state: &str, unit_file_state: &str, need_daemon_reload: bool) -> String {
    format!(
        "{active_state}; {unit_file_state}{}",
        if need_daemon_reload {
            "; daemon reload needed"
        } else {
            ""
        }
    )
}

fn active_state_matches(desired: ServiceState, current: &str) -> bool {
    match desired {
        ServiceState::Running => matches!(current, "active" | "reloading"),
        ServiceState::Stopped => current == "inactive",
    }
}

fn unit_file_state_is_enabled(state: &str) -> bool {
    matches!(state, "enabled" | "enabled-runtime")
}

fn unit_file_state_is_masked(state: &str) -> bool {
    matches!(state, "masked" | "masked-runtime")
}

fn unit_file_state_is_enableable(state: &str) -> bool {
    matches!(
        state,
        "disabled" | "enabled" | "enabled-runtime" | "linked" | "linked-runtime" | "indirect"
    )
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notifications(path: &str) -> ServiceNotifications {
        let mut notifications = ServiceNotifications::default();
        notifications.notify_file(Path::new(path), &["example".to_string()]);
        notifications
    }

    #[test]
    fn validates_and_normalizes_service_names() {
        assert_eq!(normalize_unit_name("docker").unwrap(), "docker.service");
        assert_eq!(
            normalize_unit_name("postgresql@16-main.service").unwrap(),
            "postgresql@16-main.service"
        );
        assert!(normalize_unit_name("../docker").is_err());
        assert!(normalize_unit_name("--help").is_err());
    }

    #[test]
    fn rejects_contradictory_masking() {
        assert!(
            ServiceRequest::from_toml(
                "example".to_string(),
                ServiceTomlConfig {
                    masked: true,
                    ..Default::default()
                },
            )
            .is_err()
        );
        assert!(
            ServiceRequest::from_toml(
                "example".to_string(),
                ServiceTomlConfig {
                    state: ServiceState::Stopped,
                    enabled: false,
                    masked: true,
                    on_change: ServiceChangeAction::default(),
                },
            )
            .is_ok()
        );
    }

    #[test]
    fn parses_systemctl_properties() {
        assert!(matches!(
            parse_inspection("LoadState=not-found\nActiveState=inactive\nUnitFileState=\n"),
            ServiceInspection::Missing
        ));
        let ServiceInspection::Present {
            active_state,
            unit_file_state,
            ..
        } = parse_inspection("LoadState=loaded\nActiveState=active\nUnitFileState=enabled\n")
        else {
            panic!("expected present service")
        };
        assert_eq!(active_state, "active");
        assert_eq!(unit_file_state, "enabled");
    }

    #[test]
    fn plans_service_convergence_and_static_failures() {
        let mut request =
            ServiceRequest::from_toml("docker".to_string(), Default::default()).unwrap();
        request.inspection = Some(ServiceInspection::Present {
            active_state: "active".to_string(),
            unit_file_state: "enabled".to_string(),
            need_daemon_reload: false,
        });
        assert_eq!(request.plan().action, ResourceAction::Noop);
        request.inspection = Some(ServiceInspection::Present {
            active_state: "active".to_string(),
            unit_file_state: "enabled".to_string(),
            need_daemon_reload: true,
        });
        assert_eq!(request.plan().action, ResourceAction::Update);
        request.inspection = Some(ServiceInspection::Present {
            active_state: "inactive".to_string(),
            unit_file_state: "disabled".to_string(),
            need_daemon_reload: false,
        });
        assert_eq!(request.plan().action, ResourceAction::Update);
        request.inspection = Some(ServiceInspection::Present {
            active_state: "active".to_string(),
            unit_file_state: "static".to_string(),
            need_daemon_reload: false,
        });
        assert_eq!(request.plan().action, ResourceAction::Unknown);
    }

    #[test]
    fn orders_mask_and_unmask_actions_safely() {
        let masked = ServiceAction {
            unit: "example.service".to_string(),
            state: ServiceState::Stopped,
            enabled: false,
            masked: true,
            on_change: ServiceChangeAction::default(),
            dependency_changed: false,
            notified: false,
            active: false,
        };
        assert_eq!(
            masked.commands(),
            vec![
                vec!["stop", "example.service"],
                vec!["disable", "example.service"],
                vec!["mask", "example.service"],
            ]
        );
        let running = ServiceAction {
            unit: "example.service".to_string(),
            state: ServiceState::Running,
            enabled: true,
            masked: false,
            on_change: ServiceChangeAction::default(),
            dependency_changed: false,
            notified: false,
            active: false,
        };
        assert_eq!(
            running.commands(),
            vec![
                vec!["unmask", "example.service"],
                vec!["enable", "example.service"],
                vec!["start", "example.service"],
            ]
        );

        let notified = ServiceAction {
            dependency_changed: true,
            notified: true,
            active: true,
            ..running
        };
        assert_eq!(
            notified.commands(),
            vec![
                vec!["unmask", "example.service"],
                vec!["enable", "example.service"],
                vec!["reload-or-restart", "example.service"],
            ]
        );
    }

    #[test]
    fn notifications_do_not_act_on_stopped_services() {
        let mut request = ServiceRequest::from_toml(
            "example".to_string(),
            ServiceTomlConfig {
                state: ServiceState::Stopped,
                enabled: false,
                on_change: ServiceChangeAction::Restart,
                ..Default::default()
            },
        )
        .unwrap();
        request.inspection = Some(ServiceInspection::Present {
            active_state: "inactive".to_string(),
            unit_file_state: "disabled".to_string(),
            need_daemon_reload: false,
        });
        let change = notifications("/etc/example.conf").change_for(&request);
        assert!(request.action(change).unwrap().is_none());
        assert_eq!(
            request.plan_with_change(change).action,
            ResourceAction::Noop
        );
    }

    #[test]
    fn status_plans_include_pending_notifications() {
        let mut request =
            ServiceRequest::from_toml("example".to_string(), Default::default()).unwrap();
        request.inspection = Some(ServiceInspection::Present {
            active_state: "active".to_string(),
            unit_file_state: "enabled".to_string(),
            need_daemon_reload: false,
        });

        assert_eq!(request.plan().action, ResourceAction::Noop);
        assert_eq!(
            plans_with_notifications(&[request], &notifications("/etc/example.conf"))[0].action,
            ResourceAction::Update
        );
    }

    #[test]
    fn missing_services_are_only_retried_after_their_unit_file_changes() {
        let mut request =
            ServiceRequest::from_toml("example".to_string(), Default::default()).unwrap();
        request.inspection = Some(ServiceInspection::Missing);
        let unrelated = notifications("/etc/example.conf").change_for(&request);
        let unit_file = notifications("/etc/systemd/system/example.service").change_for(&request);
        assert_eq!(request.plan().action, ResourceAction::Unknown);
        assert_eq!(
            request.plan_with_change(unrelated).action,
            ResourceAction::Unknown
        );
        assert!(request.action(unrelated).is_err());
        assert_eq!(
            request.plan_with_change(unit_file).action,
            ResourceAction::Update
        );
        let action = request.action(unit_file).unwrap().unwrap();
        assert!(action.dependency_changed);
        assert!(action.notified);
    }

    #[test]
    fn instantiated_services_accept_managed_template_units() {
        let request =
            ServiceRequest::from_toml("worker@blue".to_string(), Default::default()).unwrap();
        let mut notifications = ServiceNotifications::default();
        notifications.notify_file(
            Path::new("/etc/systemd/system/worker@.service"),
            &["worker@blue".to_string()],
        );
        let change = notifications.change_for(&request);
        assert!(change.provides_unit);
    }

    #[test]
    fn daemon_reload_does_not_imply_a_service_notification() {
        let mut request =
            ServiceRequest::from_toml("example".to_string(), Default::default()).unwrap();
        request.inspection = Some(ServiceInspection::Present {
            active_state: "active".to_string(),
            unit_file_state: "enabled".to_string(),
            need_daemon_reload: true,
        });
        let action = request.action(ServiceChange::default()).unwrap().unwrap();
        assert!(!action.dependency_changed);
        assert!(!action.notified);

        request.inspection = Some(ServiceInspection::Present {
            active_state: "active".to_string(),
            unit_file_state: "enabled".to_string(),
            need_daemon_reload: false,
        });
        assert!(
            request
                .commands_after_reload(action.notified)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn stopped_service_drift_remains_independently_confirmable() {
        let mut request = ServiceRequest::from_toml(
            "example".to_string(),
            ServiceTomlConfig {
                state: ServiceState::Stopped,
                enabled: false,
                ..Default::default()
            },
        )
        .unwrap();
        request.inspection = Some(ServiceInspection::Present {
            active_state: "active".to_string(),
            unit_file_state: "enabled".to_string(),
            need_daemon_reload: false,
        });
        let action = request
            .action(notifications("/etc/example.conf").change_for(&request))
            .unwrap()
            .unwrap();
        assert!(!action.dependency_changed);
        assert!(!action.notified);
    }
}
