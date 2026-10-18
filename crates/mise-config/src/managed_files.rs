use mise_bootstrap::{ManagedFilePhase, ManagedState};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ManagedFileTomlConfig {
    #[serde(default)]
    pub phase: ManagedFilePhase,
    pub source: Option<String>,
    pub content: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>,
    #[serde(default)]
    pub template: bool,
    #[serde(default)]
    pub remove_empty: bool,
    #[serde(default)]
    pub state: ManagedState,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub notify: Vec<String>,
    #[serde(default, deserialize_with = "crate::packages::deserialize_package_os")]
    pub os: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ManagedDirectoryTomlConfig {
    #[serde(default)]
    pub phase: ManagedFilePhase,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>,
    #[serde(default)]
    pub state: ManagedState,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub notify: Vec<String>,
    #[serde(default, deserialize_with = "crate::packages::deserialize_package_os")]
    pub os: Vec<String>,
}
