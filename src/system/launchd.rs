//! macOS user LaunchAgents for `[bootstrap.macos.launchd.agents]`.
//!
//! Entries are rendered to `~/Library/LaunchAgents/dev.mise.<name>.plist` and
//! loaded with `launchctl bootstrap gui/$UID ...` when explicitly applied.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use eyre::{Result, bail};
use indexmap::IndexMap;
use plist::{Dictionary, Value};
use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub struct LaunchdTomlConfig {
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub run_at_load: bool,
    #[serde(default)]
    pub keep_alive: bool,
    #[serde(default)]
    pub start_interval: Option<u64>,
    #[serde(default)]
    pub start_calendar_interval: Option<LaunchdCalendarIntervals>,
    #[serde(default)]
    pub environment: IndexMap<String, String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub stdout_path: Option<String>,
    #[serde(default)]
    pub stderr_path: Option<String>,
    #[serde(default)]
    pub kickstart: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
pub struct LaunchdCalendarInterval {
    #[serde(default)]
    pub minute: Option<u8>,
    #[serde(default)]
    pub hour: Option<u8>,
    #[serde(default)]
    pub day: Option<u8>,
    #[serde(default)]
    pub weekday: Option<u8>,
    #[serde(default)]
    pub month: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum LaunchdCalendarIntervals {
    Single(LaunchdCalendarInterval),
    Multiple(Vec<LaunchdCalendarInterval>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchdRequest {
    pub name: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    pub run_at_load: bool,
    pub keep_alive: bool,
    pub start_interval: Option<u64>,
    pub start_calendar_interval: Option<LaunchdCalendarIntervals>,
    pub environment: IndexMap<String, String>,
    pub working_directory: Option<String>,
    pub stdout_path: Option<String>,
    pub stderr_path: Option<String>,
    pub kickstart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchdState {
    Loaded,
    Unloaded,
    Differs,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchdStatus {
    pub request: LaunchdRequest,
    pub path: PathBuf,
    pub loaded: bool,
    pub state: LaunchdState,
}

impl LaunchdRequest {
    pub fn from_toml(name: String, config: LaunchdTomlConfig) -> Result<Self> {
        if !valid_name(&name) {
            bail!("agent name '{name}' must contain only letters, numbers, '.', '_', or '-'");
        }
        let Some(program) = config.program.map(|s| s.trim().to_string()) else {
            bail!("agent '{name}' must set `program`");
        };
        if program.is_empty() {
            bail!("agent '{name}' must set a non-empty `program`");
        }
        if let Some(interval) = &config.start_calendar_interval {
            interval.validate(&name)?;
        }
        Ok(Self {
            label: format!("dev.mise.{name}"),
            name,
            program,
            args: config.args,
            run_at_load: config.run_at_load,
            keep_alive: config.keep_alive,
            start_interval: config.start_interval,
            start_calendar_interval: config.start_calendar_interval,
            environment: config.environment,
            working_directory: config.working_directory,
            stdout_path: config.stdout_path,
            stderr_path: config.stderr_path,
            kickstart: config.kickstart,
        })
    }
}

impl LaunchdCalendarInterval {
    fn validate(&self, agent_name: &str) -> Result<()> {
        if self.minute.is_none()
            && self.hour.is_none()
            && self.day.is_none()
            && self.weekday.is_none()
            && self.month.is_none()
        {
            bail!("agent '{agent_name}' `start_calendar_interval` must set at least one field");
        }
        validate_range(agent_name, "minute", self.minute, 0, 59)?;
        validate_range(agent_name, "hour", self.hour, 0, 23)?;
        validate_range(agent_name, "day", self.day, 1, 31)?;
        validate_range(agent_name, "weekday", self.weekday, 0, 7)?;
        validate_range(agent_name, "month", self.month, 1, 12)?;
        Ok(())
    }
}

impl LaunchdCalendarIntervals {
    fn validate(&self, agent_name: &str) -> Result<()> {
        match self {
            Self::Single(interval) => interval.validate(agent_name),
            Self::Multiple(intervals) => {
                if intervals.is_empty() {
                    bail!("agent '{agent_name}' `start_calendar_interval` must not be empty");
                }
                for interval in intervals {
                    interval.validate(agent_name)?;
                }
                Ok(())
            }
        }
    }
}

fn validate_range(
    agent_name: &str,
    field: &str,
    value: Option<u8>,
    min: u8,
    max: u8,
) -> Result<()> {
    if let Some(value) = value
        && !(min..=max).contains(&value)
    {
        bail!(
            "agent '{agent_name}' `start_calendar_interval.{field}` must be between {min} and {max}"
        );
    }
    Ok(())
}

impl std::fmt::Display for LaunchdRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.label)
    }
}

pub fn is_available() -> bool {
    cfg!(target_os = "macos") && crate::file::which("launchctl").is_some()
}

pub fn unavailable_reason() -> String {
    if cfg!(target_os = "macos") {
        "`launchctl` not found".to_string()
    } else {
        "only available on macos".to_string()
    }
}

pub async fn status(requests: &[LaunchdRequest]) -> Result<Vec<LaunchdStatus>> {
    let mut out = vec![];
    for req in requests {
        let path = plist_path(req);
        let loaded = is_loaded(&req.label).await?;
        let state = match std::fs::read(&path) {
            Ok(current) if plist_matches(&current, req) => {
                if loaded {
                    LaunchdState::Loaded
                } else {
                    LaunchdState::Unloaded
                }
            }
            Ok(_) => LaunchdState::Differs,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => LaunchdState::Missing,
            Err(err) => return Err(err.into()),
        };
        out.push(LaunchdStatus {
            request: req.clone(),
            path,
            loaded,
            state,
        });
    }
    Ok(out)
}

pub async fn apply(requests: &[LaunchdRequest], dry_run: bool) -> Result<()> {
    for req in requests {
        let path = plist_path(req);
        let domain = launchctl_domain();
        let target = format!("{domain}/{}", req.label);
        let plist = render_plist(req)?;
        if dry_run {
            miseprintln!(
                "{}",
                shell_words::join([
                    "mkdir".to_string(),
                    "-p".to_string(),
                    launch_agents_dir().display().to_string(),
                ])
            );
            miseprintln!("write {}", shell_words::join([path.display().to_string()]));
            miseprintln!(
                "{}",
                shell_words::join([
                    "launchctl".to_string(),
                    "bootout".to_string(),
                    domain.clone(),
                    path.display().to_string(),
                ])
            );
            miseprintln!(
                "{}",
                shell_words::join([
                    "launchctl".to_string(),
                    "bootstrap".to_string(),
                    domain.clone(),
                    path.display().to_string(),
                ])
            );
            miseprintln!(
                "{}",
                shell_words::join([
                    "launchctl".to_string(),
                    "enable".to_string(),
                    target.clone()
                ])
            );
            if req.kickstart {
                miseprintln!(
                    "{}",
                    shell_words::join([
                        "launchctl".to_string(),
                        "kickstart".to_string(),
                        "-k".to_string(),
                        target,
                    ])
                );
            }
            continue;
        }
        std::fs::create_dir_all(launch_agents_dir())?;
        std::fs::write(&path, plist)?;
        bootout(&domain, &path).await?;
        launchctl(&[
            "bootstrap".to_string(),
            domain.clone(),
            path.to_string_lossy().to_string(),
        ])
        .await?;
        launchctl(&["enable".to_string(), target.clone()]).await?;
        if req.kickstart {
            launchctl(&["kickstart".to_string(), "-k".to_string(), target]).await?;
        }
    }
    Ok(())
}

pub fn render_plist(request: &LaunchdRequest) -> Result<Vec<u8>> {
    let mut out = vec![];
    plist::to_writer_xml(&mut out, &plist_value(request))?;
    Ok(out)
}

fn plist_value(request: &LaunchdRequest) -> Value {
    let mut dict = Dictionary::new();
    dict.insert("Label".into(), Value::String(request.label.clone()));
    let mut program_args = vec![Value::String(expand_path_string(&request.program))];
    program_args.extend(request.args.iter().cloned().map(Value::String));
    dict.insert("ProgramArguments".into(), Value::Array(program_args));
    if request.run_at_load {
        dict.insert("RunAtLoad".into(), Value::Boolean(true));
    }
    if request.keep_alive {
        dict.insert("KeepAlive".into(), Value::Boolean(true));
    }
    if let Some(interval) = request.start_interval {
        dict.insert("StartInterval".into(), Value::Integer(interval.into()));
    }
    if let Some(interval) = &request.start_calendar_interval {
        dict.insert(
            "StartCalendarInterval".into(),
            calendar_intervals_value(interval),
        );
    }
    if !request.environment.is_empty() {
        let mut env = Dictionary::new();
        for (key, value) in &request.environment {
            env.insert(key.clone(), Value::String(value.clone()));
        }
        dict.insert("EnvironmentVariables".into(), Value::Dictionary(env));
    }
    if let Some(path) = &request.working_directory {
        dict.insert(
            "WorkingDirectory".into(),
            Value::String(expand_path_string(path)),
        );
    }
    if let Some(path) = &request.stdout_path {
        dict.insert(
            "StandardOutPath".into(),
            Value::String(expand_path_string(path)),
        );
    }
    if let Some(path) = &request.stderr_path {
        dict.insert(
            "StandardErrorPath".into(),
            Value::String(expand_path_string(path)),
        );
    }
    Value::Dictionary(dict)
}

fn calendar_intervals_value(intervals: &LaunchdCalendarIntervals) -> Value {
    match intervals {
        LaunchdCalendarIntervals::Single(interval) => {
            Value::Dictionary(calendar_interval_value(interval))
        }
        LaunchdCalendarIntervals::Multiple(intervals) => Value::Array(
            intervals
                .iter()
                .map(|interval| Value::Dictionary(calendar_interval_value(interval)))
                .collect(),
        ),
    }
}

fn calendar_interval_value(interval: &LaunchdCalendarInterval) -> Dictionary {
    let mut dict = Dictionary::new();
    if let Some(value) = interval.minute {
        dict.insert("Minute".into(), Value::Integer(value.into()));
    }
    if let Some(value) = interval.hour {
        dict.insert("Hour".into(), Value::Integer(value.into()));
    }
    if let Some(value) = interval.day {
        dict.insert("Day".into(), Value::Integer(value.into()));
    }
    if let Some(value) = interval.weekday {
        dict.insert("Weekday".into(), Value::Integer(value.into()));
    }
    if let Some(value) = interval.month {
        dict.insert("Month".into(), Value::Integer(value.into()));
    }
    dict
}

fn plist_matches(current: &[u8], request: &LaunchdRequest) -> bool {
    match Value::from_reader_xml(Cursor::new(current)) {
        Ok(current) => current == plist_value(request),
        Err(_) => false,
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn launch_agents_dir() -> PathBuf {
    crate::dirs::HOME.join("Library").join("LaunchAgents")
}

fn plist_path(request: &LaunchdRequest) -> PathBuf {
    launch_agents_dir().join(format!("{}.plist", request.label))
}

fn launchctl_domain() -> String {
    format!("gui/{}", current_uid())
}

#[cfg(unix)]
fn current_uid() -> u32 {
    current_uid_from(
        nix::unistd::geteuid().as_raw(),
        crate::env::var("SUDO_UID").ok().as_deref(),
    )
}

#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

#[cfg(unix)]
fn current_uid_from(euid: u32, sudo_uid: Option<&str>) -> u32 {
    if euid == 0
        && let Some(uid) = sudo_uid.and_then(|uid| uid.parse::<u32>().ok())
        && uid != 0
    {
        return uid;
    }
    euid
}

fn expand_path_string(path: &str) -> String {
    if path == "~" {
        return crate::dirs::HOME.to_string_lossy().to_string();
    }
    crate::file::replace_path(PathBuf::from(path).as_path())
        .to_string_lossy()
        .to_string()
}

async fn is_loaded(label: &str) -> Result<bool> {
    let target = format!("{}/{}", launchctl_domain(), label);
    let output = tokio::process::Command::new("launchctl")
        .args(["print", &target])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    Ok(output.success())
}

async fn launchctl(args: &[String]) -> Result<()> {
    debug!("$ launchctl {}", shell_words::join(args));
    let output = tokio::process::Command::new("launchctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !output.status.success() {
        bail!(
            "`launchctl {}` failed: {}",
            shell_words::join(args),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

async fn bootout(domain: &str, path: &Path) -> Result<()> {
    let args = [
        "bootout".to_string(),
        domain.to_string(),
        path.to_string_lossy().to_string(),
    ];
    match launchctl(&args).await {
        Ok(()) => Ok(()),
        Err(err) if bootout_missing_error(&err.to_string()) => Ok(()),
        Err(err) => Err(err),
    }
}

fn bootout_missing_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("no such process")
        || error.contains("could not find specified service")
        || error.contains("could not find service")
        || error.contains("service is not loaded")
        || error.contains("not in domain")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launchd_request_validation() {
        let request = LaunchdRequest::from_toml(
            "my-agent".to_string(),
            LaunchdTomlConfig {
                program: Some("~/.local/bin/my-agent".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(request.label, "dev.mise.my-agent");
        assert!(LaunchdRequest::from_toml("bad/name".to_string(), Default::default()).is_err());
        assert!(
            LaunchdRequest::from_toml(
                "my-agent".to_string(),
                LaunchdTomlConfig {
                    program: Some("/bin/echo".to_string()),
                    start_calendar_interval: Some(LaunchdCalendarIntervals::Single(
                        Default::default(),
                    )),
                    ..Default::default()
                },
            )
            .is_err()
        );
        assert!(
            LaunchdRequest::from_toml(
                "my-agent".to_string(),
                LaunchdTomlConfig {
                    program: Some("/bin/echo".to_string()),
                    start_calendar_interval: Some(LaunchdCalendarIntervals::Single(
                        LaunchdCalendarInterval {
                            hour: Some(24),
                            ..Default::default()
                        }
                    )),
                    ..Default::default()
                },
            )
            .is_err()
        );
        assert!(
            LaunchdRequest::from_toml(
                "my-agent".to_string(),
                LaunchdTomlConfig {
                    program: Some("/bin/echo".to_string()),
                    start_calendar_interval: Some(LaunchdCalendarIntervals::Multiple(vec![])),
                    ..Default::default()
                },
            )
            .is_err()
        );
    }

    #[test]
    fn test_render_plist() {
        let mut environment = IndexMap::new();
        environment.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        let request = LaunchdRequest {
            name: "sync".to_string(),
            label: "dev.mise.sync".to_string(),
            program: "/bin/echo".to_string(),
            args: vec!["hello".to_string()],
            run_at_load: true,
            keep_alive: true,
            start_interval: Some(60),
            start_calendar_interval: Some(LaunchdCalendarIntervals::Single(
                LaunchdCalendarInterval {
                    hour: Some(2),
                    minute: Some(0),
                    ..Default::default()
                },
            )),
            environment,
            working_directory: Some("~".to_string()),
            stdout_path: Some("~/Library/Logs/sync.log".to_string()),
            stderr_path: Some("~/Library/Logs/sync.err.log".to_string()),
            kickstart: false,
        };
        let plist = render_plist(&request).unwrap();
        let dict = match Value::from_reader_xml(Cursor::new(plist.as_slice())).unwrap() {
            Value::Dictionary(dict) => dict,
            value => panic!("expected dictionary, got {value:?}"),
        };
        assert_eq!(
            dict.get("Label"),
            Some(&Value::String("dev.mise.sync".to_string()))
        );
        assert_eq!(dict.get("RunAtLoad"), Some(&Value::Boolean(true)));
        assert_eq!(dict.get("KeepAlive"), Some(&Value::Boolean(true)));
        assert_eq!(dict.get("StartInterval"), Some(&Value::Integer(60.into())));
        match dict.get("StartCalendarInterval") {
            Some(Value::Dictionary(interval)) => {
                assert_eq!(interval.get("Hour"), Some(&Value::Integer(2.into())));
                assert_eq!(interval.get("Minute"), Some(&Value::Integer(0.into())));
            }
            value => panic!("expected StartCalendarInterval dictionary, got {value:?}"),
        }
        assert_eq!(
            dict.get("WorkingDirectory"),
            Some(&Value::String(
                crate::dirs::HOME.to_string_lossy().to_string()
            ))
        );
        assert_eq!(
            dict.get("StandardOutPath"),
            Some(&Value::String(
                crate::dirs::HOME
                    .join("Library/Logs/sync.log")
                    .to_string_lossy()
                    .to_string()
            ))
        );
        assert_eq!(
            dict.get("StandardErrorPath"),
            Some(&Value::String(
                crate::dirs::HOME
                    .join("Library/Logs/sync.err.log")
                    .to_string_lossy()
                    .to_string()
            ))
        );
        match dict.get("ProgramArguments") {
            Some(Value::Array(args)) => {
                assert_eq!(args[0], Value::String("/bin/echo".to_string()));
                assert_eq!(args[1], Value::String("hello".to_string()));
            }
            value => panic!("expected ProgramArguments array, got {value:?}"),
        }
        match dict.get("EnvironmentVariables") {
            Some(Value::Dictionary(env)) => {
                assert_eq!(
                    env.get("PATH"),
                    Some(&Value::String("/usr/bin:/bin".to_string()))
                );
            }
            value => panic!("expected EnvironmentVariables dictionary, got {value:?}"),
        }
        assert!(plist_matches(&plist, &request));
    }

    #[test]
    fn test_render_plist_multiple_calendar_intervals() {
        let request = LaunchdRequest {
            name: "sync".to_string(),
            label: "dev.mise.sync".to_string(),
            program: "/bin/echo".to_string(),
            args: vec![],
            run_at_load: false,
            keep_alive: false,
            start_interval: None,
            start_calendar_interval: Some(LaunchdCalendarIntervals::Multiple(vec![
                LaunchdCalendarInterval {
                    hour: Some(3),
                    minute: Some(0),
                    ..Default::default()
                },
                LaunchdCalendarInterval {
                    hour: Some(12),
                    weekday: Some(1),
                    ..Default::default()
                },
            ])),
            environment: IndexMap::new(),
            working_directory: None,
            stdout_path: None,
            stderr_path: None,
            kickstart: false,
        };
        let plist = render_plist(&request).unwrap();
        let dict = match Value::from_reader_xml(Cursor::new(plist.as_slice())).unwrap() {
            Value::Dictionary(dict) => dict,
            value => panic!("expected dictionary, got {value:?}"),
        };
        match dict.get("StartCalendarInterval") {
            Some(Value::Array(intervals)) => {
                assert_eq!(intervals.len(), 2);
                match &intervals[0] {
                    Value::Dictionary(interval) => {
                        assert_eq!(interval.get("Hour"), Some(&Value::Integer(3.into())));
                        assert_eq!(interval.get("Minute"), Some(&Value::Integer(0.into())));
                    }
                    value => panic!("expected first calendar interval dictionary, got {value:?}"),
                }
                match &intervals[1] {
                    Value::Dictionary(interval) => {
                        assert_eq!(interval.get("Hour"), Some(&Value::Integer(12.into())));
                        assert_eq!(interval.get("Weekday"), Some(&Value::Integer(1.into())));
                    }
                    value => panic!("expected second calendar interval dictionary, got {value:?}"),
                }
            }
            value => panic!("expected StartCalendarInterval array, got {value:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_current_uid_prefers_sudo_uid_for_root() {
        assert_eq!(current_uid_from(0, Some("501")), 501);
        assert_eq!(current_uid_from(0, Some("0")), 0);
        assert_eq!(current_uid_from(0, Some("not-a-uid")), 0);
        assert_eq!(current_uid_from(1000, Some("501")), 1000);
    }

    #[test]
    fn test_bootout_missing_errors() {
        assert!(bootout_missing_error(
            "`launchctl bootout gui/501 foo` failed: No such process"
        ));
        assert!(bootout_missing_error(
            "`launchctl bootout gui/501 foo` failed: Could not find specified service"
        ));
        assert!(!bootout_missing_error(
            "`launchctl bootout gui/501 foo` failed: Boot-out failed: 5: Input/output error"
        ));
    }
}
