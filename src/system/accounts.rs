use std::collections::BTreeSet;
use std::ffi::CString;
use std::path::PathBuf;
use std::process::Command;

use eyre::{Result, WrapErr, bail, eyre};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::system::resources::{ResourceAction, ResourceId, ResourcePlan};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    #[default]
    Present,
    Absent,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct GroupTomlConfig {
    #[serde(default)]
    pub state: AccountState,
    pub gid: Option<u32>,
    #[serde(default)]
    pub system: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UserTomlConfig {
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
pub struct GroupRequest {
    pub name: String,
    pub state: AccountState,
    pub gid: Option<u32>,
    pub system: bool,
    inspection: GroupInspection,
}

#[derive(Clone, Debug)]
pub struct UserRequest {
    pub name: String,
    pub state: AccountState,
    pub uid: Option<u32>,
    pub group: Option<String>,
    pub groups: Option<BTreeSet<String>>,
    pub exclusive_groups: bool,
    pub home: Option<PathBuf>,
    pub shell: Option<PathBuf>,
    pub comment: Option<String>,
    pub system: bool,
    pub create_home: bool,
    pub move_home: bool,
    pub remove_home: bool,
    inspection: UserInspection,
}

#[derive(Clone, Debug, Default)]
pub struct AccountRequests {
    pub groups: Vec<GroupRequest>,
    pub users: Vec<UserRequest>,
}

#[derive(Clone, Debug)]
enum GroupInspection {
    Missing,
    Present {
        gid: u32,
        desired_gid_owner: Option<String>,
    },
    IdCollision {
        gid: u32,
        name: String,
    },
}

#[derive(Clone, Debug)]
enum UserInspection {
    Missing,
    Present {
        uid: u32,
        desired_uid_owner: Option<String>,
        primary_group: String,
        groups: BTreeSet<String>,
        home: PathBuf,
        shell: PathBuf,
        comment: String,
    },
    IdCollision {
        uid: u32,
        name: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AccountAction {
    CreateGroup {
        name: String,
        gid: Option<u32>,
        system: bool,
    },
    UpdateGroup {
        name: String,
        gid: u32,
    },
    RemoveGroup {
        name: String,
    },
    CreateUser {
        name: String,
        uid: Option<u32>,
        group: String,
        groups: Vec<String>,
        home: Option<PathBuf>,
        shell: Option<PathBuf>,
        comment: Option<String>,
        system: bool,
        create_home: bool,
    },
    UpdateUser {
        name: String,
        uid: Option<u32>,
        group: String,
        groups: Option<Vec<String>>,
        exclusive_groups: bool,
        home: Option<PathBuf>,
        shell: Option<PathBuf>,
        comment: Option<String>,
        move_home: bool,
    },
    RemoveUser {
        name: String,
        remove_home: bool,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AccountPlan {
    actions: Vec<AccountAction>,
}

pub fn requests_from_config(config: &Config) -> Result<AccountRequests> {
    let mut groups = IndexMap::new();
    let mut users = IndexMap::new();
    for cf in config.config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            for (name, group) in bootstrap.groups {
                groups.entry(name).or_insert(group);
            }
            for (name, user) in bootstrap.users {
                users.entry(name).or_insert(user);
            }
        }
    }
    if groups.is_empty() && users.is_empty() {
        return Ok(AccountRequests::default());
    }
    if !cfg!(target_os = "linux") {
        bail!("bootstrap users and groups are only supported on Linux");
    }
    let groups = groups
        .into_iter()
        .map(|(name, config)| GroupRequest::from_toml(name, config))
        .collect::<Result<Vec<_>>>()?;
    let users = users
        .into_iter()
        .map(|(name, config)| UserRequest::from_toml(name, config))
        .collect::<Result<Vec<_>>>()?;
    validate_requests(&groups, &users)?;
    Ok(AccountRequests { groups, users })
}

pub fn prepare_requests_from_config(config: &Config) -> Result<AccountRequests> {
    requests_from_config(config)
}

impl GroupRequest {
    fn from_toml(name: String, config: GroupTomlConfig) -> Result<Self> {
        validate_name("group", &name)?;
        if config.state == AccountState::Absent && (config.gid.is_some() || config.system) {
            bail!("absent bootstrap group '{name}' must not set gid or system");
        }
        let inspection = match nix::unistd::Group::from_name(&name)? {
            Some(group) => {
                let gid = group.gid.as_raw();
                let desired_gid_owner = match config.gid.filter(|desired| *desired != gid) {
                    Some(desired) => {
                        nix::unistd::Group::from_gid(nix::unistd::Gid::from_raw(desired))?
                            .map(|group| group.name)
                    }
                    None => None,
                };
                GroupInspection::Present {
                    gid,
                    desired_gid_owner,
                }
            }
            None => match config.gid {
                Some(gid) => match nix::unistd::Group::from_gid(nix::unistd::Gid::from_raw(gid))? {
                    Some(group) => GroupInspection::IdCollision {
                        gid,
                        name: group.name,
                    },
                    None => GroupInspection::Missing,
                },
                None => GroupInspection::Missing,
            },
        };
        Ok(Self {
            name,
            state: config.state,
            gid: config.gid,
            system: config.system,
            inspection,
        })
    }

    pub fn plan(&self) -> ResourcePlan {
        let id = ResourceId::new("group", &self.name);
        let desired = match (self.state, self.gid) {
            (AccountState::Absent, _) => "absent".to_string(),
            (AccountState::Present, Some(gid)) => format!("present gid {gid}"),
            (AccountState::Present, None) => "present".to_string(),
        };
        match (&self.inspection, self.state) {
            (GroupInspection::Missing, AccountState::Absent) => {
                ResourcePlan::new(id, "absent", desired, ResourceAction::Noop)
            }
            (GroupInspection::Missing, AccountState::Present) => {
                ResourcePlan::new(id, "absent", desired, ResourceAction::Create)
            }
            (GroupInspection::Present { gid, .. }, AccountState::Absent) if *gid == 0 => {
                ResourcePlan::new(id, "present gid 0", desired, ResourceAction::Unknown)
            }
            (GroupInspection::Present { gid, .. }, AccountState::Absent) => ResourcePlan::new(
                id,
                format!("present gid {gid}"),
                desired,
                ResourceAction::Remove,
            ),
            (
                GroupInspection::Present {
                    gid,
                    desired_gid_owner: Some(owner),
                },
                AccountState::Present,
            ) => ResourcePlan::new(
                id,
                format!("present gid {gid}; desired gid belongs to {owner}"),
                desired,
                ResourceAction::Unknown,
            ),
            (
                GroupInspection::Present {
                    gid,
                    desired_gid_owner: None,
                },
                AccountState::Present,
            ) => ResourcePlan::new(
                id,
                format!("present gid {gid}"),
                desired,
                if self.gid.is_none_or(|desired| desired == *gid) {
                    ResourceAction::Noop
                } else if *gid == 0 {
                    ResourceAction::Unknown
                } else {
                    ResourceAction::Update
                },
            ),
            (GroupInspection::IdCollision { gid, name }, AccountState::Present) => {
                ResourcePlan::new(
                    id,
                    format!("absent; gid {gid} belongs to {name}"),
                    desired,
                    ResourceAction::Unknown,
                )
            }
            (GroupInspection::IdCollision { .. }, AccountState::Absent) => {
                ResourcePlan::new(id, "absent", desired, ResourceAction::Noop)
            }
        }
    }

    fn action(&self) -> Result<Option<AccountAction>> {
        match self.plan().action {
            ResourceAction::Noop => Ok(None),
            ResourceAction::Unknown => bail!(
                "refusing unsafe change to bootstrap group '{}'; inspect `mise bootstrap plan`",
                self.name
            ),
            ResourceAction::Create => Ok(Some(AccountAction::CreateGroup {
                name: self.name.clone(),
                gid: self.gid,
                system: self.system,
            })),
            ResourceAction::Update => Ok(Some(AccountAction::UpdateGroup {
                name: self.name.clone(),
                gid: self.gid.expect("group update requires a desired gid"),
            })),
            ResourceAction::Remove => Ok(Some(AccountAction::RemoveGroup {
                name: self.name.clone(),
            })),
        }
    }
}

impl UserRequest {
    fn from_toml(name: String, config: UserTomlConfig) -> Result<Self> {
        validate_name("user", &name)?;
        if config.state == AccountState::Present && config.group.is_none() {
            bail!("present bootstrap user '{name}' requires a primary group");
        }
        if config.state == AccountState::Present && config.remove_home {
            bail!("present bootstrap user '{name}' must not set remove_home");
        }
        if config.state == AccountState::Absent
            && (config.uid.is_some()
                || config.group.is_some()
                || config.groups.is_some()
                || config.exclusive_groups
                || config.home.is_some()
                || config.shell.is_some()
                || config.comment.is_some()
                || config.system
                || config.create_home.is_some()
                || config.move_home)
        {
            bail!("absent bootstrap user '{name}' may only set state and remove_home");
        }
        if config.exclusive_groups && config.groups.is_none() {
            bail!("bootstrap user '{name}' sets exclusive_groups without groups");
        }
        if config.move_home && config.home.is_none() {
            bail!("bootstrap user '{name}' sets move_home without home");
        }
        if let Some(group) = &config.group {
            validate_name("group", group)?;
        }
        if let Some(path) = &config.home {
            validate_account_path(&name, "home", path)?;
        }
        if let Some(path) = &config.shell {
            validate_account_path(&name, "shell", path)?;
        }
        if config
            .comment
            .as_ref()
            .is_some_and(|comment| comment.contains([':', '\n', '\r']))
        {
            bail!("bootstrap user '{name}' comment must not contain ':', CR, or LF");
        }
        let mut groups = config
            .groups
            .map(|groups| {
                groups
                    .into_iter()
                    .map(|group| {
                        validate_name("group", &group)?;
                        Ok(group)
                    })
                    .collect::<Result<BTreeSet<_>>>()
            })
            .transpose()?;
        if let (Some(groups), Some(primary_group)) = (&mut groups, &config.group) {
            groups.remove(primary_group);
        }
        let inspection = inspect_user(&name, config.uid)?;
        Ok(Self {
            name,
            state: config.state,
            uid: config.uid,
            group: config.group,
            groups,
            exclusive_groups: config.exclusive_groups,
            home: config.home,
            shell: config.shell,
            comment: config.comment,
            system: config.system,
            create_home: config.create_home.unwrap_or(!config.system),
            move_home: config.move_home,
            remove_home: config.remove_home,
            inspection,
        })
    }

    pub fn plan(&self) -> ResourcePlan {
        let id = ResourceId::new("user", &self.name);
        let desired = self.desired();
        match (&self.inspection, self.state) {
            (UserInspection::Missing, AccountState::Absent) => {
                ResourcePlan::new(id, "absent", desired, ResourceAction::Noop)
            }
            (UserInspection::Missing, AccountState::Present) => {
                ResourcePlan::new(id, "absent", desired, ResourceAction::Create)
            }
            (UserInspection::Present { uid, .. }, AccountState::Absent)
                if *uid == 0 || *uid == invoking_uid() =>
            {
                ResourcePlan::new(
                    id,
                    format!("present uid {uid}"),
                    desired,
                    ResourceAction::Unknown,
                )
            }
            (UserInspection::Present { uid, .. }, AccountState::Absent) => ResourcePlan::new(
                id,
                format!("present uid {uid}"),
                desired,
                ResourceAction::Remove,
            ),
            (
                UserInspection::Present {
                    uid,
                    desired_uid_owner,
                    primary_group,
                    groups,
                    home,
                    shell,
                    comment,
                },
                AccountState::Present,
            ) => {
                if let Some(owner) = desired_uid_owner {
                    return ResourcePlan::new(
                        id,
                        format!("present uid {uid}; desired uid belongs to {owner}"),
                        desired,
                        ResourceAction::Unknown,
                    );
                }
                let groups_match = self.groups.as_ref().is_none_or(|desired| {
                    if self.exclusive_groups {
                        desired == groups
                    } else {
                        desired.is_subset(groups)
                    }
                });
                let matches = self.uid.is_none_or(|desired| desired == *uid)
                    && self.group.as_ref() == Some(primary_group)
                    && groups_match
                    && self.home.as_ref().is_none_or(|desired| desired == home)
                    && self.shell.as_ref().is_none_or(|desired| desired == shell)
                    && self
                        .comment
                        .as_ref()
                        .is_none_or(|desired| desired == comment);
                let action = if matches {
                    ResourceAction::Noop
                } else if *uid == 0 {
                    ResourceAction::Unknown
                } else {
                    ResourceAction::Update
                };
                ResourcePlan::new(
                    id,
                    describe_user(*uid, primary_group, groups, home, shell, comment),
                    desired,
                    action,
                )
            }
            (UserInspection::IdCollision { uid, name }, AccountState::Present) => {
                ResourcePlan::new(
                    id,
                    format!("absent; uid {uid} belongs to {name}"),
                    desired,
                    ResourceAction::Unknown,
                )
            }
            (UserInspection::IdCollision { .. }, AccountState::Absent) => {
                ResourcePlan::new(id, "absent", desired, ResourceAction::Noop)
            }
        }
    }

    pub fn current_primary_group(&self) -> Option<&str> {
        match &self.inspection {
            UserInspection::Present { primary_group, .. } => Some(primary_group),
            UserInspection::Missing | UserInspection::IdCollision { .. } => None,
        }
    }

    fn desired(&self) -> String {
        if self.state == AccountState::Absent {
            return if self.remove_home {
                "absent; remove home".to_string()
            } else {
                "absent; preserve home".to_string()
            };
        }
        let mut parts = vec!["present".to_string()];
        if let Some(uid) = self.uid {
            parts.push(format!("uid {uid}"));
        }
        if let Some(group) = &self.group {
            parts.push(format!("group {group}"));
        }
        if let Some(groups) = &self.groups {
            parts.push(format!(
                "{} groups {}",
                if self.exclusive_groups {
                    "exact"
                } else {
                    "additional"
                },
                groups.iter().cloned().collect::<Vec<_>>().join(",")
            ));
        }
        if let Some(home) = &self.home {
            parts.push(format!("home {}", home.display()));
        }
        if let Some(shell) = &self.shell {
            parts.push(format!("shell {}", shell.display()));
        }
        if let Some(comment) = &self.comment {
            parts.push(format!("comment {comment}"));
        }
        parts.join("; ")
    }

    fn action(&self) -> Result<Option<AccountAction>> {
        match self.plan().action {
            ResourceAction::Noop => Ok(None),
            ResourceAction::Unknown => bail!(
                "refusing unsafe change to bootstrap user '{}'; inspect `mise bootstrap plan`",
                self.name
            ),
            ResourceAction::Create => Ok(Some(AccountAction::CreateUser {
                name: self.name.clone(),
                uid: self.uid,
                group: self
                    .group
                    .clone()
                    .expect("present user has a primary group"),
                groups: self
                    .groups
                    .as_ref()
                    .map(|groups| groups.iter().cloned().collect())
                    .unwrap_or_default(),
                home: self.home.clone(),
                shell: self.shell.clone(),
                comment: self.comment.clone(),
                system: self.system,
                create_home: self.create_home,
            })),
            ResourceAction::Update => Ok(Some(AccountAction::UpdateUser {
                name: self.name.clone(),
                uid: self.uid,
                group: self
                    .group
                    .clone()
                    .expect("present user has a primary group"),
                groups: self
                    .groups
                    .as_ref()
                    .map(|groups| groups.iter().cloned().collect()),
                exclusive_groups: self.exclusive_groups,
                home: self.home.clone(),
                shell: self.shell.clone(),
                comment: self.comment.clone(),
                move_home: self.move_home,
            })),
            ResourceAction::Remove => Ok(Some(AccountAction::RemoveUser {
                name: self.name.clone(),
                remove_home: self.remove_home,
            })),
        }
    }
}

fn validate_requests(groups: &[GroupRequest], users: &[UserRequest]) -> Result<()> {
    let managed_groups = groups
        .iter()
        .map(|group| (group.name.as_str(), group.state))
        .collect::<IndexMap<_, _>>();
    for user in users
        .iter()
        .filter(|user| user.state == AccountState::Present)
    {
        for group in user
            .group
            .iter()
            .chain(user.groups.iter().flat_map(|groups| groups.iter()))
        {
            match managed_groups.get(group.as_str()) {
                Some(AccountState::Absent) => bail!(
                    "bootstrap user '{}' requires group '{group}', but that group is absent",
                    user.name
                ),
                Some(AccountState::Present) => {}
                None if nix::unistd::Group::from_name(group)?.is_none() => bail!(
                    "bootstrap user '{}' requires undeclared group '{group}'",
                    user.name
                ),
                None => {}
            }
        }
    }
    Ok(())
}

fn inspect_user(name: &str, desired_uid: Option<u32>) -> Result<UserInspection> {
    let Some(user) = nix::unistd::User::from_name(name)? else {
        return match desired_uid {
            Some(uid) => match nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))? {
                Some(user) => Ok(UserInspection::IdCollision {
                    uid,
                    name: user.name,
                }),
                None => Ok(UserInspection::Missing),
            },
            None => Ok(UserInspection::Missing),
        };
    };
    let primary_group = nix::unistd::Group::from_gid(user.gid)?
        .map(|group| group.name)
        .unwrap_or_else(|| format!("#{}", user.gid.as_raw()));
    let c_name = CString::new(name).wrap_err("bootstrap username contains a NUL byte")?;
    let mut groups = BTreeSet::new();
    for gid in nix::unistd::getgrouplist(&c_name, user.gid)? {
        if gid == user.gid {
            continue;
        }
        groups.insert(
            nix::unistd::Group::from_gid(gid)?
                .map(|group| group.name)
                .unwrap_or_else(|| format!("#{}", gid.as_raw())),
        );
    }
    let uid = user.uid.as_raw();
    let desired_uid_owner = match desired_uid.filter(|desired| *desired != uid) {
        Some(desired) => {
            nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(desired))?.map(|user| user.name)
        }
        None => None,
    };
    Ok(UserInspection::Present {
        uid,
        desired_uid_owner,
        primary_group,
        groups,
        home: user.dir,
        shell: user.shell,
        comment: user.gecos.to_string_lossy().into_owned(),
    })
}

fn invoking_uid() -> u32 {
    invoking_uid_from(
        nix::unistd::geteuid().as_raw(),
        crate::env::var("SUDO_UID").ok().as_deref(),
    )
}

fn invoking_uid_from(euid: u32, sudo_uid: Option<&str>) -> u32 {
    if euid == 0
        && let Some(uid) = sudo_uid.and_then(|uid| uid.parse::<u32>().ok())
        && uid != 0
    {
        return uid;
    }
    euid
}

fn describe_user(
    uid: u32,
    primary_group: &str,
    groups: &BTreeSet<String>,
    home: &std::path::Path,
    shell: &std::path::Path,
    comment: &str,
) -> String {
    format!(
        "present uid {uid}; group {primary_group}; groups {}; home {}; shell {}; comment {comment}",
        groups.iter().cloned().collect::<Vec<_>>().join(","),
        home.display(),
        shell.display()
    )
}

fn validate_name(kind: &str, name: &str) -> Result<()> {
    let base = name.strip_suffix('$').unwrap_or(name);
    let mut characters = base.chars();
    let valid = !name.is_empty()
        && name.len() <= 32
        && characters
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| {
            character == '_'
                || character == '-'
                || character.is_ascii_digit()
                || character.is_ascii_alphabetic()
        });
    if !valid {
        bail!(
            "invalid bootstrap {kind} name '{name}': use at most 32 ASCII letters, digits, '_' or '-', with an optional trailing '$'"
        );
    }
    Ok(())
}

