use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "linux"))]
use std::collections::HashSet;

use eyre::{Result, WrapErr, bail, eyre};
use indexmap::IndexMap;
use path_absolutize::Absolutize;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::system::resources::{ResourceAction, ResourceId, ResourcePlan};

#[derive(Clone, Debug, Deserialize)]
pub struct ManagedFileTomlConfig {
    pub source: Option<String>,
    pub content: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>,
    #[serde(default)]
    pub template: bool,
    #[serde(default)]
    pub state: ManagedState,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub notify: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ManagedDirectoryTomlConfig {
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
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedState {
    #[default]
    Present,
    Absent,
}

#[derive(Clone, Debug)]
pub struct ManagedFileRequest {
    pub path: PathBuf,
    pub content: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: u32,
    pub state: ManagedState,
    pub replace: bool,
    pub notify: Vec<String>,
    inspection: Option<PathInspection>,
}

#[derive(Clone, Debug)]
pub struct ManagedDirectoryRequest {
    pub path: PathBuf,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: u32,
    pub state: ManagedState,
    pub recursive: bool,
    pub replace: bool,
    pub notify: Vec<String>,
    inspection: Option<PathInspection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PrivilegedAction {
    WriteFile {
        path: PathBuf,
        content: String,
        owner: Option<String>,
        group: Option<String>,
        mode: u32,
        replace: bool,
    },
    RemoveFile {
        path: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
        owner: Option<String>,
        group: Option<String>,
        mode: u32,
        replace: bool,
    },
    RemoveDirectory {
        path: PathBuf,
        recursive: bool,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PrivilegedPlan {
    pub actions: Vec<PrivilegedAction>,
}

#[derive(Clone, Debug, Default)]
pub struct ApplyReport {
    pub notified_services: super::services::ServiceNotifications,
}

pub fn pending_notifications(
    files: &[ManagedFileRequest],
    directories: &[ManagedDirectoryRequest],
) -> Result<super::services::ServiceNotifications> {
    let mut notifications = super::services::ServiceNotifications::default();
    for directory in directories {
        if matches!(
            directory.plan()?.action,
            ResourceAction::Create | ResourceAction::Update | ResourceAction::Remove
        ) {
            notifications.notify_directory(&directory.path, &directory.notify);
        }
    }
    for file in files {
        if matches!(
            file.plan()?.action,
            ResourceAction::Create | ResourceAction::Update | ResourceAction::Remove
        ) {
            notifications.notify_file(&file.path, &file.notify);
        }
    }
    Ok(notifications)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PrivilegedInspectionPlan {
    paths: Vec<PrivilegedPathInspection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PrivilegedPathInspection {
    path: PathBuf,
    expected_content: Option<String>,
    owner: Option<String>,
    group: Option<String>,
    mode: u32,
    check_metadata: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum PathInspection {
    Missing,
    Present {
        kind: ManagedPathKind,
        current: String,
        metadata_matches: bool,
        content_matches: Option<bool>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ManagedPathKind {
    File,
    Directory,
    Symlink,
    Other,
}

pub fn requests_from_config(
    config: &Config,
    secrets: &super::secrets::SecretValues,
) -> Result<(Vec<ManagedFileRequest>, Vec<ManagedDirectoryRequest>)> {
    let (mut files, mut directories) = prepare_requests_from_config(config, secrets)?;
    inspect_requests(&mut files, &mut directories)?;
    Ok((files, directories))
}

pub fn status_requests_from_config(
    config: &Config,
    secrets: &super::secrets::SecretValues,
) -> Result<(
    Vec<ManagedFileRequest>,
    Vec<ManagedDirectoryRequest>,
    Vec<ResourcePlan>,
)> {
    let mut files = vec![];
    let mut unavailable = vec![];
    let mut directories = directories_from_config(config)?;
    let directory_states = directories
        .iter()
        .map(|directory| (directory.path.as_path(), directory.state))
        .collect::<std::collections::HashMap<_, _>>();
    for (path, (file, base)) in merged_files_from_config(config)? {
        let state = file.state;
        match ManagedFileRequest::from_toml(config, path.clone(), file, &base, secrets) {
            Ok(file) => files.push(file),
            Err(error) if super::secrets::is_unavailable(&error) => {
                if directory_states.contains_key(path.as_path()) {
                    bail!(
                        "managed system path '{}' is declared as both a file and a directory",
                        path.display()
                    );
                }
                validate_present_ancestors(&path, state, &directory_states)?;
                unavailable.push(ResourcePlan::new(
                    ResourceId::new("file", path.to_string_lossy().into_owned()),
                    "not inspected: required secret unavailable",
                    "template rendered",
                    ResourceAction::Unknown,
                ));
            }
            Err(error) => return Err(error),
        }
    }
    ignore_non_linux_account_principals(config, &mut files, &mut directories);
    validate_requests(&files, &directories)?;
    inspect_paths(&mut files, &mut directories)?;
    Ok((files, directories, unavailable))
}

pub fn prepare_requests_from_config(
    config: &Config,
    secrets: &super::secrets::SecretValues,
) -> Result<(Vec<ManagedFileRequest>, Vec<ManagedDirectoryRequest>)> {
    let mut files = files_from_config(config, secrets)?;
    let mut directories = directories_from_config(config)?;
    ignore_non_linux_account_principals(config, &mut files, &mut directories);
    validate_requests(&files, &directories)?;
    Ok((files, directories))
}

#[cfg(target_os = "linux")]
fn ignore_non_linux_account_principals(
    _config: &Config,
    _files: &mut [ManagedFileRequest],
    _directories: &mut [ManagedDirectoryRequest],
) {
}

#[cfg(not(target_os = "linux"))]
fn ignore_non_linux_account_principals(
    config: &Config,
    files: &mut [ManagedFileRequest],
    directories: &mut [ManagedDirectoryRequest],
) {
    let mut users = HashSet::new();
    let mut groups = HashSet::new();
    for cf in config.config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            users.extend(bootstrap.users.into_keys());
            groups.extend(bootstrap.groups.into_keys());
        }
    }
    clear_ignored_principals(files, directories, &users, &groups);
}

#[cfg(any(not(target_os = "linux"), test))]
fn clear_ignored_principals(
    files: &mut [ManagedFileRequest],
    directories: &mut [ManagedDirectoryRequest],
    users: &std::collections::HashSet<String>,
    groups: &std::collections::HashSet<String>,
) {
    for (kind, path, owner, group) in files
        .iter_mut()
        .map(|request| {
            (
                "file",
                request.path.as_path(),
                &mut request.owner,
                &mut request.group,
            )
        })
        .chain(directories.iter_mut().map(|request| {
            (
                "directory",
                request.path.as_path(),
                &mut request.owner,
                &mut request.group,
            )
        }))
    {
        if owner.as_ref().is_some_and(|owner| users.contains(owner)) {
            warn!(
                "ignoring owner '{}' for managed {kind} '{}' because [bootstrap.users] is Linux-only",
                owner.as_deref().expect("matching owner is present"),
                path.display()
            );
            *owner = None;
        }
        if group.as_ref().is_some_and(|group| groups.contains(group)) {
            warn!(
                "ignoring group '{}' for managed {kind} '{}' because [bootstrap.groups] is Linux-only",
                group.as_deref().expect("matching group is present"),
                path.display()
            );
            *group = None;
        }
    }
}

pub fn inspect_requests(
    files: &mut [ManagedFileRequest],
    directories: &mut [ManagedDirectoryRequest],
) -> Result<()> {
    inspect_paths(files, directories)
}

fn files_from_config(
    config: &Config,
    secrets: &super::secrets::SecretValues,
) -> Result<Vec<ManagedFileRequest>> {
    merged_files_from_config(config)?
        .into_iter()
        .map(|(path, (file, base))| {
            ManagedFileRequest::from_toml(config, path, file, &base, secrets)
        })
        .collect()
}

fn merged_files_from_config(
    config: &Config,
) -> Result<IndexMap<PathBuf, (ManagedFileTomlConfig, PathBuf)>> {
    let mut merged: IndexMap<PathBuf, (ManagedFileTomlConfig, PathBuf)> = IndexMap::new();
    // Config files are ordered from highest to lowest precedence. Preserve the
    // first declaration of a target so a parent or global layer cannot replace
    // the nearer project declaration.
    for cf in config.config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            let mut layer_paths = IndexMap::new();
            let base = cf
                .get_path()
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            for (path, file) in bootstrap.files {
                let target = absolute_target(&path)?;
                if let Some(previous) = layer_paths.insert(target.clone(), path.clone()) {
                    bail!(
                        "managed file paths '{previous}' and '{path}' normalize to the same target '{}'",
                        target.display()
                    );
                }
                merged.entry(target).or_insert_with(|| (file, base.clone()));
            }
        }
    }
    Ok(merged)
}

fn directories_from_config(config: &Config) -> Result<Vec<ManagedDirectoryRequest>> {
    let mut merged: IndexMap<PathBuf, ManagedDirectoryTomlConfig> = IndexMap::new();
    for cf in config.config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            let mut layer_paths = IndexMap::new();
            for (path, directory) in bootstrap.directories {
                let target = absolute_target(&path)?;
                if let Some(previous) = layer_paths.insert(target.clone(), path.clone()) {
                    bail!(
                        "managed directory paths '{previous}' and '{path}' normalize to the same target '{}'",
                        target.display()
                    );
                }
                merged.entry(target).or_insert(directory);
            }
        }
    }
    merged
        .into_iter()
        .map(|(path, config)| ManagedDirectoryRequest::from_toml(path, config))
        .collect()
}

fn validate_requests(
    files: &[ManagedFileRequest],
    directories: &[ManagedDirectoryRequest],
) -> Result<()> {
    let directory_states = directories
        .iter()
        .map(|directory| (directory.path.as_path(), directory.state))
        .collect::<std::collections::HashMap<_, _>>();
    for file in files {
        if directory_states.contains_key(file.path.as_path()) {
            bail!(
                "managed system path '{}' is declared as both a file and a directory",
                file.path.display()
            );
        }
        validate_present_ancestors(&file.path, file.state, &directory_states)?;
    }
    for directory in directories {
        validate_present_ancestors(&directory.path, directory.state, &directory_states)?;
    }
    Ok(())
}

fn validate_present_ancestors(
    path: &Path,
    state: ManagedState,
    directory_states: &std::collections::HashMap<&Path, ManagedState>,
) -> Result<()> {
    if state != ManagedState::Present {
        return Ok(());
    }
    if let Some(parent) = path
        .ancestors()
        .skip(1)
        .find(|parent| directory_states.get(parent) == Some(&ManagedState::Absent))
    {
        bail!(
            "managed path '{}' cannot be present while managed ancestor '{}' is absent",
            path.display(),
            parent.display()
        );
    }
    Ok(())
}

impl ManagedFileRequest {
    fn from_toml(
        root_config: &Config,
        path: PathBuf,
        config: ManagedFileTomlConfig,
        base: &Path,
        secrets: &super::secrets::SecretValues,
    ) -> Result<Self> {
        let owner = nonempty("owner", config.owner)?;
        let group = nonempty("group", config.group)?;
        let mode = parse_mode(config.mode.as_deref(), 0o644)?;
        let mut content = match (config.source, config.content, config.state) {
            (Some(_), Some(_), _) => {
                bail!(
                    "[bootstrap.files].\"{}\": source and content are mutually exclusive",
                    path.display()
                )
            }
            (Some(source), None, ManagedState::Present) => {
                let source = Path::new(&source);
                let source = if source.is_absolute() {
                    source.to_path_buf()
                } else {
                    base.join(source)
                };
                Some(fs::read_to_string(&source).wrap_err_with(|| {
                    format!(
                        "[bootstrap.files].\"{}\": failed to read source {}",
                        path.display(),
                        source.display()
                    )
                })?)
            }
            (None, Some(content), ManagedState::Present) => Some(content),
            (None, None, ManagedState::Present) => bail!(
                "[bootstrap.files].\"{}\": present files require source or content",
                path.display()
            ),
            (None, None, ManagedState::Absent) => None,
            (_, _, ManagedState::Absent) => bail!(
                "[bootstrap.files].\"{}\": absent files must not declare source or content",
                path.display()
            ),
        };
        if config.template {
            let rendered = content
                .as_deref()
                .map(|content| secrets.render(root_config, content, base, &path))
                .transpose()
                .wrap_err_with(|| {
                    format!(
                        "[bootstrap.files].\"{}\": failed to render template",
                        path.display()
                    )
                })?;
            content = rendered;
        }
        Ok(Self {
            path,
            content,
            owner,
            group,
            mode,
            state: config.state,
            replace: config.replace,
            notify: config.notify,
            inspection: None,
        })
    }

    pub fn plan(&self) -> Result<ResourcePlan> {
        plan_file(self)
    }

    fn operation(&self) -> Result<Option<PrivilegedAction>> {
        match self.plan()?.action {
            ResourceAction::Noop => return Ok(None),
            ResourceAction::Unknown if self.state == ManagedState::Absent => bail!(
                "refusing to remove directory {} as a file; declare it in [bootstrap.directories]",
                self.path.display()
            ),
            ResourceAction::Unknown => bail!(
                "refusing to replace non-file path {}; set replace = true to allow replacement",
                self.path.display()
            ),
            _ => {}
        }
        Ok(Some(match self.state {
            ManagedState::Present => PrivilegedAction::WriteFile {
                path: self.path.clone(),
                content: self.content.clone().expect("present file has content"),
                owner: self.owner.clone(),
                group: self.group.clone(),
                mode: self.mode,
                replace: self.replace,
            },
            ManagedState::Absent => PrivilegedAction::RemoveFile {
                path: self.path.clone(),
            },
        }))
    }
}

impl ManagedDirectoryRequest {
    fn from_toml(path: PathBuf, config: ManagedDirectoryTomlConfig) -> Result<Self> {
        if config.state == ManagedState::Present && config.recursive {
            bail!(
                "[bootstrap.directories].\"{}\": recursive is only valid with state = \"absent\"",
                path.display()
            );
        }
        Ok(Self {
            path,
            owner: nonempty("owner", config.owner)?,
            group: nonempty("group", config.group)?,
            mode: parse_mode(config.mode.as_deref(), 0o755)?,
            state: config.state,
            recursive: config.recursive,
            replace: config.replace,
            notify: config.notify,
            inspection: None,
        })
    }

    pub fn plan(&self) -> Result<ResourcePlan> {
        plan_directory(self)
    }

    fn operation(&self) -> Result<Option<PrivilegedAction>> {
        match self.plan()?.action {
            ResourceAction::Noop => return Ok(None),
            ResourceAction::Unknown if self.state == ManagedState::Absent => bail!(
                "refusing to remove non-directory path {} as a directory; declare it in [bootstrap.files]",
                self.path.display()
            ),
            ResourceAction::Unknown => bail!(
                "refusing to replace non-directory path {}; set replace = true to allow replacement",
                self.path.display()
            ),
            _ => {}
        }
        Ok(Some(match self.state {
            ManagedState::Present => PrivilegedAction::CreateDirectory {
                path: self.path.clone(),
                owner: self.owner.clone(),
                group: self.group.clone(),
                mode: self.mode,
                replace: self.replace,
            },
            ManagedState::Absent => PrivilegedAction::RemoveDirectory {
                path: self.path.clone(),
                recursive: self.recursive,
            },
        }))
    }
}

impl PrivilegedPlan {
    /// Apply actions as the current user until elevation is required. Actions
    /// that could mutate state before reporting a permission error are sent to
    /// the privileged helper without first attempting them.
    fn apply_until_elevation_required(self) -> Result<Self> {
        let mut actions = self.actions.into_iter();
        while let Some(action) = actions.next() {
            if action.requires_preemptive_elevation()? {
                return Ok(Self {
                    actions: std::iter::once(action).chain(actions).collect(),
                });
            }
            match action.apply() {
                Ok(()) => {}
                Err(error) if is_permission_denied(&error) => {
                    return Ok(Self {
                        actions: std::iter::once(action).chain(actions).collect(),
                    });
                }
                Err(error) => return Err(error),
            }
        }
        Ok(Self::default())
    }
}

pub fn apply_with_accounts(
    files: &[ManagedFileRequest],
    directories: &[ManagedDirectoryRequest],
    accounts: Option<&super::accounts::AccountRequests>,
    allow_pending_accounts: bool,
    dry_run: bool,
    yes: bool,
) -> Result<ApplyReport> {
    validate_principals(files, directories, accounts, allow_pending_accounts)?;
    let mut plan = PrivilegedPlan::default();
    let mut report = ApplyReport::default();
    let mut unknown = vec![];
    let mut present_directories = directories
        .iter()
        .filter(|request| request.state == ManagedState::Present)
        .collect::<Vec<_>>();
    present_directories.sort_by_key(|request| request.path.components().count());
    for directory in present_directories {
        let resource = directory.plan()?;
        if dry_run && resource.action == ResourceAction::Unknown {
            unknown.push(resource);
            continue;
        }
        if let Some(action) = directory.operation()? {
            plan.actions.push(action);
            report
                .notified_services
                .notify_directory(&directory.path, &directory.notify);
        }
    }
    for file in files {
        let resource = file.plan()?;
        if dry_run && resource.action == ResourceAction::Unknown {
            unknown.push(resource);
            continue;
        }
        if let Some(action) = file.operation()? {
            plan.actions.push(action);
            report
                .notified_services
                .notify_file(&file.path, &file.notify);
        }
    }
    let mut absent_directories = directories
        .iter()
        .filter(|request| request.state == ManagedState::Absent)
        .collect::<Vec<_>>();
    absent_directories.sort_by_key(|request| std::cmp::Reverse(request.path.components().count()));
    for directory in absent_directories {
        let resource = directory.plan()?;
        if dry_run && resource.action == ResourceAction::Unknown {
            unknown.push(resource);
            continue;
        }
        if let Some(action) = directory.operation()? {
            plan.actions.push(action);
            report
                .notified_services
                .notify_directory(&directory.path, &directory.notify);
        }
    }
    let descriptions = plan
        .actions
        .iter()
        .map(PrivilegedAction::description)
        .collect::<Vec<_>>();
    if dry_run {
        for description in descriptions {
            miseprintln!("would {description}");
        }
        for resource in &unknown {
            warn!(
                "would not change {}: current {}, desired {} (manual action required)",
                resource.id, resource.current, resource.desired
            );
        }
        if plan.actions.is_empty() && unknown.is_empty() {
            info!("system files: already converged");
        }
        return Ok(report);
    }
    if plan.actions.is_empty() {
        info!("system files: already converged");
        return Ok(report);
    }
    if !yes
        && console::user_attended_stderr()
        && !crate::ui::prompt::confirm(format!(
            "system files: apply {} change(s)?",
            plan.actions.len()
        ))?
    {
        info!("system files: skipped");
        return Ok(ApplyReport::default());
    }
    let change_count = plan.actions.len();
    let privileged_plan = plan.apply_until_elevation_required()?;
    if !privileged_plan.actions.is_empty() {
        let input = serde_json::to_vec(&privileged_plan)?;
        let executable = std::env::current_exe()?.to_string_lossy().to_string();
        crate::system::sudo::run_with_input(
            &executable,
            &[
                "--no-config".to_string(),
                "--no-env".to_string(),
                "--no-hooks".to_string(),
                "bootstrap".to_string(),
                "__apply-system-plan".to_string(),
            ],
            &input,
        )?;
    }
    info!("system files: applied {change_count} change(s)");
    Ok(report)
}

#[cfg(unix)]
pub fn validate_principals(
    files: &[ManagedFileRequest],
    directories: &[ManagedDirectoryRequest],
    accounts: Option<&super::accounts::AccountRequests>,
    allow_pending_accounts: bool,
) -> Result<()> {
    for (owner, group) in files
        .iter()
        .filter(|request| request.state == ManagedState::Present)
        .map(|request| (request.owner.as_deref(), request.group.as_deref()))
        .chain(
            directories
                .iter()
                .filter(|request| request.state == ManagedState::Present)
                .map(|request| (request.owner.as_deref(), request.group.as_deref())),
        )
    {
        if let Some(owner) = owner {
            match accounts
                .and_then(|accounts| accounts.users.iter().find(|request| request.name == owner))
            {
                Some(request) if request.state == super::accounts::AccountState::Absent => bail!(
                    "managed system files require owner '{owner}', but that bootstrap user is absent"
                ),
                Some(request)
                    if allow_pending_accounts
                        && request.plan().action == ResourceAction::Unknown =>
                {
                    bail!(
                        "managed system files require owner '{owner}', but that bootstrap user cannot be safely converged"
                    )
                }
                Some(_) if allow_pending_accounts => {}
                Some(_) | None => {
                    resolve_user(owner)?;
                }
            }
        }
        if let Some(group) = group {
            match accounts
                .and_then(|accounts| accounts.groups.iter().find(|request| request.name == group))
            {
                Some(request) if request.state == super::accounts::AccountState::Absent => bail!(
                    "managed system files require group '{group}', but that bootstrap group is absent"
                ),
                Some(request)
                    if allow_pending_accounts
                        && request.plan().action == ResourceAction::Unknown =>
                {
                    bail!(
                        "managed system files require group '{group}', but that bootstrap group cannot be safely converged"
                    )
                }
                Some(_) if allow_pending_accounts => {}
                Some(_) | None => {
                    resolve_group(group)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn validate_principals(
    _files: &[ManagedFileRequest],
    _directories: &[ManagedDirectoryRequest],
    _accounts: Option<&super::accounts::AccountRequests>,
    _allow_pending_accounts: bool,
) -> Result<()> {
    bail!("managed system files are only supported on Unix")
}

impl PrivilegedAction {
    fn requires_preemptive_elevation(&self) -> Result<bool> {
        match self {
            // Ownership changes normally require privilege. Avoid creating or
            // replacing a path before discovering that at set_metadata().
            Self::WriteFile {
                path,
                owner,
                group,
                replace,
                ..
            } => Ok(owner.is_some()
                || group.is_some()
                || (*replace && replacement_is_not(path, ManagedPathKind::File)?)),
            Self::CreateDirectory {
                path,
                owner,
                group,
                replace,
                ..
            } => Ok(owner.is_some()
                || group.is_some()
                || (*replace && replacement_is_not(path, ManagedPathKind::Directory)?)),
            // Recursive removal can delete writable descendants before an
            // inaccessible one fails. Run it once with the required access.
            Self::RemoveDirectory {
                recursive: true, ..
            } => Ok(true),
            Self::RemoveFile { .. } | Self::RemoveDirectory { .. } => Ok(false),
        }
    }

    fn description(&self) -> String {
        match self {
            Self::WriteFile { path, .. } => format!("write file {}", path.display()),
            Self::RemoveFile { path } => format!("remove file {}", path.display()),
            Self::CreateDirectory { path, .. } => format!("create directory {}", path.display()),
            Self::RemoveDirectory { path, recursive } => format!(
                "remove directory {}{}",
                path.display(),
                if *recursive { " recursively" } else { "" }
            ),
        }
    }
}

fn replacement_is_not(path: &Path, expected: ManagedPathKind) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(ManagedPathKind::from_metadata(&metadata) != expected),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(true),
        Err(error) => Err(error.into()),
    }
}

pub fn apply_privileged_plan_from_stdin() -> Result<()> {
    let plan: PrivilegedPlan = serde_json::from_reader(std::io::stdin().lock())?;
    for action in plan.actions {
        action.apply()?;
    }
    Ok(())
}

pub fn inspect_privileged_files_from_stdin() -> Result<()> {
    let plan: PrivilegedInspectionPlan = serde_json::from_reader(std::io::stdin().lock())?;
    let inspections = plan
        .paths
        .into_iter()
        .map(inspect_path)
        .collect::<Result<Vec<_>>>()?;
    serde_json::to_writer(std::io::stdout().lock(), &inspections)?;
    Ok(())
}

impl PrivilegedAction {
    fn apply(&self) -> Result<()> {
        match self {
            Self::WriteFile {
                path,
                content,
                owner,
                group,
                mode,
                replace,
            } => write_file(
                &validate_privileged_target(path)?,
                content.as_bytes(),
                owner.as_deref(),
                group.as_deref(),
                *mode,
                *replace,
            ),
            Self::RemoveFile { path } => remove_file(&validate_privileged_target(path)?),
            Self::CreateDirectory {
                path,
                owner,
                group,
                mode,
                replace,
            } => create_directory(
                &validate_privileged_target(path)?,
                owner.as_deref(),
                group.as_deref(),
                *mode,
                *replace,
            ),
            Self::RemoveDirectory { path, recursive } => {
                remove_directory(&validate_privileged_target(path)?, *recursive)
            }
        }
    }
}

fn plan_file(request: &ManagedFileRequest) -> Result<ResourcePlan> {
    let desired = match request.state {
        ManagedState::Present => desired_metadata(
            "file",
            request.mode,
            request.owner.as_deref(),
            request.group.as_deref(),
        ),
        ManagedState::Absent => "absent".to_string(),
    };
    let id = ResourceId::new("file", request.path.to_string_lossy());
    let inspection = request
        .inspection
        .as_ref()
        .ok_or_else(|| eyre!("managed file was not inspected: {}", request.path.display()))?;
    match (request.state, inspection) {
        (ManagedState::Absent, PathInspection::Missing) => Ok(ResourcePlan::new(
            id,
            "absent",
            desired,
            ResourceAction::Noop,
        )),
        (ManagedState::Absent, PathInspection::Present { kind, current, .. }) => {
            Ok(ResourcePlan::new(
                id,
                current,
                desired,
                if *kind == ManagedPathKind::Directory {
                    ResourceAction::Unknown
                } else {
                    ResourceAction::Remove
                },
            ))
        }
        (ManagedState::Present, PathInspection::Missing) => Ok(ResourcePlan::new(
            id,
            "absent",
            desired,
            ResourceAction::Create,
        )),
        (
            ManagedState::Present,
            PathInspection::Present {
                kind,
                current,
                metadata_matches,
                content_matches,
            },
        ) => Ok(ResourcePlan::new(
            id,
            current,
            desired,
            if *kind != ManagedPathKind::File && !request.replace {
                ResourceAction::Unknown
            } else if *kind == ManagedPathKind::File
                && content_matches == &Some(true)
                && *metadata_matches
            {
                ResourceAction::Noop
            } else {
                ResourceAction::Update
            },
        )),
    }
}

fn inspect_paths(
    files: &mut [ManagedFileRequest],
    directories: &mut [ManagedDirectoryRequest],
) -> Result<()> {
    enum Target {
        File(usize),
        Directory(usize),
    }

    let mut privileged = vec![];
    let mut targets = vec![];
    for (index, file) in files.iter_mut().enumerate() {
        let request = PrivilegedPathInspection {
            path: file.path.clone(),
            expected_content: (file.state == ManagedState::Present)
                .then(|| file.content.clone().expect("present file has content")),
            owner: file.owner.clone(),
            group: file.group.clone(),
            mode: file.mode,
            check_metadata: file.state == ManagedState::Present,
        };
        match inspect_path(request.clone()) {
            Ok(inspection) => file.inspection = Some(inspection),
            Err(error) if is_permission_denied(&error) => {
                privileged.push(request);
                targets.push(Target::File(index));
            }
            Err(error) => return Err(error),
        }
    }
    for (index, directory) in directories.iter_mut().enumerate() {
        let request = PrivilegedPathInspection {
            path: directory.path.clone(),
            expected_content: None,
            owner: directory.owner.clone(),
            group: directory.group.clone(),
            mode: directory.mode,
            check_metadata: directory.state == ManagedState::Present,
        };
        match inspect_path(request.clone()) {
            Ok(inspection) => directory.inspection = Some(inspection),
            Err(error) if is_permission_denied(&error) => {
                privileged.push(request);
                targets.push(Target::Directory(index));
            }
            Err(error) => return Err(error),
        }
    }
    if privileged.is_empty() {
        return Ok(());
    }
    let input = serde_json::to_vec(&PrivilegedInspectionPlan { paths: privileged })?;
    let executable = std::env::current_exe()?.to_string_lossy().to_string();
    let output = crate::system::sudo::run_with_input_output(
        &executable,
        &[
            "--no-config".to_string(),
            "--no-env".to_string(),
            "--no-hooks".to_string(),
            "bootstrap".to_string(),
            "__inspect-system-files".to_string(),
        ],
        &input,
    )?;
    let inspections: Vec<PathInspection> = serde_json::from_slice(&output)?;
    if inspections.len() != targets.len() {
        bail!("privileged path inspection returned an unexpected result count");
    }
    for (target, inspection) in targets.into_iter().zip(inspections) {
        match target {
            Target::File(index) => files[index].inspection = Some(inspection),
            Target::Directory(index) => directories[index].inspection = Some(inspection),
        }
    }
    Ok(())
}

fn plan_directory(request: &ManagedDirectoryRequest) -> Result<ResourcePlan> {
    let desired = match request.state {
        ManagedState::Present => desired_metadata(
            "directory",
            request.mode,
            request.owner.as_deref(),
            request.group.as_deref(),
        ),
        ManagedState::Absent => "absent".to_string(),
    };
    let id = ResourceId::new("directory", request.path.to_string_lossy());
    let inspection = request.inspection.as_ref().ok_or_else(|| {
        eyre!(
            "managed directory was not inspected: {}",
            request.path.display()
        )
    })?;
    match (request.state, inspection) {
        (ManagedState::Absent, PathInspection::Missing) => Ok(ResourcePlan::new(
            id,
            "absent",
            desired,
            ResourceAction::Noop,
        )),
        (ManagedState::Absent, PathInspection::Present { kind, current, .. }) => {
            Ok(ResourcePlan::new(
                id,
                current,
                desired,
                if *kind == ManagedPathKind::Directory {
                    ResourceAction::Remove
                } else {
                    ResourceAction::Unknown
                },
            ))
        }
        (ManagedState::Present, PathInspection::Missing) => Ok(ResourcePlan::new(
            id,
            "absent",
            desired,
            ResourceAction::Create,
        )),
        (
            ManagedState::Present,
            PathInspection::Present {
                kind,
                current,
                metadata_matches,
                ..
            },
        ) => Ok(ResourcePlan::new(
            id,
            current,
            desired,
            if *kind != ManagedPathKind::Directory && !request.replace {
                ResourceAction::Unknown
            } else if *kind == ManagedPathKind::Directory && *metadata_matches {
                ResourceAction::Noop
            } else {
                ResourceAction::Update
            },
        )),
    }
}

fn inspect_path(request: PrivilegedPathInspection) -> Result<PathInspection> {
    let path = validate_privileged_target(&request.path)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PathInspection::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    let kind = ManagedPathKind::from_metadata(&metadata);
    let content_matches = match (request.expected_content, kind) {
        (Some(expected), ManagedPathKind::File) => Some(fs::read(&path)? == expected.as_bytes()),
        _ => None,
    };
    let metadata_matches = !request.check_metadata
        || metadata_matches(
            &metadata,
            request.mode,
            request.owner.as_deref(),
            request.group.as_deref(),
        )?;
    Ok(PathInspection::Present {
        kind,
        current: describe_metadata_value(&metadata),
        metadata_matches,
        content_matches,
    })
}

fn is_permission_denied(error: &eyre::Report) -> bool {
    error.chain().any(|error| {
        let io_permission_denied = error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied);
        #[cfg(unix)]
        let platform_permission_denied =
            error
                .downcast_ref::<nix::errno::Errno>()
                .is_some_and(|error| {
                    matches!(error, nix::errno::Errno::EACCES | nix::errno::Errno::EPERM)
                });
        #[cfg(not(unix))]
        let platform_permission_denied = false;
        io_permission_denied || platform_permission_denied
    })
}

impl ManagedPathKind {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        if metadata.file_type().is_file() {
            Self::File
        } else if metadata.file_type().is_dir() {
            Self::Directory
        } else if metadata.file_type().is_symlink() {
            Self::Symlink
        } else {
            Self::Other
        }
    }
}

fn absolute_target(path: &str) -> Result<PathBuf> {
    let path = crate::file::replace_path(Path::new(path));
    validate_privileged_target(&path)
}

fn validate_privileged_target(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("managed system path must be absolute: {}", path.display());
    }
    let path = path.absolutize()?.to_path_buf();
    if path == Path::new("/") {
        bail!("refusing to manage the filesystem root");
    }
    Ok(path)
}

fn parse_mode(mode: Option<&str>, default: u32) -> Result<u32> {
    let Some(mode) = mode else {
        return Ok(default);
    };
    let mode = mode.strip_prefix("0o").unwrap_or(mode);
    let parsed = u32::from_str_radix(mode, 8).wrap_err("mode must be an octal string")?;
    if parsed > 0o7777 {
        bail!("mode must be between 0000 and 7777");
    }
    Ok(parsed)
}

fn nonempty(field: &str, value: Option<String>) -> Result<Option<String>> {
    match value {
        Some(value) if value.trim().is_empty() => bail!("{field} must not be empty"),
        Some(value) => Ok(Some(value)),
        None => Ok(None),
    }
}

fn desired_metadata(kind: &str, mode: u32, owner: Option<&str>, group: Option<&str>) -> String {
    let mut desired = format!("{kind} mode {mode:04o}");
    if let Some(owner) = owner {
        desired.push_str(&format!(" owner {owner}"));
    }
    if let Some(group) = group {
        desired.push_str(&format!(" group {group}"));
    }
    desired
}

#[cfg(unix)]
fn metadata_matches(
    metadata: &fs::Metadata,
    mode: u32,
    owner: Option<&str>,
    group: Option<&str>,
) -> Result<bool> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let owner_matches = match owner {
        Some(owner) => lookup_user(owner)?.is_some_and(|uid| metadata.uid() == uid),
        None => true,
    };
    let group_matches = match group {
        Some(group) => lookup_group(group)?.is_some_and(|gid| metadata.gid() == gid),
        None => true,
    };
    Ok(metadata.permissions().mode() & 0o7777 == mode && owner_matches && group_matches)
}

#[cfg(not(unix))]
fn metadata_matches(
    _metadata: &fs::Metadata,
    _mode: u32,
    _owner: Option<&str>,
    _group: Option<&str>,
) -> Result<bool> {
    bail!("managed system files are only supported on Unix")
}

#[cfg(unix)]
fn describe_metadata_value(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    format!(
        "{} mode {:04o} uid {} gid {}",
        describe_file_type(metadata),
        metadata.permissions().mode() & 0o7777,
        metadata.uid(),
        metadata.gid(),
    )
}

#[cfg(not(unix))]
fn describe_metadata_value(metadata: &fs::Metadata) -> String {
    describe_file_type(metadata)
}

fn describe_file_type(metadata: &fs::Metadata) -> String {
    if metadata.file_type().is_file() {
        "file"
    } else if metadata.file_type().is_dir() {
        "directory"
    } else if metadata.file_type().is_symlink() {
        "symlink"
    } else {
        "other"
    }
    .to_string()
}

#[cfg(unix)]
fn resolve_user(name: &str) -> Result<u32> {
    lookup_user(name)?.ok_or_else(|| eyre!("user '{name}' does not exist"))
}

#[cfg(unix)]
fn resolve_group(name: &str) -> Result<u32> {
    lookup_group(name)?.ok_or_else(|| eyre!("group '{name}' does not exist"))
}

#[cfg(unix)]
fn lookup_user(name: &str) -> Result<Option<u32>> {
    Ok(nix::unistd::User::from_name(name)?.map(|user| user.uid.as_raw()))
}

#[cfg(unix)]
fn lookup_group(name: &str) -> Result<Option<u32>> {
    Ok(nix::unistd::Group::from_name(name)?.map(|group| group.gid.as_raw()))
}

#[cfg(unix)]
fn set_metadata(path: &Path, owner: Option<&str>, group: Option<&str>, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let uid = owner
        .map(resolve_user)
        .transpose()?
        .map(nix::unistd::Uid::from_raw);
    let gid = group
        .map(resolve_group)
        .transpose()?
        .map(nix::unistd::Gid::from_raw);
    nix::unistd::chown(path, uid, gid)?;
    // chown may clear setuid/setgid bits, so apply the requested mode last.
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_metadata(
    _path: &Path,
    _owner: Option<&str>,
    _group: Option<&str>,
    _mode: u32,
) -> Result<()> {
    bail!("managed system files are only supported on Unix")
}

fn write_file(
    path: &Path,
    content: &[u8],
    owner: Option<&str>,
    group: Option<&str>,
    mode: u32,
    replace: bool,
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("managed file has no parent: {}", path.display()))?;
    match fs::metadata(parent) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => bail!(
            "managed file parent is not a directory: {}",
            parent.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("managed file parent does not exist: {}", parent.display())
        }
        Err(error) => return Err(error.into()),
    }
    // Prepare the complete replacement before mutating the destination. In
    // particular, a metadata permission error must leave the old path intact.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(content)?;
    set_metadata(temporary.path(), owner, group, mode)?;
    temporary.as_file_mut().sync_all()?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) if !replace => {
            bail!("refusing to replace non-file path: {}", path.display())
        }
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir(path).wrap_err_with(|| {
                format!(
                    "refusing to replace non-empty directory with file: {}",
                    path.display()
                )
            })?
        }
        Ok(_) => fs::remove_file(path)?,
    }
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .wrap_err_with(|| format!("failed to atomically replace {}", path.display()))?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn remove_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
        Ok(metadata) if metadata.file_type().is_dir() => {
            bail!("refusing to remove directory as a file: {}", path.display())
        }
        Ok(_) => fs::remove_file(path).map_err(Into::into),
    }
}

