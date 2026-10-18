use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComposeState {
    #[default]
    Running,
    Stopped,
    Absent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComposePullPolicy {
    Always,
    #[default]
    Missing,
    Never,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComposeBuildPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComposeRecreatePolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComposeRemoveImages {
    Local,
    All,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema)]
pub struct ComposeTomlConfig {
    /// absolute directory containing the Compose project
    #[schemars(length(min = 1))]
    pub project_dir: PathBuf,
    /// Compose file paths relative to project_dir unless absolute
    #[serde(default)]
    pub files: Vec<PathBuf>,
    /// environment file paths relative to project_dir unless absolute
    #[serde(default)]
    pub env_files: Vec<PathBuf>,
    /// optional Compose project name
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub oneshot: Vec<String>,
    #[serde(default)]
    pub state: ComposeState,
    #[serde(default)]
    pub pull: ComposePullPolicy,
    #[serde(default)]
    pub build: ComposeBuildPolicy,
    #[serde(default)]
    pub recreate: ComposeRecreatePolicy,
    #[serde(default = "default_true")]
    pub wait: bool,
    /// wait timeout in seconds
    #[serde(default)]
    pub wait_timeout: Option<u64>,
    /// Compose shutdown timeout in seconds
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default = "default_true")]
    pub remove_orphans: bool,
    #[serde(default)]
    pub renew_anonymous_volumes: bool,
    #[serde(default)]
    pub down_volumes: bool,
    #[serde(default)]
    pub down_images: Option<ComposeRemoveImages>,
    #[serde(default)]
    pub sudo: bool,
    /// standalone Compose command
    #[serde(default)]
    pub command: Vec<String>,
    /// container engine command
    #[serde(default)]
    pub engine_command: Vec<String>,
    /// bootstrap resource dependencies
    #[serde(default)]
    pub depends_on: Vec<String>,
}

fn default_true() -> bool {
    true
}