fn validate_account_path(name: &str, field: &str, path: &std::path::Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("bootstrap user '{name}' {field} must be an absolute path");
    }
    if path
        .to_string_lossy()
        .chars()
        .any(|character| character == ':' || character == '\n' || character == '\r')
    {
        bail!("bootstrap user '{name}' {field} must not contain ':', CR, or LF");
    }
    Ok(())
}

pub fn plans(requests: &AccountRequests) -> Vec<ResourcePlan> {
    requests
        .groups
        .iter()
        .map(GroupRequest::plan)
        .chain(requests.users.iter().map(UserRequest::plan))
        .collect()
}

pub fn apply(requests: &AccountRequests, dry_run: bool, yes: bool) -> Result<bool> {
    let mut actions = vec![];
    let mut unknown = vec![];
    for group in requests
        .groups
        .iter()
        .filter(|group| group.state == AccountState::Present)
    {
        collect_action(
            group.plan(),
            || group.action(),
            dry_run,
            &mut actions,
            &mut unknown,
        )?;
    }
    for user in requests
        .users
        .iter()
        .filter(|user| user.state == AccountState::Present)
    {
        collect_action(
            user.plan(),
            || user.action(),
            dry_run,
            &mut actions,
            &mut unknown,
        )?;
    }
    for user in requests
        .users
        .iter()
        .filter(|user| user.state == AccountState::Absent)
    {
        collect_action(
            user.plan(),
            || user.action(),
            dry_run,
            &mut actions,
            &mut unknown,
        )?;
    }
    for group in requests
        .groups
        .iter()
        .filter(|group| group.state == AccountState::Absent)
    {
        collect_action(
            group.plan(),
            || group.action(),
            dry_run,
            &mut actions,
            &mut unknown,
        )?;
    }
    if dry_run {
        let has_unknown = !unknown.is_empty();
        for action in &actions {
            miseprintln!("would {}", action.description());
        }
        for resource in unknown {
            warn!(
                "would not change {}: current {}, desired {} (manual action required)",
                resource.id, resource.current, resource.desired
            );
        }
        if actions.is_empty() && !has_unknown {
            info!("accounts: already converged");
        }
        return Ok(true);
    }
    if actions.is_empty() {
        info!("accounts: already converged");
        return Ok(true);
    }
    if !yes
        && console::user_attended_stderr()
        && !crate::ui::prompt::confirm(format!("accounts: apply {} change(s)?", actions.len()))?
    {
        info!("accounts: skipped");
        return Ok(false);
    }
    let input = serde_json::to_vec(&AccountPlan { actions })?;
    let executable = std::env::current_exe()?.to_string_lossy().to_string();
    crate::system::sudo::run_with_input(
        &executable,
        &[
            "--no-config".to_string(),
            "--no-env".to_string(),
            "--no-hooks".to_string(),
            "bootstrap".to_string(),
            "__apply-account-plan".to_string(),
        ],
        &input,
    )?;
    info!("accounts: applied changes");
    Ok(true)
}

