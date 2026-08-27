//! Bootstrap user and group declarations, parsed the same way on every
//! platform. `accounts` (Linux) and `accounts_non_linux` re-export these.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AccountState {
    #[default]
    Present,
    Absent,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct GroupTomlConfig {
    #[serde(default)]
    pub state: AccountState,
    pub gid: Option<u32>,
    #[serde(default)]
    pub system: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct UserTomlConfig {
    #[serde(default)]
    pub state: AccountState,
    pub uid: Option<u32>,
    pub group: Option<String>,
    pub groups: Option<Vec<String>>,
    #[serde(default)]
    pub exclusive_groups: bool,
    pub home: Option<PathBuf>,
    pub shell: Option<PathBuf>,
    pub comment: Option<String>,
    #[serde(default)]
    pub system: bool,
    pub create_home: Option<bool>,
    #[serde(default)]
    pub move_home: bool,
    #[serde(default)]
    pub remove_home: bool,
}
