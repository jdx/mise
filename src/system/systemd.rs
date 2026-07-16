//! systemd user services and timers for `[bootstrap.linux.systemd.units]`.
//!
//! Entries are rendered to `~/.config/systemd/user/dev.mise.<name>.<service|timer>`
//! and managed with `systemctl --user` when explicitly applied.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use eyre::{Result, bail, eyre};
use indexmap::IndexMap;
use serde::Deserialize;

const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SystemdTomlConfig {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub wants: Vec<String>,
    #[serde(default)]
    pub exec_start: Option<String>,
    #[serde(default, rename = "type")]
    pub service_type: Option<String>,
    #[serde(default)]
    pub remain_after_exit: Option<bool>,
    #[serde(default)]
    pub exec_stop: Option<String>,
    #[serde(default)]
    pub timeout_start_sec: Option<String>,
    #[serde(default)]
    pub timeout_stop_sec: Option<String>,
    #[serde(default)]
    pub no_new_privileges: Option<bool>,
    #[serde(default)]
    pub private_tmp: Option<bool>,
    #[serde(default)]
    pub environment: IndexMap<String, String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub restart: Option<String>,
    #[serde(default)]
    pub restart_sec: Option<String>,
    #[serde(default)]
    pub standard_output: Option<String>,
    #[serde(default)]
    pub standard_error: Option<String>,
    #[serde(default)]
    pub on_boot_sec: Option<String>,
    #[serde(default)]
    pub on_unit_active_sec: Option<String>,
    #[serde(default)]
    pub on_unit_inactive_sec: Option<String>,
    #[serde(default)]
    pub on_calendar: Option<String>,
    #[serde(default)]
    pub randomized_delay_sec: Option<String>,
    #[serde(default)]
    pub accuracy_sec: Option<String>,
    #[serde(default)]
    pub persistent: Option<bool>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default = "default_start")]
    pub start: bool,
    #[serde(default)]
    pub wanted_by: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemdUnitKind {
    Service,
    Timer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemdRequest {
    pub name: String,
    pub unit: String,
    pub kind: SystemdUnitKind,
    pub description: Option<String>,
    pub after: Vec<String>,
    pub wants: Vec<String>,
    pub exec_start: Option<String>,
    pub service_type: Option<String>,
    pub remain_after_exit: Option<bool>,
    pub exec_stop: Option<String>,
    pub timeout_start_sec: Option<String>,
    pub timeout_stop_sec: Option<String>,
    pub no_new_privileges: Option<bool>,
    pub private_tmp: Option<bool>,
    pub environment: IndexMap<String, String>,
    pub working_directory: Option<String>,
    pub restart: Option<String>,
    pub restart_sec: Option<String>,
    pub standard_output: Option<String>,
    pub standard_error: Option<String>,
    pub on_boot_sec: Option<String>,
    pub on_unit_active_sec: Option<String>,
    pub on_unit_inactive_sec: Option<String>,
    pub on_calendar: Option<String>,
    pub randomized_delay_sec: Option<String>,
    pub accuracy_sec: Option<String>,
    pub persistent: Option<bool>,
    pub timer_unit: Option<String>,
    pub start: bool,
    pub wanted_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemdState {
    Active,
    Inactive,
    Differs,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemdStatus {
    pub request: SystemdRequest,
    pub path: PathBuf,
    pub active: bool,
    pub enabled: bool,
    pub state: SystemdState,
}

impl SystemdStatus {
    pub fn is_desired(&self) -> bool {
        match self.state {
            SystemdState::Active => self.request.start,
            SystemdState::Inactive => !self.request.start,
            SystemdState::Differs | SystemdState::Missing => false,
        }
    }
}

impl SystemdRequest {
    pub fn from_toml(name: String, config: SystemdTomlConfig) -> Result<Self> {
        if !valid_name(&name) {
            bail!("unit name '{name}' must contain only letters, numbers, '.', '_', '-', or '@'");
        }
        let is_timer = config.on_boot_sec.is_some()
            || config.on_unit_active_sec.is_some()
            || config.on_unit_inactive_sec.is_some()
            || config.on_calendar.is_some()
            || config.randomized_delay_sec.is_some()
            || config.accuracy_sec.is_some()
            || config.persistent.is_some()
            || config.unit.is_some();
        let kind = if is_timer {
            SystemdUnitKind::Timer
        } else {
            SystemdUnitKind::Service
        };
        let exec_start = config.exec_start.map(|s| s.trim().to_string());
        if kind == SystemdUnitKind::Service && exec_start.as_deref().is_none_or(str::is_empty) {
            bail!("service unit '{name}' must set a non-empty `exec_start`");
        }
        if kind == SystemdUnitKind::Timer {
            let service_only_fields = [
                (exec_start.is_some(), "exec_start"),
                (config.service_type.is_some(), "type"),
                (config.remain_after_exit.is_some(), "remain_after_exit"),
                (config.exec_stop.is_some(), "exec_stop"),
                (config.timeout_start_sec.is_some(), "timeout_start_sec"),
                (config.timeout_stop_sec.is_some(), "timeout_stop_sec"),
                (config.no_new_privileges.is_some(), "no_new_privileges"),
                (config.private_tmp.is_some(), "private_tmp"),
                (!config.environment.is_empty(), "environment"),
                (config.working_directory.is_some(), "working_directory"),
                (config.restart.is_some(), "restart"),
                (config.restart_sec.is_some(), "restart_sec"),
                (config.standard_output.is_some(), "standard_output"),
                (config.standard_error.is_some(), "standard_error"),
            ]
            .into_iter()
            .filter_map(|(is_set, field)| is_set.then_some(field))
            .collect::<Vec<_>>();
            if !service_only_fields.is_empty() {
                bail!(
                    "timer unit '{name}' cannot set service-only directive(s): {}",
                    service_only_fields.join(", ")
                );
            }
            if config.on_boot_sec.is_none()
                && config.on_unit_active_sec.is_none()
                && config.on_unit_inactive_sec.is_none()
                && config.on_calendar.is_none()
            {
                bail!(
                    "timer unit '{name}' must set at least one of `on_boot_sec`, \
                     `on_unit_active_sec`, `on_unit_inactive_sec`, or `on_calendar`"
                );
            }
        }
        let wanted_by = config.wanted_by.unwrap_or_else(|| match kind {
            SystemdUnitKind::Service => vec!["default.target".to_string()],
            SystemdUnitKind::Timer => vec!["timers.target".to_string()],
        });
        Ok(Self {
            unit: format!(
                "dev.mise.{name}.{}",
                match kind {
                    SystemdUnitKind::Service => "service",
                    SystemdUnitKind::Timer => "timer",
                }
            ),
            name,
            kind,
            description: config.description,
            after: config.after,
            wants: config.wants,
            exec_start,
            service_type: config.service_type,
            remain_after_exit: config.remain_after_exit,
            exec_stop: config.exec_stop,
            timeout_start_sec: config.timeout_start_sec,
            timeout_stop_sec: config.timeout_stop_sec,
            no_new_privileges: config.no_new_privileges,
            private_tmp: config.private_tmp,
            environment: config.environment,
            working_directory: config.working_directory,
            restart: config.restart,
            restart_sec: config.restart_sec,
            standard_output: config.standard_output,
            standard_error: config.standard_error,
            on_boot_sec: config.on_boot_sec,
            on_unit_active_sec: config.on_unit_active_sec,
            on_unit_inactive_sec: config.on_unit_inactive_sec,
            on_calendar: config.on_calendar,
            randomized_delay_sec: config.randomized_delay_sec,
            accuracy_sec: config.accuracy_sec,
            persistent: config.persistent,
            timer_unit: config.unit,
            start: config.start,
            wanted_by,
        })
    }
}

impl std::fmt::Display for SystemdRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.unit)
    }
}