fn create_directory(
    path: &Path,
    owner: Option<&str>,
    group: Option<&str>,
    mode: u32,
    replace: bool,
) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)?,
        Err(error) => return Err(error.into()),
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) if !replace => {
            bail!("refusing to replace non-directory path: {}", path.display())
        }
        Ok(_) => {
            fs::remove_file(path)?;
            fs::create_dir(path)?;
        }
    }
    set_metadata(path, owner, group, mode)
}

fn remove_directory(path: &Path, recursive: bool) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            bail!("refusing to remove non-directory path: {}", path.display())
        }
        Ok(_) if recursive => fs::remove_dir_all(path).map_err(Into::into),
        Ok(_) => fs::remove_dir(path).map_err(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, state: ManagedState) -> ManagedFileRequest {
        ManagedFileRequest {
            path: PathBuf::from(path),
            content: (state == ManagedState::Present).then(|| "content".to_string()),
            owner: None,
            group: None,
            mode: 0o644,
            state,
            replace: false,
            notify: vec![],
            inspection: None,
        }
    }

    fn directory(path: &str, state: ManagedState) -> ManagedDirectoryRequest {
        ManagedDirectoryRequest {
            path: PathBuf::from(path),
            owner: None,
            group: None,
            mode: 0o755,
            state,
            recursive: false,
            replace: false,
            notify: vec![],
            inspection: None,
        }
    }

    #[test]
    fn rejects_relative_and_root_targets() {
        assert!(absolute_target("etc/example").is_err());
        assert!(absolute_target("/").is_err());
        assert!(absolute_target("/tmp/..").is_err());
        assert!(absolute_target("/tmp/../..").is_err());
    }

    #[test]
    fn parses_octal_modes() {
        assert_eq!(parse_mode(Some("0600"), 0).unwrap(), 0o600);
        assert_eq!(parse_mode(Some("0o1750"), 0).unwrap(), 0o1750);
        assert!(parse_mode(Some("888"), 0).is_err());
    }

    #[test]
    fn detects_permission_errors_through_context() {
        let io_error = Err::<(), _>(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            .wrap_err("wrapped permission error")
            .unwrap_err();
        assert!(is_permission_denied(&io_error));

        #[cfg(unix)]
        {
            let nix_error: eyre::Report = nix::errno::Errno::EPERM.into();
            assert!(is_permission_denied(&nix_error));
        }
        let other_error: eyre::Report = std::io::Error::from(std::io::ErrorKind::NotFound).into();
        assert!(!is_permission_denied(&other_error));
    }

    #[test]
    fn elevates_before_destructive_composite_actions() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("file");
        fs::write(&file_path, "content").unwrap();
        let directory_path = temp.path().join("directory");
        fs::create_dir(&directory_path).unwrap();

        let replace_file_with_directory = PrivilegedAction::CreateDirectory {
            path: file_path,
            owner: None,
            group: None,
            mode: 0o755,
            replace: true,
        };
        assert!(
            replace_file_with_directory
                .requires_preemptive_elevation()
                .unwrap()
        );

        let replace_directory_with_file = PrivilegedAction::WriteFile {
            path: directory_path,
            content: "content".to_string(),
            owner: None,
            group: None,
            mode: 0o644,
            replace: true,
        };
        assert!(
            replace_directory_with_file
                .requires_preemptive_elevation()
                .unwrap()
        );

        let recursive_removal = PrivilegedAction::RemoveDirectory {
            path: temp.path().join("tree"),
            recursive: true,
        };
        assert!(recursive_removal.requires_preemptive_elevation().unwrap());

        let ordinary_write = PrivilegedAction::WriteFile {
            path: temp.path().join("ordinary"),
            content: "content".to_string(),
            owner: None,
            group: None,
            mode: 0o644,
            replace: false,
        };
        assert!(!ordinary_write.requires_preemptive_elevation().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn permission_failure_preserves_remaining_action_order() {
        use std::os::unix::fs::PermissionsExt;

        // Root can write through the mode restriction used to induce EACCES.
        if nix::unistd::geteuid().is_root() {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let blocked_parent = temp.path().join("blocked");
        let blocked = blocked_parent.join("second");
        let remaining = temp.path().join("third");
        fs::create_dir(&blocked_parent).unwrap();
        fs::set_permissions(&blocked_parent, fs::Permissions::from_mode(0o555)).unwrap();

        let write = |path: PathBuf| PrivilegedAction::WriteFile {
            path,
            content: "content".to_string(),
            owner: None,
            group: None,
            mode: 0o644,
            replace: false,
        };
        let pending = PrivilegedPlan {
            actions: vec![
                write(first.clone()),
                write(blocked.clone()),
                write(remaining.clone()),
            ],
        }
        .apply_until_elevation_required()
        .unwrap();

        // Let TempDir clean up even if an assertion below fails.
        fs::set_permissions(&blocked_parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(first.is_file());
        assert!(!blocked.exists());
        assert!(!remaining.exists());
        assert_eq!(pending.actions.len(), 2);
        assert!(matches!(
            &pending.actions[0],
            PrivilegedAction::WriteFile { path, .. } if path == &blocked
        ));
        assert!(matches!(
            &pending.actions[1],
            PrivilegedAction::WriteFile { path, .. } if path == &remaining
        ));
    }

    #[cfg(unix)]
    #[test]
    fn atomically_writes_and_updates_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config");
        write_file(&path, b"first", None, None, 0o640, false).unwrap();
        write_file(&path, b"second", None, None, 0o600, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn recursive_removal_must_be_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("directory");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("child"), "content").unwrap();
        assert!(remove_directory(&directory, false).is_err());
        remove_directory(&directory, true).unwrap();
        assert!(!directory.exists());
    }

    #[test]
    fn type_replacement_must_be_explicit() {
        let mut present_file = file("/opt/example", ManagedState::Present);
        present_file.inspection = Some(PathInspection::Present {
            kind: ManagedPathKind::Directory,
            current: "directory".to_string(),
            metadata_matches: false,
            content_matches: None,
        });
        assert_eq!(present_file.plan().unwrap().action, ResourceAction::Unknown);
        present_file.replace = true;
        assert_eq!(present_file.plan().unwrap().action, ResourceAction::Update);

        let mut present_directory = directory("/opt/example", ManagedState::Present);
        present_directory.inspection = Some(PathInspection::Present {
            kind: ManagedPathKind::File,
            current: "file".to_string(),
            metadata_matches: false,
            content_matches: None,
        });
        assert_eq!(
            present_directory.plan().unwrap().action,
            ResourceAction::Unknown
        );
        present_directory.replace = true;
        assert_eq!(
            present_directory.plan().unwrap().action,
            ResourceAction::Update
        );

        let mut absent_file = file("/opt/example", ManagedState::Absent);
        absent_file.inspection = Some(PathInspection::Present {
            kind: ManagedPathKind::Directory,
            current: "directory".to_string(),
            metadata_matches: true,
            content_matches: None,
        });
        assert_eq!(absent_file.plan().unwrap().action, ResourceAction::Unknown);
        assert!(absent_file.operation().is_err());

        let mut absent_directory = directory("/opt/example", ManagedState::Absent);
        absent_directory.inspection = Some(PathInspection::Present {
            kind: ManagedPathKind::File,
            current: "file".to_string(),
            metadata_matches: true,
            content_matches: None,
        });
        assert_eq!(
            absent_directory.plan().unwrap().action,
            ResourceAction::Unknown
        );
        assert!(absent_directory.operation().is_err());
    }

    #[test]
    fn only_actionable_file_changes_notify_services() {
        let mut changed = file("/opt/changed", ManagedState::Present);
        changed.notify.push("example".to_string());
        changed.inspection = Some(PathInspection::Missing);

        let mut unsafe_change = file("/opt/unsafe", ManagedState::Present);
        unsafe_change.notify.push("ignored".to_string());
        unsafe_change.inspection = Some(PathInspection::Present {
            kind: ManagedPathKind::Directory,
            current: "directory".to_string(),
            metadata_matches: false,
            content_matches: None,
        });

        let notifications = pending_notifications(&[changed, unsafe_change], &[]).unwrap();
        assert!(notifications.contains("example"));
        assert!(!notifications.contains("ignored"));
    }

    #[cfg(unix)]
    #[test]
    fn replaces_wrong_types_without_creating_undeclared_parents() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("file");
        fs::create_dir(&file_path).unwrap();
        assert!(write_file(&file_path, b"content", None, None, 0o600, false).is_err());
        write_file(&file_path, b"content", None, None, 0o600, true).unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "content");

        let directory_path = temp.path().join("directory");
        fs::write(&directory_path, "content").unwrap();
        assert!(create_directory(&directory_path, None, None, 0o700, false).is_err());
        create_directory(&directory_path, None, None, 0o700, true).unwrap();
        assert!(directory_path.is_dir());

        let undeclared_parent = temp.path().join("undeclared");
        let nested = undeclared_parent.join("nested");
        assert!(create_directory(&nested, None, None, 0o755, false).is_err());
        assert!(!undeclared_parent.exists());
    }

    #[test]
    fn rejects_a_path_declared_as_both_file_and_directory() {
        assert!(
            validate_requests(
                &[file("/opt/example", ManagedState::Present)],
                &[directory("/opt/example", ManagedState::Present)],
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_present_resources_below_an_absent_managed_ancestor() {
        assert!(
            validate_requests(
                &[file("/opt/example/nested/config", ManagedState::Present)],
                &[directory("/opt/example", ManagedState::Absent)],
            )
            .is_err()
        );
        assert!(
            validate_requests(
                &[],
                &[
                    directory("/opt/example", ManagedState::Absent),
                    directory("/opt/example/nested", ManagedState::Present),
                ],
            )
            .is_err()
        );
    }

    #[test]
    fn clears_only_ignored_account_principals() {
        let mut files = vec![file("/opt/example/config", ManagedState::Present)];
        files[0].owner = Some("service-user".to_string());
        files[0].group = Some("local-group".to_string());
        let mut directories = vec![directory("/opt/example", ManagedState::Present)];
        directories[0].owner = Some("local-user".to_string());
        directories[0].group = Some("service-group".to_string());

        clear_ignored_principals(
            &mut files,
            &mut directories,
            &std::collections::HashSet::from(["service-user".to_string()]),
            &std::collections::HashSet::from(["service-group".to_string()]),
        );

        assert_eq!(files[0].owner, None);
        assert_eq!(files[0].group.as_deref(), Some("local-group"));
        assert_eq!(directories[0].owner.as_deref(), Some("local-user"));
        assert_eq!(directories[0].group, None);
    }
}
