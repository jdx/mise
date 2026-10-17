use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeState {
    #[default]
    Running,
    Stopped,
    Absent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposePullPolicy {
    Always,
    #[default]
    Missing,
    Never,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeBuildPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeRecreatePolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeRemoveImages {
    Local,
    All,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ComposeTomlConfig {
    pub project_dir: PathBuf,
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub env_files: Vec<PathBuf>,
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
    #[serde(default)]
    pub wait_timeout: Option<u64>,
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
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub engine_command: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

fn default_true() -> bool {
    true
}