pub fn is_available() -> bool {
    cfg!(target_os = "linux")
        && crate::file::which("systemctl").is_some()
        && sudo_invoking_user().is_none()
        && user_manager_available()
}

pub fn unavailable_reason() -> String {
    if !cfg!(target_os = "linux") {
        "only available on linux".to_string()
    } else if crate::file::which("systemctl").is_none() {
        "`systemctl` not found".to_string()
    } else if sudo_invoking_user().is_some() {
        "`systemctl --user` cannot target SUDO_USER; run mise as the target user".to_string()
    } else if !user_manager_available() {
        "systemd user manager not available".to_string()
    } else {
        "systemd unavailable".to_string()
    }
}

pub async fn status(requests: &[SystemdRequest]) -> Result<Vec<SystemdStatus>> {
    let mut out = vec![];
    for req in requests {
        let path = unit_path(req);
        let current = match std::fs::read_to_string(&path) {
            Ok(current) => current,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let sibling = sibling_unit(req);
                if sibling_unit_path(req).exists() {
                    out.push(SystemdStatus {
                        request: req.clone(),
                        path,
                        active: is_active(&sibling).await?,
                        enabled: is_enabled(&sibling).await?,
                        state: SystemdState::Differs,
                    });
                    continue;
                }
                out.push(SystemdStatus {
                    request: req.clone(),
                    path,
                    active: false,
                    enabled: false,
                    state: SystemdState::Missing,
                });
                continue;
            }
            Err(err) => return Err(err.into()),
        };
        let active = is_active(&req.unit).await?;
        let enabled = is_enabled(&req.unit).await?;
        let desired_enabled = !req.wanted_by.is_empty();
        let state = if sibling_unit_path(req).exists()
            || normalize(&current) != normalize(&render_unit(req))
            || enabled != desired_enabled
        {
            SystemdState::Differs
        } else if active {
            SystemdState::Active
        } else {
            SystemdState::Inactive
        };
        out.push(SystemdStatus {
            request: req.clone(),
            path,
            active,
            enabled,
            state,
        });
    }
    Ok(out)
}

