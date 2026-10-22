use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    #[default]
    Running,
    Stopped,
    // User scope only: the installed service definition is removed.
    Absent,
}

impl ServiceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Absent => "absent",
        }
    }
}

// Which service manager a `[bootstrap.services]` entry targets.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceScope {
    // An existing Linux systemd system unit (the original behaviour).
    #[default]
    System,
    // A service mise defines for the current user: a systemd user unit on Linux, a LaunchAgent
    // on macOS, a Scheduled Task on Windows.
    User,
}

// Restart policy of a user-scope service, with one meaning on every platform.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceRestart {
    // Restart whenever the process exits, even successfully.
    Always,
    // Restart only after a failure.
    #[default]
    OnFailure,
    // Never restart automatically.
    Never,
}

impl ServiceRestart {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::OnFailure => "on-failure",
            Self::Never => "never",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceChangeAction {
    Reload,
    Restart,
    #[default]
    ReloadOrRestart,
    None,
}

/// Linux systemd system unit that mise starts, stops, enables, or masks, or a user service mise defines from command or builtin
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema)]
#[schemars(extend("not" = {"required": ["builtin", "command"]}))]
pub struct ServiceTomlConfig {
    /// running or stopped; absent removes a user service's definition (user scope only)
    #[serde(default)]
    pub state: ServiceState,
    /// start the service at boot (system scope) or at login (user scope)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// System scope only
    #[serde(default)]
    pub masked: bool,
    /// System scope only
    #[serde(default)]
    pub on_change: ServiceChangeAction,
    /// Defaults to system; builtin implies user
    #[serde(default)]
    pub scope: Option<ServiceScope>,
    /// A mise-provided user service
    #[serde(default)]
    #[schemars(extend("enum" = ["history-watch", "tool-update"]))]
    pub builtin: Option<String>,
    /// User service command; mutually exclusive with builtin
    #[serde(default)]
    #[schemars(length(min = 1))]
    pub command: Option<String>,
    /// text the service manager shows for the service; user scope only
    #[serde(default)]
    pub description: Option<String>,
    /// when to restart the process after it exits: on-failure, always, or never; on Windows, always and on-failure both retry failed runs only; user scope only
    #[serde(default)]
    #[schemars(extend("default" = "on-failure"))]
    pub restart: Option<ServiceRestart>,
    /// environment variables for the process; user scope only
    #[serde(default)]
    #[schemars(extend("additionalProperties" = {"type": "string", "description": "value of the environment variable"}))]
    pub environment: IndexMap<String, String>,
    /// directory to run the process in, with ~ expanded; user scope only
    #[serde(default)]
    pub working_directory: Option<String>,
    /// start the service only after `mise bootstrap` installs [tools] and plugin packages; user scope only
    #[serde(default)]
    pub requires_tools: bool,
}

impl Default for ServiceTomlConfig {
    fn default() -> Self {
        Self {
            state: ServiceState::Running,
            enabled: true,
            masked: false,
            on_change: ServiceChangeAction::default(),
            scope: None,
            builtin: None,
            command: None,
            description: None,
            restart: None,
            environment: IndexMap::new(),
            working_directory: None,
            requires_tools: false,
        }
    }
}

impl ServiceTomlConfig {
    /// The effective scope: explicit `scope`, else `"user"` when a `builtin`
    /// is named, else `"system"`.
    pub fn scope(&self) -> ServiceScope {
        self.scope.unwrap_or(if self.builtin.is_some() {
            ServiceScope::User
        } else {
            ServiceScope::System
        })
    }

    /// Fields that only apply to user-scope services, when set.
    pub fn user_only_fields(&self) -> Vec<&'static str> {
        [
            (self.builtin.is_some(), "builtin"),
            (self.command.is_some(), "command"),
            (self.description.is_some(), "description"),
            (self.restart.is_some(), "restart"),
            (!self.environment.is_empty(), "environment"),
            (self.working_directory.is_some(), "working_directory"),
            (self.requires_tools, "requires_tools"),
            (self.state == ServiceState::Absent, "state = \"absent\""),
        ]
        .into_iter()
        .filter_map(|(is_set, field)| is_set.then_some(field))
        .collect()
    }
}

fn default_true() -> bool {
    true
}
