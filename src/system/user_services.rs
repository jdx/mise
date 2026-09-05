//! User-scope services from `[bootstrap.services]` (`scope = "user"`).
//!
//! One declaration is rendered for the platform's user service manager: a
//! systemd user unit on Linux (`dev.mise.<name>.service`), a LaunchAgent on
//! macOS (`dev.mise.<name>`), a Scheduled Task on Windows (`mise\<name>`).
//! `builtin = "<name>"` selects a definition mise supplies, run through a
//! durable mise executable.

use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::Serialize;

use crate::config::Config;
use crate::system::launchd::{self, LaunchdRequest, LaunchdState, LaunchdTomlConfig};
use crate::system::resources::{ResourceAction, ResourceId, ResourceOrigin, ResourcePlan};
use crate::system::scheduled_tasks::{self, ScheduledTaskRequest, ScheduledTaskState};
use crate::system::services_common::{
    ServiceRestart, ServiceState, ServiceTomlConfig, compose_user_declarations,
};
use crate::system::systemd::{self, SystemdRequest, SystemdState, SystemdTomlConfig};

/// A service definition mise supplies.
struct Builtin {
    args: &'static [&'static str],
    description: &'static str,
    restart: ServiceRestart,
    nice: Option<i8>,
}

const BUILTIN_NAMES: &[&str] = &["history-watch"];

