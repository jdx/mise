use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// systemd user unit written to ~/.config/systemd/user/dev.mise.<name>.service, or .timer when it sets a timer key
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[schemars(extend("oneOf" = [
    {
        "title": "systemd service",
        "required": ["exec_start"],
        "not": {
            "anyOf": [
                {"required": ["on_boot_sec"]},
                {"required": ["on_unit_active_sec"]},
                {"required": ["on_unit_inactive_sec"]},
                {"required": ["on_calendar"]},
                {"required": ["randomized_delay_sec"]},
                {"required": ["accuracy_sec"]},
                {"required": ["persistent"]},
                {"required": ["unit"]}
            ]
        },
        "properties": {"state": {"const": "present"}}
    },
    {
        "title": "systemd timer",
        "anyOf": [
            {"required": ["on_boot_sec"]},
            {"required": ["on_unit_active_sec"]},
            {"required": ["on_unit_inactive_sec"]},
            {"required": ["on_calendar"]}
        ],
        "not": {
            "anyOf": [
                {
                    "required": ["exec_start_pre"],
                    "properties": {"exec_start_pre": {"type": "array", "minItems": 1}}
                },
                {"required": ["exec_start"]},
                {
                    "required": ["exec_start_post"],
                    "properties": {"exec_start_post": {"type": "array", "minItems": 1}}
                },
                {"required": ["type"]},
                {"required": ["remain_after_exit"]},
                {"required": ["exec_stop"]},
                {
                    "required": ["exec_stop_post"],
                    "properties": {"exec_stop_post": {"type": "array", "minItems": 1}}
                },
                {"required": ["timeout_start_sec"]},
                {"required": ["timeout_stop_sec"]},
                {"required": ["no_new_privileges"]},
                {"required": ["private_tmp"]},
                {
                    "required": ["environment"],
                    "properties": {"environment": {"type": "object", "minProperties": 1}}
                },
                {
                    "required": ["environment_file"],
                    "properties": {"environment_file": {"type": "array", "minItems": 1}}
                },
                {"required": ["nice"]},
                {"required": ["umask"]},
                {"required": ["working_directory"]},
                {"required": ["restart"]},
                {"required": ["restart_sec"]},
                {"required": ["standard_output"]},
                {"required": ["standard_error"]}
            ]
        },
        "properties": {"state": {"const": "present"}}
    },
    {
        "title": "absent systemd unit",
        "required": ["state"],
        "properties": {"state": {"const": "absent"}}
    }
]))]
pub struct SystemdTomlConfig {
    /// write Description in the [Unit] section
    #[serde(default)]
    pub description: Option<String>,
    /// write After dependencies in the [Unit] section
    #[serde(default)]
    pub after: Vec<String>,
    /// write Wants dependencies in the [Unit] section
    #[serde(default)]
    pub wants: Vec<String>,
    /// write Requires dependencies in the [Unit] section
    #[serde(default)]
    pub requires: Vec<String>,
    /// write Before ordering in the [Unit] section
    #[serde(default)]
    pub before: Vec<String>,
    /// write BindsTo dependencies in the [Unit] section
    #[serde(default)]
    pub binds_to: Vec<String>,
    /// write PartOf dependencies in the [Unit] section
    #[serde(default)]
    pub part_of: Vec<String>,
    /// write Conflicts dependencies in the [Unit] section
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// write one ExecStartPre entry per command in the [Service] section
    #[serde(default)]
    #[schemars(inner(length(min = 1), regex(pattern = r".*\S.*")))]
    pub exec_start_pre: Vec<String>,
    /// write ExecStart in the [Service] section
    #[serde(default)]
    #[schemars(length(min = 1), regex(pattern = r".*\S.*"))]
    pub exec_start: Option<String>,
    /// write one ExecStartPost entry per command in the [Service] section
    #[serde(default)]
    #[schemars(inner(length(min = 1), regex(pattern = r".*\S.*")))]
    pub exec_start_post: Vec<String>,
    /// write Type in the [Service] section
    #[serde(default, rename = "type")]
    #[schemars(extend("enum" = ["simple", "exec", "forking", "oneshot", "dbus", "notify", "notify-reload", "idle"]))]
    pub service_type: Option<String>,
    /// write RemainAfterExit in the [Service] section
    #[serde(default)]
    pub remain_after_exit: Option<bool>,
    /// write ExecStop in the [Service] section
    #[serde(default)]
    pub exec_stop: Option<String>,
    /// write one ExecStopPost entry per command in the [Service] section
    #[serde(default)]
    #[schemars(inner(length(min = 1), regex(pattern = r".*\S.*")))]
    pub exec_stop_post: Vec<String>,
    /// write TimeoutStartSec in the [Service] section
    #[serde(default)]
    pub timeout_start_sec: Option<String>,
    /// write TimeoutStopSec in the [Service] section
    #[serde(default)]
    pub timeout_stop_sec: Option<String>,
    /// write NoNewPrivileges in the [Service] section
    #[serde(default)]
    pub no_new_privileges: Option<bool>,
    /// write PrivateTmp in the [Service] section
    #[serde(default)]
    pub private_tmp: Option<bool>,
    /// environment variables for Environment entries
    #[serde(default)]
    #[schemars(extend("additionalProperties" = {"type": "string", "description": "value of the environment variable"}))]
    pub environment: IndexMap<String, String>,
    /// write one EnvironmentFile entry per path in the [Service] section
    #[serde(default)]
    pub environment_file: Vec<String>,
    /// write Nice in the [Service] section
    #[serde(default)]
    #[schemars(range(min = -20, max = 19))]
    pub nice: Option<i8>,
    /// write UMask in the [Service] section
    #[serde(default)]
    #[schemars(regex(pattern = r"^(?:(?:[0-7]{1,3}|0[0-7]{3})$|\{[{%])"))]
    pub umask: Option<String>,
    /// write WorkingDirectory in the [Service] section
    #[serde(default)]
    pub working_directory: Option<String>,
    /// write Restart in the [Service] section
    #[serde(default)]
    pub restart: Option<String>,
    /// write RestartSec in the [Service] section
    #[serde(default)]
    pub restart_sec: Option<String>,
    /// write StandardOutput in the [Service] section
    #[serde(default)]
    pub standard_output: Option<String>,
    /// write StandardError in the [Service] section
    #[serde(default)]
    pub standard_error: Option<String>,
    /// write OnBootSec in the [Timer] section
    #[serde(default)]
    pub on_boot_sec: Option<String>,
    /// write OnUnitActiveSec in the [Timer] section
    #[serde(default)]
    pub on_unit_active_sec: Option<String>,
    /// write OnUnitInactiveSec in the [Timer] section
    #[serde(default)]
    pub on_unit_inactive_sec: Option<String>,
    /// write OnCalendar in the [Timer] section
    #[serde(default)]
    pub on_calendar: Option<String>,
    /// write RandomizedDelaySec in the [Timer] section
    #[serde(default)]
    pub randomized_delay_sec: Option<String>,
    /// write AccuracySec in the [Timer] section
    #[serde(default)]
    pub accuracy_sec: Option<String>,
    /// write Persistent in the [Timer] section
    #[serde(default)]
    pub persistent: Option<bool>,
    /// write Unit in the [Timer] section
    #[serde(default)]
    pub unit: Option<String>,
    /// restart the unit after writing it; when false, stop the unit after writing it
    #[serde(default = "default_start")]
    pub start: bool,
    /// write WantedBy entries in the [Install] section and enable the unit; an empty array disables previous enablement
    #[serde(default)]
    pub wanted_by: Option<Vec<String>>,
    /// present writes and manages the unit; absent stops, disables, and removes both dev.mise.<name>.service and dev.mise.<name>.timer
    #[serde(default)]
    pub state: SystemdUnitState,
}

// Whether mise keeps a unit installed or removes it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SystemdUnitState {
    #[default]
    Present,
    // Stop, disable, and delete both `dev.mise.<name>.service` and `dev.mise.<name>.timer`, so
    // the declaration does not need to say which one was installed.
    Absent,
}

fn default_start() -> bool {
    true
}