fn collect_action<F>(
    plan: ResourcePlan,
    action: F,
    dry_run: bool,
    actions: &mut Vec<AccountAction>,
    unknown: &mut Vec<ResourcePlan>,
) -> Result<()>
where
    F: FnOnce() -> Result<Option<AccountAction>>,
{
    if dry_run && plan.action == ResourceAction::Unknown {
        unknown.push(plan);
        return Ok(());
    }
    if let Some(action) = action()? {
        actions.push(action);
    }
    Ok(())
}

pub fn apply_privileged_plan_from_stdin() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("bootstrap users and groups are only supported on Linux");
    }
    let plan: AccountPlan = serde_json::from_reader(std::io::stdin().lock())?;
    for action in plan.actions {
        action.apply()?;
    }
    Ok(())
}

impl AccountAction {
    fn description(&self) -> String {
        match self {
            Self::CreateGroup { name, .. } => format!("create group {name}"),
            Self::UpdateGroup { name, .. } => format!("update group {name}"),
            Self::RemoveGroup { name } => format!("remove group {name}"),
            Self::CreateUser { name, .. } => format!("create user {name}"),
            Self::UpdateUser { name, .. } => format!("update user {name}"),
            Self::RemoveUser { name, remove_home } => format!(
                "remove user {name}{}",
                if *remove_home { " and home" } else { "" }
            ),
        }
    }