pub async fn apply(requests: &[SystemdRequest], dry_run: bool) -> Result<()> {
    if dry_run {
        for req in requests {
            miseprintln!(
                "{}",
                shell_words::join([
                    "mkdir".to_string(),
                    "-p".to_string(),
                    user_units_dir().display().to_string(),
                ])
            );
            let path = unit_path(req);
            miseprintln!("write {}", shell_words::join([path.display().to_string()]));
        }
        for req in requests {
            let sibling = sibling_unit(req);
            let sibling_path = sibling_unit_path(req);
            if sibling_path.exists() {
                miseprintln!(
                    "{}",
                    shell_words::join([
                        "systemctl".to_string(),
                        "--user".to_string(),
                        "stop".to_string(),
                        sibling.clone(),
                    ])
                );
                miseprintln!(
                    "{}",
                    shell_words::join([
                        "systemctl".to_string(),
                        "--user".to_string(),
                        "disable".to_string(),
                        sibling,
                    ])
                );
                miseprintln!(
                    "{}",
                    shell_words::join(["rm".to_string(), sibling_path.display().to_string()])
                );
            }
            miseprintln!(
                "{}",
                shell_words::join([
                    "systemctl".to_string(),
                    "--user".to_string(),
                    "disable".to_string(),
                    req.unit.clone(),
                ])
            );
        }
        miseprintln!(
            "{}",
            shell_words::join([
                "systemctl".to_string(),
                "--user".to_string(),
                "daemon-reload".to_string(),
            ])
        );
        for req in requests {
            if !req.wanted_by.is_empty() {
                miseprintln!(
                    "{}",
                    shell_words::join([
                        "systemctl".to_string(),
                        "--user".to_string(),
                        "enable".to_string(),
                        req.unit.clone(),
                    ])
                );
            }
            if req.start {
                miseprintln!(
                    "{}",
                    shell_words::join([
                        "systemctl".to_string(),
                        "--user".to_string(),
                        "restart".to_string(),
                        req.unit.clone(),
                    ])
                );
            } else {
                miseprintln!(
                    "{}",
                    shell_words::join([
                        "systemctl".to_string(),
                        "--user".to_string(),
                        "stop".to_string(),
                        req.unit.clone(),
                    ])
                );
            }
        }
        return Ok(());
    }

    std::fs::create_dir_all(user_units_dir())?;
    for req in requests {
        let path = unit_path(req);
        let unit = render_unit(req);
        std::fs::write(&path, unit)?;
    }
    for req in requests {
        let sibling = sibling_unit(req);
        let sibling_path = sibling_unit_path(req);
        if sibling_path.exists() {
            stop_unit(&sibling).await?;
            disable_unit(&sibling).await?;
            std::fs::remove_file(sibling_path)?;
        }
        disable_unit(&req.unit).await?;
    }
    systemctl(&["daemon-reload".to_string()]).await?;
    for req in requests {
        if !req.wanted_by.is_empty() {
            systemctl(&["enable".to_string(), req.unit.clone()]).await?;
        }
        if req.start {
            systemctl(&["restart".to_string(), req.unit.clone()]).await?;
        } else {
            systemctl(&["stop".to_string(), req.unit.clone()]).await?;
        }
    }
    Ok(())
}

