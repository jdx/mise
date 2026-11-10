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
    /// write LaunchEvents, keyed by event stream, then event name, with launchd's case-sensitive property names
    #[serde(default)]
    #[schemars(schema_with = "launch_events")]
    pub launch_events: IndexMap<String, IndexMap<String, toml::Table>>,
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
    /// write SoftResourceLimits, the resource limits the program starts with and may raise up to the hard limits
    #[serde(default)]
    #[schemars(schema_with = "refs::launchd_resource_limits")]
    pub soft_resource_limits: Option<LaunchdResourceLimits>,
    /// write HardResourceLimits, the ceilings the program cannot raise its soft limits above
    #[serde(default)]
    #[schemars(schema_with = "refs::launchd_resource_limits")]
    pub hard_resource_limits: Option<LaunchdResourceLimits>,
    /// run `launchctl kickstart -k` after loading the agent
    #[serde(default)]
    pub kickstart: bool,
}

/// resource limits for launchd SoftResourceLimits and HardResourceLimits
// Each field is one key of the plist dictionary, named after it in snake case. The values are
// plain counts and byte sizes, as launchd.plist(5) documents them; launchd has no spelling for
// "unlimited" in a plist, so neither does this.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(extend("minProperties" = 1))]
pub struct LaunchdResourceLimits {
    /// write Core, the largest core file in bytes
    #[serde(default)]
    pub core: Option<u64>,
    /// write CPU, the CPU time in seconds
    #[serde(default)]
    pub cpu: Option<u64>,
    /// write Data, the data segment size in bytes
    #[serde(default)]
    pub data: Option<u64>,
    /// write FileSize, the largest file the program may create in bytes
    #[serde(default)]
    pub file_size: Option<u64>,
    /// write MemoryLock, the memory the program may lock in bytes
    #[serde(default)]
    pub memory_lock: Option<u64>,
    /// write NumberOfFiles, the open file descriptors; launchd starts agents with a soft limit of 256
    #[serde(default)]
    pub number_of_files: Option<u64>,
    /// write NumberOfProcesses, the processes the user may run
    #[serde(default)]
    pub number_of_processes: Option<u64>,
    /// write ResidentSetSize, the resident memory in bytes
    #[serde(default)]
    pub resident_set_size: Option<u64>,
    /// write Stack, the stack size in bytes
    #[serde(default)]
    pub stack: Option<u64>,
}

impl LaunchdResourceLimits {
    /// The plist key and value of each limit that is set, in launchd's spelling.
    pub fn entries(&self) -> Vec<(&'static str, u64)> {
        [
            ("Core", self.core),
            ("CPU", self.cpu),
            ("Data", self.data),
            ("FileSize", self.file_size),
            ("MemoryLock", self.memory_lock),
            ("NumberOfFiles", self.number_of_files),
            ("NumberOfProcesses", self.number_of_processes),
            ("ResidentSetSize", self.resident_set_size),
            ("Stack", self.stack),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect()
    }

    pub fn validate(&self, agent_name: &str, field: &str) -> Result<()> {
        if self.entries().is_empty() {
            bail!("agent '{agent_name}' `{field}` must set at least one limit");
        }
        Ok(())
    }
}

fn launch_events(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": {
            "type": "object",
            "additionalProperties": {
                "type": "object",
                "additionalProperties": refs::macos_defaults_value(generator)
            }
        },
        "default": {}
    })
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
