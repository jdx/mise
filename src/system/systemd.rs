//! systemd user services for `[bootstrap.linux.systemd.units]`.
//!
//! Entries are rendered to `~/.config/systemd/user/dev.mise.<name>.service`
//! and managed with `systemctl --user` when explicitly applied.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;

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
    #[serde(default = "default_start")]
    pub start: bool,
    #[serde(default)]
    pub wanted_by: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemdRequest {
    pub name: String,
    pub unit: String,
    pub description: Option<String>,
    pub after: Vec<String>,
    pub wants: Vec<String>,
    pub exec_start: String,
    pub environment: IndexMap<String, String>,
    pub working_directory: Option<String>,
    pub restart: Option<String>,
    pub restart_sec: Option<String>,
    pub standard_output: Option<String>,
    pub standard_error: Option<String>,
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

impl SystemdRequest {
    pub fn from_toml(name: String, config: SystemdTomlConfig) -> Result<Self> {
        if !valid_name(&name) {
            bail!("unit name '{name}' must contain only letters, numbers, '.', '_', '-', or '@'");
        }
        let Some(exec_start) = config.exec_start.map(|s| s.trim().to_string()) else {
            bail!("unit '{name}' must set `exec_start`");
        };
        if exec_start.is_empty() {
            bail!("unit '{name}' must set a non-empty `exec_start`");
        }
        Ok(Self {
            unit: format!("dev.mise.{name}.service"),
            name,
            description: config.description,
            after: config.after,
            wants: config.wants,
            exec_start,
            environment: config.environment,
            working_directory: config.working_directory,
            restart: config.restart,
            restart_sec: config.restart_sec,
            standard_output: config.standard_output,
            standard_error: config.standard_error,
            start: config.start,
            wanted_by: config
                .wanted_by
                .unwrap_or_else(|| vec!["default.target".to_string()]),
        })
    }
}

impl std::fmt::Display for SystemdRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.unit)
    }
}

pub fn is_available() -> bool {
    cfg!(target_os = "linux") && crate::file::which("systemctl").is_some()
}

pub fn unavailable_reason() -> String {
    if cfg!(target_os = "linux") {
        "`systemctl` not found".to_string()
    } else {
        "only available on linux".to_string()
    }
}

pub async fn status(requests: &[SystemdRequest]) -> Result<Vec<SystemdStatus>> {
    let mut out = vec![];
    for req in requests {
        let path = unit_path(req);
        let active = is_active(&req.unit).await?;
        let enabled = is_enabled(&req.unit).await?;
        let state = match std::fs::read_to_string(&path) {
            Ok(current) if normalize(&current) == normalize(&render_unit(req)) => {
                if active {
                    SystemdState::Active
                } else {
                    SystemdState::Inactive
                }
            }
            Ok(_) => SystemdState::Differs,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => SystemdState::Missing,
            Err(err) => return Err(err.into()),
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
    for req in requests {
        let path = unit_path(req);
        let unit = render_unit(req);
        if dry_run {
            miseprintln!(
                "{}",
                shell_words::join([
                    "mkdir".to_string(),
                    "-p".to_string(),
                    user_units_dir().display().to_string(),
                ])
            );
            miseprintln!("write {}", shell_words::join([path.display().to_string()]));
            miseprintln!(
                "{}",
                shell_words::join([
                    "systemctl".to_string(),
                    "--user".to_string(),
                    "daemon-reload".to_string(),
                ])
            );
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
            }
            continue;
        }
        std::fs::create_dir_all(user_units_dir())?;
        std::fs::write(&path, unit)?;
        systemctl(&["daemon-reload".to_string()]).await?;
        if !req.wanted_by.is_empty() {
            systemctl(&["enable".to_string(), req.unit.clone()]).await?;
        }
        if req.start {
            systemctl(&["restart".to_string(), req.unit.clone()]).await?;
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
    out.push_str("\n[Service]\n");
    out.push_str(&format!(
        "ExecStart={}\n",
        expand_path_string(&request.exec_start)
    ));
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
    if !request.wanted_by.is_empty() {
        out.push_str("\n[Install]\n");
        out.push_str(&format!("WantedBy={}\n", request.wanted_by.join(" ")));
    }
    out
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

async fn is_active(unit: &str) -> Result<bool> {
    systemctl_status(&[
        "is-active".to_string(),
        "--quiet".to_string(),
        unit.to_string(),
    ])
    .await
}

async fn is_enabled(unit: &str) -> Result<bool> {
    systemctl_status(&[
        "is-enabled".to_string(),
        "--quiet".to_string(),
        unit.to_string(),
    ])
    .await
}

async fn systemctl_status(args: &[String]) -> Result<bool> {
    debug!("$ systemctl --user {}", shell_words::join(args));
    let status = tokio::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    Ok(status.success())
}

async fn systemctl(args: &[String]) -> Result<()> {
    debug!("$ systemctl --user {}", shell_words::join(args));
    let output = tokio::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !output.status.success() {
        bail!(
            "`systemctl --user {}` failed: {}",
            shell_words::join(args),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
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
    }

    #[test]
    fn test_render_unit() {
        let mut environment = IndexMap::new();
        environment.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        environment.insert("QUOTED".to_string(), "hello \"there\"".to_string());
        let request = SystemdRequest {
            name: "sync".to_string(),
            unit: "dev.mise.sync.service".to_string(),
            description: Some("sync files".to_string()),
            after: vec!["network-online.target".to_string()],
            wants: vec!["network-online.target".to_string()],
            exec_start: "~/.local/bin/sync --watch".to_string(),
            environment,
            working_directory: Some("~".to_string()),
            restart: Some("on-failure".to_string()),
            restart_sec: Some("5s".to_string()),
            standard_output: Some("append:%h/.local/state/sync.log".to_string()),
            standard_error: Some("journal".to_string()),
            start: true,
            wanted_by: vec!["default.target".to_string()],
        };
        let unit = render_unit(&request);
        assert!(unit.contains("[Unit]\n"));
        assert!(unit.contains("Description=sync files\n"));
        assert!(unit.contains("After=network-online.target\n"));
        assert!(unit.contains("Wants=network-online.target\n"));
        assert!(unit.contains(&format!(
            "ExecStart={}/.local/bin/sync --watch\n",
            crate::dirs::HOME.display()
        )));
        assert!(unit.contains(&format!(
            "WorkingDirectory={}\n",
            crate::dirs::HOME.display()
        )));
        assert!(unit.contains("Environment=\"PATH=/usr/bin:/bin\"\n"));
        assert!(unit.contains("Environment=\"QUOTED=hello \\\"there\\\"\"\n"));
        assert!(unit.contains("Restart=on-failure\n"));
        assert!(unit.contains("RestartSec=5s\n"));
        assert!(unit.contains("StandardOutput=append:%h/.local/state/sync.log\n"));
        assert!(unit.contains("StandardError=journal\n"));
        assert!(unit.contains("[Install]\nWantedBy=default.target\n"));
    }
}