pub fn render_unit(request: &SystemdRequest) -> String {
    let mut out = String::new();
    out.push_str("[Unit]\n");
    if let Some(description) = &request.description {
        out.push_str(&format!("Description={description}\n"));
    }
    if !request.after.is_empty() {
        out.push_str(&format!("After={}\n", request.after.join(" ")));
    }
    if !request.wants.is_empty() {
        out.push_str(&format!("Wants={}\n", request.wants.join(" ")));
    }
    match request.kind {
        SystemdUnitKind::Service => render_service(request, &mut out),
        SystemdUnitKind::Timer => render_timer(request, &mut out),
    }
    if !request.wanted_by.is_empty() {
        out.push_str("\n[Install]\n");
        out.push_str(&format!("WantedBy={}\n", request.wanted_by.join(" ")));
    }
    out
}

fn render_service(request: &SystemdRequest, out: &mut String) {
    out.push_str("\n[Service]\n");
    if let Some(service_type) = &request.service_type {
        out.push_str(&format!("Type={service_type}\n"));
    }
    if let Some(exec_start) = &request.exec_start {
        out.push_str(&format!("ExecStart={}\n", expand_path_string(exec_start)));
    }
    if let Some(remain_after_exit) = request.remain_after_exit {
        out.push_str(&format!("RemainAfterExit={}\n", yes_no(remain_after_exit)));
    }
    if let Some(exec_stop) = &request.exec_stop {
        out.push_str(&format!("ExecStop={}\n", expand_path_string(exec_stop)));
    }
    if let Some(timeout_start_sec) = &request.timeout_start_sec {
        out.push_str(&format!("TimeoutStartSec={timeout_start_sec}\n"));
    }
    if let Some(timeout_stop_sec) = &request.timeout_stop_sec {
        out.push_str(&format!("TimeoutStopSec={timeout_stop_sec}\n"));
    }
    if let Some(no_new_privileges) = request.no_new_privileges {
        out.push_str(&format!("NoNewPrivileges={}\n", yes_no(no_new_privileges)));
    }
    if let Some(private_tmp) = request.private_tmp {
        out.push_str(&format!("PrivateTmp={}\n", yes_no(private_tmp)));
    }
    if let Some(path) = &request.working_directory {
        out.push_str(&format!("WorkingDirectory={}\n", expand_path_string(path)));
    }
    for (key, value) in &request.environment {
        out.push_str(&format!(
            "Environment={}\n",
            quote_environment(&format!("{key}={value}"))
        ));
    }
    if let Some(restart) = &request.restart {
        out.push_str(&format!("Restart={restart}\n"));
    }
    if let Some(restart_sec) = &request.restart_sec {
        out.push_str(&format!("RestartSec={restart_sec}\n"));
    }
    if let Some(standard_output) = &request.standard_output {
        out.push_str(&format!("StandardOutput={standard_output}\n"));
    }
    if let Some(standard_error) = &request.standard_error {
        out.push_str(&format!("StandardError={standard_error}\n"));
    }
}

