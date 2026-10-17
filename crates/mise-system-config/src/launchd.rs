use eyre::{Result, bail};
use indexmap::IndexMap;
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
    /// `KeepAlive = { SuccessfulExit = false }`: relaunch only after a failure.
    #[serde(default)]
    pub keep_alive_on_failure: bool,
    #[serde(default)]
    pub start_interval: Option<u64>,
    #[serde(default)]
    pub throttle_interval: Option<u64>,
    /// Niceness of the process (`Nice` in the plist).
    #[serde(default)]
    pub nice: Option<i8>,
    /// Scheduling band launchd places the job in (`ProcessType` in the plist):
    /// `Background`, `Standard`, `Adaptive` or `Interactive`.
    #[serde(default)]
    pub process_type: Option<String>,
    #[serde(default)]
    pub start_calendar_interval: Option<LaunchdCalendarIntervals>,
    #[serde(default)]
    pub queue_directories: Vec<String>,
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
    pub fn validate(&self, agent_name: &str) -> Result<()> {
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
