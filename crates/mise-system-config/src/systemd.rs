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
    pub requires: Vec<String>,
    #[serde(default)]
    pub before: Vec<String>,
    #[serde(default)]
    pub binds_to: Vec<String>,
    #[serde(default)]
    pub part_of: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub exec_start_pre: Vec<String>,
    #[serde(default)]
    pub exec_start: Option<String>,
    #[serde(default)]
    pub exec_start_post: Vec<String>,
    #[serde(default, rename = "type")]
    pub service_type: Option<String>,
    #[serde(default)]
    pub remain_after_exit: Option<bool>,
    #[serde(default)]
    pub exec_stop: Option<String>,
    #[serde(default)]
    pub exec_stop_post: Vec<String>,
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
    pub environment_file: Vec<String>,
    #[serde(default)]
    pub nice: Option<i8>,
    #[serde(default)]
    pub umask: Option<String>,
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
    #[serde(default)]
    pub state: SystemdUnitState,
}

/// Whether mise keeps a unit installed or removes it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemdUnitState {
    #[default]
    Present,
    /// Stop, disable, and delete both `dev.mise.<name>.service` and
    /// `dev.mise.<name>.timer`, so the declaration does not need to say which
    /// one was installed.
    Absent,
}

fn default_start() -> bool {
    true
}