fn render_timer(request: &SystemdRequest, out: &mut String) {
    out.push_str("\n[Timer]\n");
    for (key, value) in [
        ("OnBootSec", &request.on_boot_sec),
        ("OnUnitActiveSec", &request.on_unit_active_sec),
        ("OnUnitInactiveSec", &request.on_unit_inactive_sec),
        ("OnCalendar", &request.on_calendar),
        ("RandomizedDelaySec", &request.randomized_delay_sec),
        ("AccuracySec", &request.accuracy_sec),
    ] {
        if let Some(value) = value {
            out.push_str(&format!("{key}={value}\n"));
        }
    }
    if let Some(persistent) = request.persistent {
        out.push_str(&format!("Persistent={}\n", yes_no(persistent)));
    }
    if let Some(unit) = &request.timer_unit {
        out.push_str(&format!("Unit={unit}\n"));
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn default_start() -> bool {
    true
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' || c == '@')
}

fn user_units_dir() -> PathBuf {
    crate::dirs::HOME
        .join(".config")
        .join("systemd")
        .join("user")
}

fn unit_path(request: &SystemdRequest) -> PathBuf {
    user_units_dir().join(&request.unit)
}

fn sibling_unit(request: &SystemdRequest) -> String {
    format!(
        "dev.mise.{}.{}",
        request.name,
        match request.kind {
            SystemdUnitKind::Service => "timer",
            SystemdUnitKind::Timer => "service",
        }
    )
}

fn sibling_unit_path(request: &SystemdRequest) -> PathBuf {
    user_units_dir().join(sibling_unit(request))
}

fn expand_path_string(path: &str) -> String {
    if path == "~" {
        return crate::dirs::HOME.to_string_lossy().to_string();
    }
    crate::file::replace_path(Path::new(path))
        .to_string_lossy()
        .to_string()
}

fn quote_environment(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn normalize(value: &str) -> String {
    value.replace("\r\n", "\n").trim_end().to_string()
}

fn user_manager_available() -> bool {
    if crate::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok_and(|v| !v.is_empty()) {
        return true;
    }
    crate::env::var("XDG_RUNTIME_DIR")
        .map(|dir| user_manager_socket_available(Path::new(&dir)))
        .unwrap_or(false)
}

fn user_manager_socket_available(runtime_dir: &Path) -> bool {
    runtime_dir.join("systemd/private").exists() || runtime_dir.join("bus").exists()
}

fn sudo_invoking_user() -> Option<String> {
    if crate::system::sudo::is_root()
        && let Ok(sudo_user) = crate::env::var("SUDO_USER")
        && !sudo_user.is_empty()
        && sudo_user != "root"
    {
        Some(sudo_user)
    } else {
        None
    }
}

async fn is_active(unit: &str) -> Result<bool> {
    systemctl_status(&[
        "is-active".to_string(),
        "--quiet".to_string(),
        unit.to_string(),
    ])
    .await
}

async fn is_enabled(unit: &str) -> Result<bool> {
    let args = ["is-enabled".to_string(), unit.to_string()];
    debug!("$ systemctl --user {}", shell_words::join(&args));
    let mut cmd = tokio::process::Command::new("systemctl");
    cmd.arg("--user")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(SYSTEMCTL_TIMEOUT, cmd.output())
        .await
        .map_err(|_| eyre!("`systemctl --user {}` timed out", shell_words::join(&args)))??;
    let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    debug!(
        "`systemctl --user {}` state: {}",
        shell_words::join(&args),
        state
    );
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            debug!(
                "`systemctl --user {}` exited non-zero: {}",
                shell_words::join(&args),
                stderr
            );
        }
    }
    Ok(unit_file_state_is_enabled(&state))
}

fn unit_file_state_is_enabled(state: &str) -> bool {
    matches!(state, "enabled" | "enabled-runtime")
}

async fn systemctl_status(args: &[String]) -> Result<bool> {
    debug!("$ systemctl --user {}", shell_words::join(args));
    let mut cmd = tokio::process::Command::new("systemctl");
    cmd.arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(SYSTEMCTL_TIMEOUT, cmd.output())
        .await
        .map_err(|_| eyre!("`systemctl --user {}` timed out", shell_words::join(args)))??;
    if output.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        debug!(
            "`systemctl --user {}` exited non-zero: {}",
            shell_words::join(args),
            stderr
        );
    }
    Ok(false)
}

