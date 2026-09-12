use std::collections::BTreeSet;
use std::path::PathBuf;

use eyre::{Result, bail};
use serde::Deserialize;

use crate::config::Config;
use crate::system::resources::{ResourceAction, ResourceId, ResourcePlan};

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

#[derive(Clone, Debug)]
pub(crate) struct GroupRequest {
    pub name: String,
    pub state: AccountState,
}

#[derive(Clone, Debug)]
pub(crate) struct UserRequest {
    pub name: String,
    pub state: AccountState,
    pub group: Option<String>,
    pub groups: Option<BTreeSet<String>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AccountRequests {
    pub groups: Vec<GroupRequest>,
    pub users: Vec<UserRequest>,
}

impl GroupRequest {
    pub(crate) fn plan(&self) -> ResourcePlan {
        ResourcePlan::new(
            ResourceId::new("group", &self.name),
            "unsupported",
            "unsupported",
            ResourceAction::Unknown,
        )
    }
}

impl UserRequest {
    pub(crate) fn plan(&self) -> ResourcePlan {
        ResourcePlan::new(
            ResourceId::new("user", &self.name),
            "unsupported",
            "unsupported",
            ResourceAction::Unknown,
        )
    }

    pub(crate) fn current_primary_group(&self) -> Option<&str> {
        None
    }
}

pub(crate) fn requests_from_config(config: &Config) -> Result<AccountRequests> {
    if accounts_configured(config) {
        bail!("bootstrap users and groups are only supported on Linux");
    }
    Ok(AccountRequests::default())
}

pub(crate) fn prepare_requests_from_config(config: &Config) -> Result<AccountRequests> {
    if accounts_configured(config) {
        warn!("ignoring [bootstrap.users] and [bootstrap.groups] on non-Linux host");
    }
    Ok(AccountRequests::default())
}

pub(crate) fn plans(_requests: &AccountRequests) -> Vec<ResourcePlan> {
    vec![]
}

pub(crate) fn apply(requests: &AccountRequests, _dry_run: bool, _yes: bool) -> Result<bool> {
    if !requests.groups.is_empty() || !requests.users.is_empty() {
        bail!("bootstrap users and groups are only supported on Linux");
    }
    Ok(true)
}

pub(crate) fn apply_privileged_plan_from_stdin() -> Result<()> {
    bail!("bootstrap users and groups are only supported on Linux")
}

fn accounts_configured(config: &Config) -> bool {
    config.config_files.values().any(|cf| {
        cf.bootstrap_config().is_some_and(|bootstrap| {
            for group in bootstrap.groups.values() {
                let _ = (group.state, group.gid, group.system);
            }
            for user in bootstrap.users.values() {
                let _ = (
                    user.state,
                    user.uid,
                    &user.group,
                    &user.groups,
                    user.exclusive_groups,
                    &user.home,
                    &user.shell,
                    &user.comment,
                    user.system,
                    user.create_home,
                    user.move_home,
                    user.remove_home,
                );
            }
            !bootstrap.groups.is_empty() || !bootstrap.users.is_empty()
        })
    })
}