    fn apply(self) -> Result<()> {
        match self {
            Self::CreateGroup { name, gid, system } => {
                let mut args = vec![];
                if system {
                    args.push("--system".to_string());
                }
                if let Some(gid) = gid {
                    args.extend(["--gid".to_string(), gid.to_string()]);
                }
                args.push(name);
                run_account_command("groupadd", &args)
            }
            Self::UpdateGroup { name, gid } => {
                run_account_command("groupmod", &["--gid".to_string(), gid.to_string(), name])
            }
            Self::RemoveGroup { name } => run_account_command("groupdel", &[name]),
            Self::CreateUser {
                name,
                uid,
                group,
                groups,
                home,
                shell,
                comment,
                system,
                create_home,
            } => {
                let mut args = vec![];
                if system {
                    args.push("--system".to_string());
                }
                if let Some(uid) = uid {
                    args.extend(["--uid".to_string(), uid.to_string()]);
                }
                args.extend(["--gid".to_string(), group]);
                if !groups.is_empty() {
                    args.extend(["--groups".to_string(), groups.join(",")]);
                }
                if let Some(home) = home {
                    args.extend([
                        "--home-dir".to_string(),
                        home.to_string_lossy().into_owned(),
                    ]);
                }
                if let Some(shell) = shell {
                    args.extend(["--shell".to_string(), shell.to_string_lossy().into_owned()]);
                }
                if let Some(comment) = comment {
                    args.extend(["--comment".to_string(), comment]);
                }
                args.push(if create_home {
                    "--create-home".to_string()
                } else {
                    "--no-create-home".to_string()
                });
                args.push(name);
                run_account_command("useradd", &args)
            }
            Self::UpdateUser {
                name,
                uid,
                group,
                groups,
                exclusive_groups,
                home,
                shell,
                comment,
                move_home,
            } => {
                let mut args = vec![];
                if let Some(uid) = uid {
                    args.extend(["--uid".to_string(), uid.to_string()]);
                }
                args.extend(["--gid".to_string(), group]);
                if let Some(groups) = groups
                    && (exclusive_groups || !groups.is_empty())
                {
                    if !exclusive_groups {
                        args.push("--append".to_string());
                    }
                    args.extend(["--groups".to_string(), groups.join(",")]);
                }
                if let Some(home) = home {
                    args.extend(["--home".to_string(), home.to_string_lossy().into_owned()]);
                    if move_home {
                        args.push("--move-home".to_string());
                    }
                }
                if let Some(shell) = shell {
                    args.extend(["--shell".to_string(), shell.to_string_lossy().into_owned()]);
                }
                if let Some(comment) = comment {
                    args.extend(["--comment".to_string(), comment]);
                }
                args.push(name);
                run_account_command("usermod", &args)
            }
            Self::RemoveUser { name, remove_home } => {
                let mut args = vec![];
                if remove_home {
                    args.push("--remove".to_string());
                }
                args.push(name);
                run_account_command("userdel", &args)
            }
        }
    }
}