async fn systemctl(args: &[String]) -> Result<()> {
    debug!("$ systemctl --user {}", shell_words::join(args));
    let mut cmd = tokio::process::Command::new("systemctl");
    cmd.arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(SYSTEMCTL_TIMEOUT, cmd.output())
        .await
        .map_err(|_| eyre!("`systemctl --user {}` timed out", shell_words::join(args)))??;
    if !output.status.success() {
        bail!(
            "`systemctl --user {}` failed: {}",
            shell_words::join(args),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

async fn disable_unit(unit: &str) -> Result<()> {
    match systemctl(&["disable".to_string(), unit.to_string()]).await {
        Ok(()) => Ok(()),
        Err(err) if disable_unit_error_is_noop(&err.to_string()) => {
            debug!("systemd: ignoring disable for {unit}: {err}");
            Ok(())
        }
        Err(err) => Err(err),
    }
}

async fn stop_unit(unit: &str) -> Result<()> {
    match systemctl(&["stop".to_string(), unit.to_string()]).await {
        Ok(()) => Ok(()),
        Err(err) if unit_operation_error_is_noop(&err.to_string()) => {
            debug!("systemd: ignoring stop for {unit}: {err}");
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn disable_unit_error_is_noop(error: &str) -> bool {
    unit_operation_error_is_noop(error)
        || error.contains("no [Install] section")
        || error.contains("has no installation config")
        || error.contains("is a static unit")
}

fn unit_operation_error_is_noop(error: &str) -> bool {
    error.contains("does not exist") || error.contains("not loaded")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_systemd_request_validation() {
        let request = SystemdRequest::from_toml(
            "my-service".to_string(),
            SystemdTomlConfig {
                exec_start: Some("~/.local/bin/my-service".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(request.unit, "dev.mise.my-service.service");
        assert_eq!(request.wanted_by, vec!["default.target"]);
        assert!(SystemdRequest::from_toml("bad/name".to_string(), Default::default()).is_err());

        let err = SystemdRequest::from_toml(
            "modifier-only-timer".to_string(),
            SystemdTomlConfig {
                persistent: Some(true),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("must set at least one"));

        let err = SystemdRequest::from_toml(
            "invalid-timer".to_string(),
            SystemdTomlConfig {
                on_boot_sec: Some("1min".to_string()),
                restart: Some("on-failure".to_string()),
                environment: IndexMap::from([("KEY".to_string(), "value".to_string())]),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("restart"));
        assert!(err.to_string().contains("environment"));
    }

    #[test]
    fn test_render_unit() {
        let mut environment = IndexMap::new();
        environment.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        environment.insert("QUOTED".to_string(), "hello \"there\"".to_string());
        let request = SystemdRequest::from_toml(
            "sync".to_string(),
            SystemdTomlConfig {
                description: Some("sync files".to_string()),
                after: vec!["network-online.target".to_string()],
                wants: vec!["network-online.target".to_string()],
                exec_start: Some("~/.local/bin/sync --watch".to_string()),
                service_type: Some("oneshot".to_string()),
                remain_after_exit: Some(true),
                exec_stop: Some("~/.local/bin/sync --stop".to_string()),
                timeout_start_sec: Some("120".to_string()),
                timeout_stop_sec: Some("30".to_string()),
                no_new_privileges: Some(true),
                private_tmp: Some(true),
                environment,
                working_directory: Some("~".to_string()),
                restart: Some("on-failure".to_string()),
                restart_sec: Some("5s".to_string()),
                standard_output: Some("append:%h/.local/state/sync.log".to_string()),
                standard_error: Some("journal".to_string()),
                start: true,
                wanted_by: Some(vec!["default.target".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();
        let unit = render_unit(&request);
        assert!(unit.contains("[Unit]\n"));
        assert!(unit.contains("Description=sync files\n"));
        assert!(unit.contains("After=network-online.target\n"));
        assert!(unit.contains("Wants=network-online.target\n"));
        assert!(unit.contains(&format!(
            "ExecStart={}\n",
            expand_path_string("~/.local/bin/sync --watch")
        )));
        assert!(unit.contains("Type=oneshot\n"));
        assert!(unit.contains("RemainAfterExit=yes\n"));
        assert!(unit.contains(&format!(
            "ExecStop={}\n",
            expand_path_string("~/.local/bin/sync --stop")
        )));
        assert!(unit.contains("TimeoutStartSec=120\n"));
        assert!(unit.contains("TimeoutStopSec=30\n"));
        assert!(unit.contains("NoNewPrivileges=yes\n"));
        assert!(unit.contains("PrivateTmp=yes\n"));
        assert!(unit.contains(&format!("WorkingDirectory={}\n", expand_path_string("~"))));
        assert!(unit.contains("Environment=\"PATH=/usr/bin:/bin\"\n"));
        assert!(unit.contains("Environment=\"QUOTED=hello \\\"there\\\"\"\n"));
        assert!(unit.contains("Restart=on-failure\n"));
        assert!(unit.contains("RestartSec=5s\n"));
        assert!(unit.contains("StandardOutput=append:%h/.local/state/sync.log\n"));
        assert!(unit.contains("StandardError=journal\n"));
        assert!(unit.contains("[Install]\nWantedBy=default.target\n"));
    }

    #[test]
    fn test_render_timer_unit() {
        let request = SystemdRequest::from_toml(
            "healthcheck".to_string(),
            SystemdTomlConfig {
                on_boot_sec: Some("2min".to_string()),
                on_unit_inactive_sec: Some("5min".to_string()),
                randomized_delay_sec: Some("30s".to_string()),
                accuracy_sec: Some("1s".to_string()),
                persistent: Some(true),
                unit: Some("dev.mise.healthcheck.service".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(request.unit, "dev.mise.healthcheck.timer");
        assert_eq!(sibling_unit(&request), "dev.mise.healthcheck.service");
        assert_eq!(request.wanted_by, vec!["timers.target"]);
        assert_eq!(
            render_unit(&request),
            "[Unit]\n\n[Timer]\nOnBootSec=2min\nOnUnitInactiveSec=5min\nRandomizedDelaySec=30s\nAccuracySec=1s\nPersistent=yes\nUnit=dev.mise.healthcheck.service\n\n[Install]\nWantedBy=timers.target\n"
        );
    }

    #[test]
    fn test_systemd_status_desired_state() {
        let request = SystemdRequest::from_toml(
            "sync".to_string(),
            SystemdTomlConfig {
                exec_start: Some("true".to_string()),
                start: true,
                ..Default::default()
            },
        )
        .unwrap();
        let active = SystemdStatus {
            request: request.clone(),
            path: PathBuf::new(),
            active: true,
            enabled: true,
            state: SystemdState::Active,
        };
        assert!(active.is_desired());

        let mut stopped_request = request.clone();
        stopped_request.start = false;
        let active_stopped = SystemdStatus {
            request: stopped_request.clone(),
            path: PathBuf::new(),
            active: true,
            enabled: true,
            state: SystemdState::Active,
        };
        assert!(!active_stopped.is_desired());

        let inactive_stopped = SystemdStatus {
            request: stopped_request,
            path: PathBuf::new(),
            active: false,
            enabled: true,
            state: SystemdState::Inactive,
        };
        assert!(inactive_stopped.is_desired());
    }

    #[test]
    fn test_unit_file_state_is_enabled() {
        for state in ["enabled", "enabled-runtime"] {
            assert!(unit_file_state_is_enabled(state), "{state}");
        }
        for state in [
            "static",
            "disabled",
            "linked",
            "linked-runtime",
            "alias",
            "masked",
            "masked-runtime",
            "indirect",
            "generated",
            "transient",
            "not-found",
            "bad",
        ] {
            assert!(!unit_file_state_is_enabled(state), "{state}");
        }
    }

    #[test]
    fn test_user_manager_socket_available() {
        let runtime_dir = tempfile::tempdir().unwrap();
        assert!(!user_manager_socket_available(runtime_dir.path()));

        crate::file::create_dir_all(runtime_dir.path().join("systemd/private")).unwrap();
        assert!(user_manager_socket_available(runtime_dir.path()));

        crate::file::remove_file_or_dir(runtime_dir.path().join("systemd/private")).unwrap();
        std::fs::write(runtime_dir.path().join("bus"), "").unwrap();
        assert!(user_manager_socket_available(runtime_dir.path()));
    }
}