fn builtin(name: &str) -> Option<Builtin> {
    match name {
        "history-watch" => Some(Builtin {
            args: &["history", "watch"],
            description: "mise history: save tracked files as they change",
            restart: ServiceRestart::OnFailure,
            nice: Some(10),
        }),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UserServiceRequest {
    pub name: String,
    pub description: Option<String>,
    /// The resolved command line; `None` when a builtin has no durable
    /// executable to run through (see `unresolved`).
    pub command: Option<String>,
    pub unresolved: Option<String>,
    pub builtin: Option<String>,
    pub restart: ServiceRestart,
    pub nice: Option<i8>,
    pub environment: IndexMap<String, String>,
    pub working_directory: Option<String>,
    pub requires_tools: bool,
    pub state: ServiceState,
    pub enabled: bool,
    pub origin: Option<ResourceOrigin>,
}

impl UserServiceRequest {
    pub(crate) fn from_toml(
        name: String,
        config: ServiceTomlConfig,
        origin: Option<ResourceOrigin>,
    ) -> Result<Self> {
        Self::from_toml_with_executable(name, config, origin, durable_mise_executable())
    }

    fn from_toml_with_executable(
        name: String,
        config: ServiceTomlConfig,
        origin: Option<ResourceOrigin>,
        executable: Option<PathBuf>,
    ) -> Result<Self> {
        if !valid_name(&name) {
            bail!(
                "user service name '{name}' must contain only letters, numbers, '.', '_', or '-'"
            );
        }
        if config.masked {
            bail!("user service '{name}' cannot be masked; use `state = \"absent\"` to remove it");
        }
        let mut description = config.description;
        let mut restart = config.restart;
        let mut nice = None;
        let mut unresolved = None;
        let command = match (config.builtin.as_deref(), config.command.as_deref()) {
            (Some(_), Some(_)) => {
                bail!("user service '{name}' sets both `builtin` and `command`; choose one")
            }
            (None, None) => {
                bail!("user service '{name}' must set `command` or `builtin`")
            }
            (Some(builtin_name), None) => {
                let Some(definition) = builtin(builtin_name) else {
                    bail!(
                        "user service '{name}' names unknown builtin '{builtin_name}'; available: {}",
                        BUILTIN_NAMES.join(", ")
                    );
                };
                description.get_or_insert_with(|| definition.description.to_string());
                restart.get_or_insert(definition.restart);
                nice = definition.nice;
                match executable {
                    Some(exe) => Some(
                        std::iter::once(quote_program(&exe.to_string_lossy()))
                            .chain(definition.args.iter().map(|arg| arg.to_string()))
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    None => {
                        unresolved = Some(
                            "no durable mise executable; install mise on this host first"
                                .to_string(),
                        );
                        None
                    }
                }
            }
            (None, Some(command)) => {
                let command = command.trim();
                if command.is_empty() {
                    bail!("user service '{name}' must set a non-empty `command`");
                }
                Some(command.to_string())
            }
        };
        Ok(Self {
            name,
            description,
            command,
            unresolved,
            builtin: config.builtin,
            restart: restart.unwrap_or_default(),
            nice,
            environment: config.environment,
            working_directory: config.working_directory,
            requires_tools: config.requires_tools,
            state: config.state,
            enabled: config.enabled,
            origin,
        })
    }

    pub(crate) fn desired(&self) -> String {
        match self.state {
            ServiceState::Absent => "absent".to_string(),
            state if self.enabled => state.as_str().to_string(),
            state => format!("{} (not at login)", state.as_str()),
        }
    }

    fn start(&self) -> bool {
        self.state == ServiceState::Running
    }

    pub(crate) fn systemd_request(&self) -> Result<SystemdRequest> {
        let config = SystemdTomlConfig {
            description: self.description.clone(),
            exec_start: self.command.clone(),
            environment: self.environment.clone(),
            working_directory: self.working_directory.clone(),
            nice: self.nice,
            restart: Some(
                match self.restart {
                    ServiceRestart::Always => "always",
                    ServiceRestart::OnFailure => "on-failure",
                    ServiceRestart::Never => "no",
                }
                .to_string(),
            ),
            restart_sec: Some("5s".to_string()),
            start: self.start(),
            wanted_by: (!self.enabled).then(Vec::new),
            ..Default::default()
        };
        SystemdRequest::from_toml(self.name.clone(), config)
    }

    pub(crate) fn launchd_request(&self) -> Result<LaunchdRequest> {
        let command = self.command.clone().unwrap_or_default();
        let mut words = shell_words::split(&command)
            .map_err(|err| eyre::eyre!("user service '{}': invalid `command`: {err}", self.name))?;
        let program = if words.is_empty() {
            None
        } else {
            Some(words.remove(0))
        };
        let config = LaunchdTomlConfig {
            program,
            args: words,
            run_at_load: self.enabled && self.start(),
            keep_alive: self.start() && self.restart == ServiceRestart::Always,
            keep_alive_on_failure: self.start() && self.restart == ServiceRestart::OnFailure,
            environment: self.environment.clone(),
            working_directory: self.working_directory.clone(),
            kickstart: self.start(),
            ..Default::default()
        };
        LaunchdRequest::from_toml(self.name.clone(), config)
    }

    pub(crate) fn scheduled_task_request(&self) -> ScheduledTaskRequest {
        let mut request = ScheduledTaskRequest::new(&self.name);
        request.description = self.description.clone();
        request.command = self.command.clone().unwrap_or_default();
        request.restart_on_failure = self.restart != ServiceRestart::Never;
        request.environment = self.environment.clone();
        request.working_directory = self.working_directory.clone();
        request.start = self.start();
        request.at_logon = self.enabled;
        request
    }
}

impl std::fmt::Display for UserServiceRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (user)", self.name)
    }
}

/// Quote an executable path for the platform's command line: double quotes
/// on Windows (what Task Scheduler and `cmd.exe` understand), POSIX shell
/// quoting elsewhere.
fn quote_program(path: &str) -> String {
    if cfg!(windows) {
        if path.contains(char::is_whitespace) {
            format!("\"{path}\"")
        } else {
            path.to_string()
        }
    } else {
        shell_words::quote(path).to_string()
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Every user-scope service, validated, with names that collide with
/// `[bootstrap.linux.systemd.units]` or `[bootstrap.macos.launchd.agents]`
/// rejected (both would write the same unit or plist).
pub(crate) fn requests_from_config(config: &Config) -> Result<Vec<UserServiceRequest>> {
    let requests = compose_user_declarations(config)?
        .into_iter()
        .map(|(name, (declaration, origin))| {
            UserServiceRequest::from_toml(name, declaration, Some(origin))
        })
        .collect::<Result<Vec<_>>>()?;
    if requests.is_empty() {
        return Ok(requests);
    }
    let units = crate::system::systemd_from_config(config);
    let agents = crate::system::launchd_from_config(config);
    for request in &requests {
        if units.iter().any(|unit| unit.name == request.name) {
            bail!(
                "user service '{}' is also declared in [bootstrap.linux.systemd.units]; declare it once",
                request.name
            );
        }
        if agents.iter().any(|agent| agent.name == request.name) {
            bail!(
                "user service '{}' is also declared in [bootstrap.macos.launchd.agents]; declare it once",
                request.name
            );
        }
    }
    Ok(requests)
}

/// The mise executable a service definition may reference: the running
/// binary unless it lives in a temporary or remote-bootstrap staging
/// directory, else a `mise` on `PATH` outside those.
pub(crate) fn durable_mise_executable() -> Option<PathBuf> {
    let current = std::fs::canonicalize(&*crate::env::MISE_BIN).ok();
    if let Some(current) = current.filter(|path| is_durable(path)) {
        return Some(current);
    }
    crate::file::which("mise")
        .and_then(|path| std::fs::canonicalize(path).ok())
        .filter(|path| is_durable(path))
}

fn is_durable(path: &Path) -> bool {
    let temp = std::env::temp_dir();
    let temp = std::fs::canonicalize(&temp).unwrap_or(temp);
    if path.starts_with(&temp) {
        return false;
    }
    let text = path.to_string_lossy();
    !(text.contains("/mise-bootstrap.") || text.contains("\\mise-bootstrap."))
}

pub(crate) fn is_available() -> bool {
    if cfg!(target_os = "linux") {
        systemd::is_available()
    } else if cfg!(target_os = "macos") {
        launchd::is_available()
    } else if cfg!(windows) {
        scheduled_tasks::is_available()
    } else {
        false
    }
}

pub(crate) fn unavailable_reason() -> String {
    if cfg!(target_os = "linux") {
        systemd::unavailable_reason()
    } else if cfg!(target_os = "macos") {
        launchd::unavailable_reason()
    } else if cfg!(windows) {
        scheduled_tasks::unavailable_reason()
    } else {
        "user services are only supported on linux, macos, and windows".to_string()
    }
}

/// The name of the platform's user service manager, for messages.
pub(crate) fn manager_name() -> &'static str {
    if cfg!(target_os = "linux") {
        "systemd user unit"
    } else if cfg!(target_os = "macos") {
        "LaunchAgent"
    } else {
        "Scheduled Task"
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UserServiceStatus {
    pub name: String,
    pub scope: &'static str,
    pub current: String,
    pub desired: String,
    pub action: ResourceAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub requires_tools: bool,
    pub restart: &'static str,
    /// The definition mise renders for this platform, for inspection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    #[serde(skip)]
    pub request: UserServiceRequest,
}

impl UserServiceStatus {
    pub(crate) fn plan(&self) -> ResourcePlan {
        let plan = ResourcePlan::new(
            ResourceId::new("user-service", &self.name),
            self.current.clone(),
            self.desired.clone(),
            self.action,
        );
        match &self.request.origin {
            Some(origin) => plan.with_origin(origin.clone()),
            None => plan,
        }
    }

    fn new(request: &UserServiceRequest, current: String, action: ResourceAction) -> Self {
        Self {
            name: request.name.clone(),
            scope: "user",
            current,
            desired: request.desired(),
            action,
            path: None,
            builtin: request.builtin.clone(),
            command: request.command.clone(),
            requires_tools: request.requires_tools,
            restart: request.restart.as_str(),
            definition: None,
            request: request.clone(),
        }
    }
}

pub(crate) async fn status(requests: &[UserServiceRequest]) -> Result<Vec<UserServiceStatus>> {
    let mut out = vec![];
    for request in requests {
        out.push(status_one(request).await?);
    }
    Ok(out)
}

/// The definition mise would install for this platform, for inspection.
fn render_definition(request: &UserServiceRequest) -> Result<String> {
    if cfg!(target_os = "linux") {
        Ok(systemd::render_unit(&request.systemd_request()?))
    } else if cfg!(target_os = "macos") {
        let plist = launchd::render_plist(&request.launchd_request()?)?;
        Ok(String::from_utf8_lossy(&plist).to_string())
    } else {
        scheduled_tasks::render_xml(&request.scheduled_task_request(), "<user>")
    }
}

fn definition_path(name: &str) -> PathBuf {
    if cfg!(target_os = "linux") {
        systemd::service_unit_path(name)
    } else if cfg!(target_os = "macos") {
        launchd::agent_plist_path(name)
    } else {
        scheduled_tasks::definition_path(name)
    }
}

async fn status_one(request: &UserServiceRequest) -> Result<UserServiceStatus> {
    let path = definition_path(&request.name);
    // removal needs no command: an absent builtin is removed even when no
    // durable executable resolves
    if request.state == ServiceState::Absent {
        if !is_available() {
            let mut out = UserServiceStatus::new(
                request,
                format!("unavailable: {}", unavailable_reason()),
                ResourceAction::Unknown,
            );
            out.path = Some(path);
            return Ok(out);
        }
        let installed = if cfg!(windows) {
            scheduled_tasks::exists(&request.name).await?
        } else {
            path.exists()
        };
        let (current, action) = absent_state(installed);
        let mut out = UserServiceStatus::new(request, current.to_string(), action);
        out.path = Some(path);
        return Ok(out);
    }
    if let Some(reason) = &request.unresolved {
        return Ok(UserServiceStatus::new(
            request,
            format!("unknown: {reason}"),
            ResourceAction::Unknown,
        ));
    }
    let definition = render_definition(request)?;
    if !is_available() {
        let mut out = UserServiceStatus::new(
            request,
            format!("unavailable: {}", unavailable_reason()),
            ResourceAction::Unknown,
        );
        out.path = Some(path);
        out.definition = Some(definition);
        return Ok(out);
    }
    let (current, action) = if cfg!(target_os = "linux") {
        let unit = request.systemd_request()?;
        {
            let status = systemd::status(std::slice::from_ref(&unit))
                .await?
                .pop()
                .expect("one status per request");
            let current = match status.state {
                SystemdState::Missing => "not installed",
                SystemdState::Differs => "installed, differs",
                SystemdState::Active => "running",
                SystemdState::Inactive => "stopped",
            };
            (
                current,
                converge_action(status.is_desired(), status.state == SystemdState::Missing),
            )
        }
    } else if cfg!(target_os = "macos") {
        let agent = request.launchd_request()?;
        {
            let status = launchd::status(std::slice::from_ref(&agent))
                .await?
                .pop()
                .expect("one status per request");
            let running = status.loaded && launchd::is_running(&agent.label).await?;
            let (current, desired) = match status.state {
                LaunchdState::Missing => ("not installed", false),
                LaunchdState::Differs => ("installed, differs", false),
                LaunchdState::Unloaded => ("installed, not loaded", false),
                LaunchdState::Loaded if running => ("running", request.start()),
                LaunchdState::Loaded => ("stopped", !request.start()),
            };
            (
                current,
                converge_action(desired, status.state == LaunchdState::Missing),
            )
        }
    } else {
        let task = request.scheduled_task_request();
        {
            let status = scheduled_tasks::status(std::slice::from_ref(&task))
                .await?
                .pop()
                .expect("one status per request");
            let current = match status.state {
                ScheduledTaskState::Missing => "not installed",
                ScheduledTaskState::Differs => "installed, differs",
                ScheduledTaskState::Disabled => "installed, disabled",
                ScheduledTaskState::Running => "running",
                ScheduledTaskState::Ready => "stopped",
            };
            (
                current,
                converge_action(
                    status.is_desired(),
                    status.state == ScheduledTaskState::Missing,
                ),
            )
        }
    };
    let mut out = UserServiceStatus::new(request, current.to_string(), action);
    out.path = Some(path);
    out.definition = Some(definition);
    Ok(out)
}

fn absent_state(installed: bool) -> (&'static str, ResourceAction) {
    if installed {
        ("installed", ResourceAction::Remove)
    } else {
        ("absent", ResourceAction::Noop)
    }
}

fn converge_action(desired: bool, missing: bool) -> ResourceAction {
    if desired {
        ResourceAction::Noop
    } else if missing {
        ResourceAction::Create
    } else {
        ResourceAction::Update
    }
}

/// Converge the given user services. Returns a reason when the platform's
/// user service manager is unavailable and nothing was applied.
pub(crate) async fn apply(
    requests: &[UserServiceRequest],
    dry_run: bool,
    yes: bool,
) -> Result<Option<String>> {
    if requests.is_empty() {
        return Ok(None);
    }
    if !is_available() {
        let reason = unavailable_reason();
        debug!("user services: skipping, {reason}");
        return Ok(Some(reason));
    }
    let statuses = status(requests).await?;
    let mut targets = vec![];
    for status in &statuses {
        match status.action {
            ResourceAction::Noop => {}
            ResourceAction::Unknown => {
                warn!(
                    "user service {}: {}; not written",
                    status.name, status.current
                );
            }
            _ => targets.push(status.request.clone()),
        }
    }
    let applied = statuses.len() - targets.len();
    if applied > 0 {
        info!("user services: {applied} service(s) already applied");
    }
    if targets.is_empty() {
        return Ok(None);
    }
    let list = targets.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
    if !dry_run && !yes && console::user_attended_stderr() {
        let msg = format!("user services: apply {}?", list.join(", "));
        if !crate::ui::prompt::confirm(msg)?.is_yes() {
            info!("user services: skipped");
            return Ok(None);
        }
    }
    for request in &targets {
        apply_one(request, dry_run).await?;
    }
    if !dry_run {
        info!("user services: applied {}", list.join(", "));
    }
    Ok(None)
}

async fn apply_one(request: &UserServiceRequest, dry_run: bool) -> Result<()> {
    if request.state == ServiceState::Absent {
        remove_named(&request.name, dry_run).await?;
        return Ok(());
    }
    if cfg!(target_os = "linux") {
        systemd::apply(&[request.systemd_request()?], dry_run).await
    } else if cfg!(target_os = "macos") {
        launchd::apply(&[request.launchd_request()?], dry_run).await
    } else {
        scheduled_tasks::apply(&[request.scheduled_task_request()], dry_run).await
    }
}

/// Remove the installed definition for `name`, declared or not. Returns
/// whether one existed.
pub(crate) async fn remove_named(name: &str, dry_run: bool) -> Result<bool> {
    if !valid_name(name) {
        bail!("user service name '{name}' must contain only letters, numbers, '.', '_', or '-'");
    }
    if !is_available() {
        bail!(
            "cannot remove user service '{name}': {}",
            unavailable_reason()
        );
    }
    if cfg!(target_os = "linux") {
        systemd::remove_service(name, dry_run).await
    } else if cfg!(target_os = "macos") {
        launchd::remove_agent(name, dry_run).await
    } else {
        scheduled_tasks::remove_task(name, dry_run).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::services_common::ServiceScope;

    fn user_config(command: &str) -> ServiceTomlConfig {
        ServiceTomlConfig {
            scope: Some(ServiceScope::User),
            command: Some(command.to_string()),
            ..Default::default()
        }
    }

    fn request(config: ServiceTomlConfig) -> UserServiceRequest {
        UserServiceRequest::from_toml_with_executable(
            "agent".to_string(),
            config,
            None,
            Some(PathBuf::from("/usr/bin/mise")),
        )
        .unwrap()
    }

    #[test]
    fn requires_a_command_or_builtin() {
        let err = request_result(ServiceTomlConfig {
            scope: Some(ServiceScope::User),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("must set `command` or `builtin`"));
        let err = request_result(ServiceTomlConfig {
            builtin: Some("history-watch".into()),
            command: Some("x".into()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("both `builtin` and `command`"));
        let err = request_result(ServiceTomlConfig {
            builtin: Some("nope".into()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("unknown builtin 'nope'; available: history-watch")
        );
        let err = request_result(ServiceTomlConfig {
            masked: true,
            ..user_config("agent")
        })
        .unwrap_err();
        assert!(err.to_string().contains("cannot be masked"));
    }

    fn request_result(config: ServiceTomlConfig) -> Result<UserServiceRequest> {
        UserServiceRequest::from_toml_with_executable(
            "agent".to_string(),
            config,
            None,
            Some(PathBuf::from("/usr/bin/mise")),
        )
    }

    #[test]
    fn builtin_expands_through_the_durable_executable() {
        let request = request(ServiceTomlConfig {
            builtin: Some("history-watch".into()),
            ..Default::default()
        });
        assert_eq!(
            request.command.as_deref(),
            Some("/usr/bin/mise history watch")
        );
        assert_eq!(request.restart, ServiceRestart::OnFailure);
        assert_eq!(request.nice, Some(10));
        assert!(request.description.is_some());
        assert!(request.unresolved.is_none());

        let staged = UserServiceRequest::from_toml_with_executable(
            "agent".to_string(),
            ServiceTomlConfig {
                builtin: Some("history-watch".into()),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
        assert!(staged.command.is_none());
        assert!(
            staged
                .unresolved
                .as_deref()
                .unwrap()
                .contains("no durable mise executable")
        );
    }

    #[test]
    fn staged_binaries_are_not_durable() {
        let temp = std::env::temp_dir();
        let temp = std::fs::canonicalize(&temp).unwrap_or(temp);
        assert!(!is_durable(
            &temp.join("mise-bootstrap.abc123").join("mise")
        ));
        assert!(!is_durable(Path::new("/tmp/mise-bootstrap.abc123/mise")));
        assert!(is_durable(Path::new("/usr/bin/mise")));
        assert!(is_durable(Path::new("/home/me/.local/bin/mise")));
    }

    #[test]
    fn renders_a_systemd_unit() {
        let mut config = user_config("~/.local/bin/agent --serve");
        config.restart = Some(ServiceRestart::Always);
        config.environment.insert("RUST_LOG".into(), "info".into());
        config.working_directory = Some("~".into());
        let unit = systemd::render_unit(&request(config).systemd_request().unwrap());
        assert!(unit.contains("ExecStart="));
        assert!(unit.contains("agent --serve\n"));
        assert!(unit.contains("Restart=always\n"));
        assert!(unit.contains("RestartSec=5s\n"));
        assert!(unit.contains("Environment=\"RUST_LOG=info\"\n"));
        assert!(unit.contains("WorkingDirectory="));
        assert!(unit.contains("WantedBy=default.target\n"));

        let mut config = user_config("agent");
        config.enabled = false;
        config.restart = Some(ServiceRestart::Never);
        let unit = systemd::render_unit(&request(config).systemd_request().unwrap());
        assert!(unit.contains("Restart=no\n"));
        assert!(!unit.contains("[Install]"));
    }

    #[test]
    fn renders_a_launch_agent() {
        let mut config = user_config("\"/Applications/My Agent.app/agent\" --serve");
        config.environment.insert("RUST_LOG".into(), "info".into());
        let agent = request(config).launchd_request().unwrap();
        assert_eq!(agent.program, "/Applications/My Agent.app/agent");
        assert_eq!(agent.args, vec!["--serve".to_string()]);
        assert!(agent.run_at_load);
        assert!(agent.kickstart);
        assert!(!agent.keep_alive);
        assert!(agent.keep_alive_on_failure);
        let plist = String::from_utf8(launchd::render_plist(&agent).unwrap()).unwrap();
        assert!(plist.contains("<key>SuccessfulExit</key>"));
        assert!(plist.contains("<key>RUST_LOG</key>"));

        let mut config = user_config("agent");
        config.state = ServiceState::Stopped;
        config.restart = Some(ServiceRestart::Always);
        let agent = request(config).launchd_request().unwrap();
        assert!(!agent.run_at_load);
        assert!(!agent.kickstart);
        assert!(!agent.keep_alive);
        assert!(!agent.keep_alive_on_failure);
    }

    #[test]
    fn renders_a_scheduled_task() {
        let mut config = user_config("C:\\Tools\\agent.exe --serve");
        config.enabled = false;
        config.restart = Some(ServiceRestart::Never);
        let task = request(config).scheduled_task_request();
        assert_eq!(task.task, "mise\\agent");
        assert!(!task.at_logon);
        assert!(!task.restart_on_failure);
        assert!(task.start);
        let xml = scheduled_tasks::render_xml(&task, "me").unwrap();
        assert!(xml.contains("<Command>C:\\Tools\\agent.exe</Command>"));
    }

    #[test]
    fn desired_state_is_readable() {
        assert_eq!(request(user_config("agent")).desired(), "running");
        let mut config = user_config("agent");
        config.enabled = false;
        assert_eq!(request(config).desired(), "running (not at login)");
        let mut config = user_config("agent");
        config.state = ServiceState::Absent;
        assert_eq!(request(config).desired(), "absent");
    }
}
