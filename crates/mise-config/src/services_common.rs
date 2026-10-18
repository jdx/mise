use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    #[default]
    Running,
    Stopped,
    /// User scope only: the installed service definition is removed.
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

/// Which service manager a `[bootstrap.services]` entry targets.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceScope {
    /// An existing Linux systemd system unit (the original behaviour).
    #[default]
    System,
    /// A service mise defines for the current user: a systemd user unit on
    /// Linux, a LaunchAgent on macOS, a Scheduled Task on Windows.
    User,
}

/// Restart policy of a user-scope service, with one meaning on every platform.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceRestart {
    /// Restart whenever the process exits, even successfully.
    Always,
    /// Restart only after a failure.
    #[default]
    OnFailure,
    /// Never restart automatically.
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceChangeAction {
    Reload,
    Restart,
    #[default]
    ReloadOrRestart,
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ServiceTomlConfig {
    #[serde(default)]
    pub state: ServiceState,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub masked: bool,
    #[serde(default)]
    pub on_change: ServiceChangeAction,
    /// `"system"` (default) or `"user"`. A `builtin` implies `"user"`.
    #[serde(default)]
    pub scope: Option<ServiceScope>,
    /// A service definition mise supplies (for example `"history-watch"`).
    #[serde(default)]
    pub builtin: Option<String>,
    /// User scope: the command line to run.
    #[serde(default)]
    pub command: Option<String>,
    /// User scope: a human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// User scope: restart policy (default `"on-failure"`).
    #[serde(default)]
    pub restart: Option<ServiceRestart>,
    /// User scope: environment variables for the process.
    #[serde(default)]
    pub environment: IndexMap<String, String>,
    /// User scope: working directory (`~` is expanded).
    #[serde(default)]
    pub working_directory: Option<String>,
    /// User scope: converge after `[tools]` are installed.
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
