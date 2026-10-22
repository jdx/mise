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

/// Docker Compose project that mise brings up, stops, or removes
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
    /// Compose profiles to enable, passed with --profile
    #[serde(default)]
    pub profiles: Vec<String>,
    /// services to manage; defaults to every service the files and profiles enable
    #[serde(default)]
    pub services: Vec<String>,
    /// services that run once and count as converged after exiting with code 0; when services is set, they must be listed there too
    #[serde(default)]
    pub oneshot: Vec<String>,
    /// running (compose up), stopped (compose stop, keeping containers and data), or absent (compose down)
    #[serde(default)]
    pub state: ComposeState,
    /// image pull policy passed to compose up as --pull
    #[serde(default)]
    pub pull: ComposePullPolicy,
    /// auto lets Compose decide, always passes --build, never passes --no-build
    #[serde(default)]
    pub build: ComposeBuildPolicy,
    /// auto lets Compose decide, always passes --force-recreate, never passes --no-recreate
    #[serde(default)]
    pub recreate: ComposeRecreatePolicy,
    /// pass --wait to compose up, so it waits for services to be running and healthy
    #[serde(default = "default_true")]
    pub wait: bool,
    /// wait timeout in seconds
    #[serde(default)]
    pub wait_timeout: Option<u64>,
    /// Compose shutdown timeout in seconds
    #[serde(default)]
    pub timeout: Option<u64>,
    /// remove containers for services that are no longer in the Compose model
    #[serde(default = "default_true")]
    pub remove_orphans: bool,
    /// pass --renew-anon-volumes to compose up, recreating anonymous volumes
    #[serde(default)]
    pub renew_anonymous_volumes: bool,
    /// pass --volumes to compose down, removing the project's volumes
    #[serde(default)]
    pub down_volumes: bool,
    /// images compose down removes, passed as --rmi
    #[serde(default)]
    pub down_images: Option<ComposeRemoveImages>,
    /// run every Compose and engine command through sudo, for a project on the system Docker daemon
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