fn run_account_command(program: &str, args: &[String]) -> Result<()> {
    let path = ["/usr/sbin", "/usr/bin", "/sbin", "/bin"]
        .iter()
        .map(|dir| PathBuf::from(dir).join(program))
        .find(|path| path.is_file())
        .ok_or_else(|| eyre!("required account command '{program}' was not found"))?;
    info!("$ {} {}", path.display(), args.join(" "));
    let status = Command::new(&path).args(args).status()?;
    if !status.success() {
        bail!("{program} failed with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_account_names() {
        assert!(validate_name("user", "mise-cache_1").is_ok());
        assert!(validate_name("user", "1invalid").is_err());
        assert!(validate_name("user", "-invalid").is_err());
        assert!(validate_name("group", "invalid/name").is_err());
        assert!(validate_name("group", &"a".repeat(33)).is_err());
    }

    #[test]
    fn invoking_uid_prefers_sudo_uid_for_root() {
        assert_eq!(invoking_uid_from(0, Some("1000")), 1000);
        assert_eq!(invoking_uid_from(0, Some("0")), 0);
        assert_eq!(invoking_uid_from(0, Some("not-a-uid")), 0);
        assert_eq!(invoking_uid_from(1000, Some("501")), 1000);
    }

    #[test]
    fn desired_user_description_includes_managed_comment() {
        let request = UserRequest {
            name: "example".to_string(),
            state: AccountState::Present,
            uid: None,
            group: Some("example".to_string()),
            groups: None,
            exclusive_groups: false,
            home: None,
            shell: None,
            comment: Some("managed by mise".to_string()),
            system: true,
            create_home: false,
            move_home: false,
            remove_home: false,
            inspection: UserInspection::Missing,
        };
        assert!(request.desired().contains("comment managed by mise"));
    }

    #[test]
    fn primary_group_is_not_treated_as_supplementary() {
        let request = UserRequest::from_toml(
            "mise-primary-group-test".to_string(),
            UserTomlConfig {
                group: Some("developers".to_string()),
                groups: Some(vec!["sudo".to_string(), "developers".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(request.groups, Some(BTreeSet::from(["sudo".to_string()])));
    }
}
