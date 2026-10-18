use eyre::{Result, bail};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::schema::refs;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

// mise-system rejects `keep_alive` together with `keep_alive_on_failure` when it builds the agent.
/// macOS LaunchAgent written to ~/Library/LaunchAgents/dev.mise.<name>.plist and loaded for the current user
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[schemars(extend("not" = {
    "required": ["keep_alive", "keep_alive_on_failure"],
    "properties": {"keep_alive": {"const": true}, "keep_alive_on_failure": {"const": true}}
}))]
pub struct LaunchdTomlConfig {
    /// program path for ProgramArguments[0]
    // Optional in the type, required by the schema. An `Option` field is optional to serde
    // without `#[serde(default)]`, which would also stop schemars from marking it required.
    #[schemars(required, length(min = 1), regex(pattern = r".*\S.*"))]
    pub program: Option<String>,
    /// additional ProgramArguments entries
    #[serde(default)]
    pub args: Vec<String>,
    /// write RunAtLoad
    #[serde(default)]
    pub run_at_load: bool,
    /// write KeepAlive
    #[serde(default)]
    pub keep_alive: bool,
    /// write KeepAlive = { SuccessfulExit = false }, so launchd relaunches the job only after a failure; cannot be combined with keep_alive
    #[serde(default)]
    pub keep_alive_on_failure: bool,
    /// write StartInterval in seconds
    #[serde(default)]
    pub start_interval: Option<u64>,
    /// write ThrottleInterval in seconds
    #[serde(default)]
    pub throttle_interval: Option<u64>,
    /// write Nice, the niceness of the process, from -20 to 20
    #[serde(default)]
    #[schemars(range(min = -20, max = 20))]
    pub nice: Option<i8>,
    /// write ProcessType, the scheduling band launchd runs the job in
    #[serde(default)]
    #[schemars(extend("enum" = ["Background", "Standard", "Adaptive", "Interactive"]))]
    pub process_type: Option<String>,
    /// write one or more StartCalendarInterval schedules
    #[serde(default)]
    pub start_calendar_interval: Option<LaunchdCalendarIntervals>,
    /// write QueueDirectories (absolute paths; `~` and `~/` are expanded)
    #[serde(default)]
    #[schemars(inner(length(min = 1), regex(pattern = r"^(?:/|~$|~/|\{[{%])")))]
    pub queue_directories: Vec<String>,
    /// environment variables for EnvironmentVariables
    #[serde(default)]
    #[schemars(extend("additionalProperties" = {"type": "string", "description": "value of the environment variable"}))]
    pub environment: IndexMap<String, String>,
    /// write WorkingDirectory
    #[serde(default)]
    pub working_directory: Option<String>,
    /// write StandardOutPath
    #[serde(default)]
    pub stdout_path: Option<String>,
    /// write StandardErrorPath
    #[serde(default)]
    pub stderr_path: Option<String>,
    /// run `launchctl kickstart -k` after loading the agent
    #[serde(default)]
    pub kickstart: bool,
}

/// calendar schedule fields for launchd StartCalendarInterval
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(extend("minProperties" = 1))]
pub struct LaunchdCalendarInterval {
    /// minute of the hour, 0-59; every minute when unset
    #[serde(default)]
    #[schemars(range(max = 59))]
    pub minute: Option<u8>,
    /// hour of the day, 0-23; every hour when unset
    #[serde(default)]
    #[schemars(range(max = 23))]
    pub hour: Option<u8>,
    /// day of the month, 1-31; every day when unset
    #[serde(default)]
    #[schemars(range(min = 1, max = 31))]
    pub day: Option<u8>,
    /// day of the week, 0-7, where 0 and 7 are Sunday; every day when unset
    #[serde(default)]
    #[schemars(range(max = 7))]
    pub weekday: Option<u8>,
    /// month of the year, 1-12; every month when unset
    #[serde(default)]
    #[schemars(range(min = 1, max = 12))]
    pub month: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum LaunchdCalendarIntervals {
    Single(#[schemars(schema_with = "refs::launchd_calendar_interval")] LaunchdCalendarInterval),
    Multiple(#[schemars(schema_with = "calendar_intervals")] Vec<LaunchdCalendarInterval>),
}

fn calendar_intervals(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "array",
        "minItems": 1,
        "items": refs::launchd_calendar_interval(generator)
    })
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
