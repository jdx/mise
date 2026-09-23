use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "linux"))]
use std::collections::HashSet;

use eyre::{Result, WrapErr, bail, eyre};
use indexmap::IndexMap;
use path_absolutize::Absolutize;
use serde::{Deserialize, Serialize};

use crate::config::{Config, ConfigMap};
use crate::system::resources::{ResourceAction, ResourceId, ResourceOrigin, ResourcePlan};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct ManagedFileTomlConfig {
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
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct ManagedDirectoryTomlConfig {
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
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ManagedFilePhase {
    PrePackages,
    #[default]
    PostPackages,
}

impl ManagedFilePhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PrePackages => "pre-packages",
            Self::PostPackages => "post-packages",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ManagedState {
    #[default]
    Present,
    Absent,
}

#[derive(Clone, Debug)]
pub(crate) struct ManagedFileRequest {
    pub phase: ManagedFilePhase,
    pub path: PathBuf,
    pub content: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    /// `None` only for a metadata-only file that leaves its mode unmanaged.
    pub mode: Option<u32>,
    pub state: ManagedState,
    pub replace: bool,
    pub notify: Vec<String>,
    pub origin: ResourceOrigin,
    /// A `remove_empty` template rendered to whitespace only, so the declared
    /// present file is removed instead.
    rendered_empty: bool,
    inspection: Option<PathInspection>,
}

#[derive(Clone, Debug)]
pub(crate) struct ManagedDirectoryRequest {
    pub phase: ManagedFilePhase,
    pub path: PathBuf,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: u32,
    pub state: ManagedState,
    pub recursive: bool,
    pub replace: bool,
    pub notify: Vec<String>,
    pub origin: ResourceOrigin,
    inspection: Option<PathInspection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum PrivilegedAction {
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
    /// Change an existing regular file's ownership or mode without touching
    /// its content or replacing its inode.
    SetFileMetadata {
        path: PathBuf,
        owner: Option<String>,
        group: Option<String>,
        mode: Option<u32>,
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
pub(crate) struct PrivilegedPlan {
    pub actions: Vec<PrivilegedAction>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ApplyReport {
    pub notified_services: super::services::ServiceNotifications,
}

pub(crate) fn pending_notifications(
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
    mode: Option<u32>,
    check_metadata: bool,
    /// Whether to report if reaching the path crosses a symlink the
    /// root-only component walk refuses; see `crosses_untrusted_symlink`.
    #[serde(default)]
    check_parent_symlinks: bool,
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
        /// The owning user, which decides whether a mode change needs root.
        #[serde(default)]
        owner_uid: Option<u32>,
        #[serde(default)]
        crosses_untrusted_symlink: bool,
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

pub(crate) fn requests_from_config(
    config: &Config,
    secrets: &super::secrets::SecretValues,
) -> Result<(Vec<ManagedFileRequest>, Vec<ManagedDirectoryRequest>)> {
    let (mut files, mut directories) = prepare_requests_from_config(config, secrets)?;
    inspect_requests(&mut files, &mut directories)?;
    Ok((files, directories))
}

pub(crate) fn status_requests_from_config(
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
    for (path, (file, base, origin)) in merged_files_from_config(config)? {
        let state = file.state;
        let phase = file.phase;
        match ManagedFileRequest::from_toml(
            config,
            path.clone(),
            file,
            &base,
            origin.clone(),
            secrets,
        ) {
            Ok(file) => files.push(file),
            Err(error) if super::secrets::is_unavailable(&error) => {
                if directory_states.contains_key(path.as_path()) {
                    bail!(
                        "managed system path '{}' is declared as both a file and a directory",
                        path.display()
                    );
                }
                validate_present_ancestors(&path, state, &directory_states)?;
                unavailable.push(
                    ResourcePlan::new(
                        ResourceId::new("file", path.to_string_lossy().into_owned()),
                        "not inspected: required secret unavailable",
                        "template rendered",
                        ResourceAction::Unknown,
                    )
                    .with_origin(origin)
                    .with_file_phase(phase),
                );
            }
            Err(error) => return Err(error),
        }
    }
    ignore_non_linux_account_principals(config, &mut files, &mut directories);
    validate_requests(&files, &directories)?;
    inspect_paths(&mut files, &mut directories)?;
    Ok((files, directories, unavailable))
}

pub(crate) fn prepare_requests_from_config(
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

pub(crate) fn inspect_requests(
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
        .map(|(path, (file, base, origin))| {
            ManagedFileRequest::from_toml(config, path, file, &base, origin, secrets)
        })
        .collect()
}

fn merged_files_from_config(
    config: &Config,
) -> Result<IndexMap<PathBuf, (ManagedFileTomlConfig, PathBuf, ResourceOrigin)>> {
    let mut composed: IndexMap<PathBuf, (ManagedFileTomlConfig, PathBuf, ResourceOrigin)> =
        IndexMap::new();
    for config_files in config.bootstrap_config_maps() {
        for (path, declaration) in merged_files_from_config_files(config_files)? {
            if let Some(existing) = composed.get(&path) {
                if managed_file_declarations_match(existing, &declaration) {
                    continue;
                }
                bail!(
                    "conflicting managed file declarations for {}\n\n  first:\n    {}\n\n  second:\n    {}",
                    path.display(),
                    existing.2.conflict_description(),
                    declaration.2.conflict_description(),
                );
            }
            composed.insert(path, declaration);
        }
    }
    Ok(composed)
}

/// Returns whether sibling declarations produce the same managed file.
fn managed_file_declarations_match(
    first: &(ManagedFileTomlConfig, PathBuf, ResourceOrigin),
    second: &(ManagedFileTomlConfig, PathBuf, ResourceOrigin),
) -> bool {
    first.0 == second.0
        && first.2.source == second.2.source
        && (!first.0.template || first.1 == second.1)
}

/// Merges managed files within one config hierarchy using normal precedence.
fn merged_files_from_config_files(
    config_files: &ConfigMap,
) -> Result<IndexMap<PathBuf, (ManagedFileTomlConfig, PathBuf, ResourceOrigin)>> {
    let mut merged = IndexMap::new();
    // Config files are ordered from highest to lowest precedence. Preserve the
    // first declaration of a target so a parent or global layer cannot replace
    // the nearer project declaration.
    for cf in config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            let mut layer_paths = IndexMap::new();
            let base = cf
                .get_path()
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            let origin = ResourceOrigin {
                config: cf.get_path().to_path_buf(),
                config_root: cf.config_root(),
                environment: crate::config::environments_for_config_path(cf.get_path()),
                source: None,
            };
            for (path, file) in bootstrap.files {
                let target = absolute_target(&path)?;
                if let Some(previous) = layer_paths.insert(target.clone(), path.clone()) {
                    bail!(
                        "managed file paths '{previous}' and '{path}' normalize to the same target '{}'",
                        target.display()
                    );
                }
                let mut file_origin = origin.clone();
                file_origin.source = file
                    .source
                    .as_deref()
                    .map(|source| resolve_source_path(&base, source));
                merged
                    .entry(target)
                    .or_insert_with(|| (file, base.clone(), file_origin));
            }
        }
    }
    Ok(merged)
}

fn directories_from_config(config: &Config) -> Result<Vec<ManagedDirectoryRequest>> {
    let mut composed: IndexMap<PathBuf, (ManagedDirectoryTomlConfig, ResourceOrigin)> =
        IndexMap::new();
    for config_files in config.bootstrap_config_maps() {
        for (path, declaration) in directories_from_config_files(config_files)? {
            if let Some(existing) = composed.get(&path) {
                if existing.0 == declaration.0 {
                    continue;
                }
                bail!(
                    "conflicting managed directory declarations for {}\n\n  first:\n    {}\n\n  second:\n    {}",
                    path.display(),
                    existing.1.conflict_description(),
                    declaration.1.conflict_description(),
                );
            }
            composed.insert(path, declaration);
        }
    }
    composed
        .into_iter()
        .map(|(path, (config, origin))| ManagedDirectoryRequest::from_toml(path, config, origin))
        .collect()
}

/// Merges managed directories within one config hierarchy using normal precedence.
fn directories_from_config_files(
    config_files: &ConfigMap,
) -> Result<IndexMap<PathBuf, (ManagedDirectoryTomlConfig, ResourceOrigin)>> {
    let mut merged = IndexMap::new();
    for cf in config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            let origin = ResourceOrigin {
                config: cf.get_path().to_path_buf(),
                config_root: cf.config_root(),
                environment: crate::config::environments_for_config_path(cf.get_path()),
                source: None,
            };
            let mut layer_paths = IndexMap::new();
            for (path, directory) in bootstrap.directories {
                let target = absolute_target(&path)?;
                if let Some(previous) = layer_paths.insert(target.clone(), path.clone()) {
                    bail!(
                        "managed directory paths '{previous}' and '{path}' normalize to the same target '{}'",
                        target.display()
                    );
                }
                merged
                    .entry(target)
                    .or_insert_with(|| (directory, origin.clone()));
            }
        }
    }
    Ok(merged)
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
        validate_present_ancestors(&file.path, file.declared_state(), &directory_states)?;
    }
    for directory in directories {
        validate_present_ancestors(&directory.path, directory.state, &directory_states)?;
    }
    for (path, state, phase) in files
        .iter()
        .map(|file| (&file.path, file.declared_state(), file.phase))
        .chain(
            directories
                .iter()
                .map(|dir| (&dir.path, dir.state, dir.phase)),
        )
    {
        for parent in directories
            .iter()
            .filter(|dir| path != &dir.path && path.starts_with(&dir.path))
        {
            let invalid = match (state, parent.state) {
                (ManagedState::Present, ManagedState::Present) => {
                    phase == ManagedFilePhase::PrePackages
                        && parent.phase == ManagedFilePhase::PostPackages
                }
                (ManagedState::Absent, ManagedState::Absent) => {
                    phase == ManagedFilePhase::PostPackages
                        && parent.phase == ManagedFilePhase::PrePackages
                }
                _ => false,
            };
            if invalid {
                bail!(
                    "managed path '{}' ({}) conflicts with ancestor '{}' ({}): parent directories must be created before children and removed after children; adjust their phase declarations",
                    path.display(),
                    phase.as_str(),
                    parent.path.display(),
                    parent.phase.as_str()
                );
            }
        }
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
        origin: ResourceOrigin,
        secrets: &super::secrets::SecretValues,
    ) -> Result<Self> {
        let metadata_only = config.state == ManagedState::Present
            && config.source.is_none()
            && config.content.is_none();
        if metadata_only {
            validate_metadata_only(&path, &config)?;
        }
        let owner = nonempty("owner", config.owner)?;
        let group = nonempty("group", config.group)?;
        let mode = if metadata_only {
            config
                .mode
                .as_deref()
                .map(parse_explicit_mode)
                .transpose()?
        } else {
            Some(parse_mode(config.mode.as_deref(), DEFAULT_FILE_MODE)?)
        };
        if config.remove_empty && !config.template {
            bail!(
                "[bootstrap.files].\"{}\": remove_empty requires template = true",
                path.display()
            );
        }
        if config.remove_empty && config.state == ManagedState::Absent {
            bail!(
                "[bootstrap.files].\"{}\": remove_empty applies only to present files",
                path.display()
            );
        }
        let mut content = match (config.source, config.content, config.state) {
            (Some(_), Some(_), _) => {
                bail!(
                    "[bootstrap.files].\"{}\": source and content are mutually exclusive",
                    path.display()
                )
            }
            (Some(source), None, ManagedState::Present) => {
                let source = resolve_source_path(base, &source);
                Some(fs::read_to_string(&source).wrap_err_with(|| {
                    format!(
                        "[bootstrap.files].\"{}\": failed to read source {}",
                        path.display(),
                        source.display()
                    )
                })?)
            }
            (None, Some(content), ManagedState::Present) => Some(content),
            (None, None, _) => None,
            (_, _, ManagedState::Absent) => bail!(
                "[bootstrap.files].\"{}\": absent files must not declare source or content",
                path.display()
            ),
        };
        if config.template {
            let rendered = content
                .as_deref()
                .map(|content| secrets.render(root_config, content, base, &path, &origin.config))
                .transpose()
                .wrap_err_with(|| {
                    format!(
                        "[bootstrap.files].\"{}\": failed to render template",
                        path.display()
                    )
                })?;
            content = rendered;
        }
        let rendered_empty = config.remove_empty
            && content
                .as_deref()
                .is_some_and(|content| content.trim().is_empty());
        let state = if rendered_empty {
            content = None;
            ManagedState::Absent
        } else {
            config.state
        };
        Ok(Self {
            path,
            content,
            phase: config.phase,
            owner,
            group,
            mode,
            state,
            replace: config.replace,
            notify: config.notify,
            origin,
            rendered_empty,
            inspection: None,
        })
    }

    /// Whether this entry manages only an existing file's ownership or mode,
    /// leaving its content, and whether it exists at all, to something else.
    pub(crate) fn is_metadata_only(&self) -> bool {
        self.state == ManagedState::Present && self.content.is_none()
    }

    /// Why apply leaves this metadata-only target alone, if it does. Status,
    /// dry-run, and apply all use this, so they agree on what apply will do.
    fn metadata_only_refusal(&self) -> Option<&'static str> {
        if !self.is_metadata_only() {
            return None;
        }
        match self.inspection.as_ref()? {
            PathInspection::Present { kind, .. } if *kind != ManagedPathKind::File => {
                Some("only existing regular files are managed without source or content")
            }
            PathInspection::Present {
                metadata_matches: false,
                crosses_untrusted_symlink: true,
                owner_uid,
                ..
            } if self.runs_elevated(*owner_uid) => Some(
                "the change needs root, and its path crosses a symlink root does not follow in a directory another user can write; declare the resolved path instead",
            ),
            _ => None,
        }
    }

    /// Whether the change is made by root: declared ownership always is, and
    /// only a file's owner can change its mode without root.
    fn runs_elevated(&self, owner_uid: Option<u32>) -> bool {
        self.owner.is_some()
            || self.group.is_some()
            || current_uid().is_none_or(|uid| uid == 0 || owner_uid != Some(uid))
    }

    /// A metadata-only target that does not exist is skipped, not created:
    /// mise has no content to create it with.
    fn is_missing_metadata_only_target(&self) -> bool {
        self.is_metadata_only() && matches!(self.inspection, Some(PathInspection::Missing))
    }

    /// Whether this file is being removed because its template rendered empty.
    pub(crate) fn rendered_empty(&self) -> bool {
        self.rendered_empty
    }

    /// The state the configuration declares, before an empty render turns a
    /// present file into a removal. Structural rules such as parent ordering
    /// hold for the declaration, so they do not depend on what renders.
    fn declared_state(&self) -> ManagedState {
        if self.rendered_empty {
            ManagedState::Present
        } else {
            self.state
        }
    }

    pub(crate) fn plan(&self) -> Result<ResourcePlan> {
        plan_file(self).map(|plan| {
            plan.with_origin(self.origin.clone())
                .with_file_phase(self.phase)
        })
    }

    fn operation(&self) -> Result<Option<PrivilegedAction>> {
        match self.plan()?.action {
            ResourceAction::Noop => return Ok(None),
            ResourceAction::Unknown if self.state == ManagedState::Absent => bail!(
                "refusing to remove directory {} as a file; declare it in [bootstrap.directories]",
                self.path.display()
            ),
            ResourceAction::Unknown if self.is_metadata_only() => bail!(
                "refusing to set permissions on {}: {}",
                self.path.display(),
                self.metadata_only_refusal()
                    .unwrap_or("its type or path cannot be managed")
            ),
            ResourceAction::Unknown => bail!(
                "refusing to replace non-file path {}; set replace = true to allow replacement",
                self.path.display()
            ),
            _ => {}
        }
        Ok(Some(match (self.state, &self.content) {
            (ManagedState::Present, Some(content)) => PrivilegedAction::WriteFile {
                path: self.path.clone(),
                content: content.clone(),
                owner: self.owner.clone(),
                group: self.group.clone(),
                mode: self.mode.unwrap_or(DEFAULT_FILE_MODE),
                replace: self.replace,
            },
            (ManagedState::Present, None) => PrivilegedAction::SetFileMetadata {
                path: self.path.clone(),
                owner: self.owner.clone(),
                group: self.group.clone(),
                mode: self.mode,
            },
            (ManagedState::Absent, _) => PrivilegedAction::RemoveFile {
                path: self.path.clone(),
            },
        }))
    }
}

const DEFAULT_FILE_MODE: u32 = 0o644;

/// A present entry without `source` or `content` manages only the metadata it
/// declares, so it must declare some, and must not ask for anything that
/// would write or replace the file.
fn validate_metadata_only(path: &Path, config: &ManagedFileTomlConfig) -> Result<()> {
    if config.mode.is_none() && config.owner.is_none() && config.group.is_none() {
        bail!(
            "[bootstrap.files].\"{}\": present files require source, content, or at least one of mode, owner, or group",
            path.display()
        );
    }
    // remove_empty also requires template, but say what is actually missing.
    for (enabled, key) in [
        (config.template, "template"),
        (config.remove_empty, "remove_empty"),
    ] {
        if enabled {
            bail!(
                "[bootstrap.files].\"{}\": {key} requires source or content",
                path.display()
            );
        }
    }
    if config.replace {
        bail!(
            "[bootstrap.files].\"{}\": replace requires source or content; a file whose content mise does not manage is never replaced",
            path.display()
        );
    }
    Ok(())
}

fn resolve_source_path(base: &Path, source: &str) -> PathBuf {
    let source = if source.starts_with("~/") {
        crate::file::replace_path(source)
    } else {
        PathBuf::from(source)
    };
    if source.is_absolute() {
        source
    } else {
        base.join(source)
    }
}

impl ManagedDirectoryRequest {
    fn from_toml(
        path: PathBuf,
        config: ManagedDirectoryTomlConfig,
        origin: ResourceOrigin,
    ) -> Result<Self> {
        if config.state == ManagedState::Present && config.recursive {
            bail!(
                "[bootstrap.directories].\"{}\": recursive is only valid with state = \"absent\"",
                path.display()
            );
        }
        Ok(Self {
            path,
            phase: config.phase,
            owner: nonempty("owner", config.owner)?,
            group: nonempty("group", config.group)?,
            mode: parse_mode(config.mode.as_deref(), 0o755)?,
            state: config.state,
            recursive: config.recursive,
            replace: config.replace,
            notify: config.notify,
            origin,
            inspection: None,
        })
    }

    pub(crate) fn plan(&self) -> Result<ResourcePlan> {
        plan_directory(self).map(|plan| {
            plan.with_origin(self.origin.clone())
                .with_file_phase(self.phase)
        })
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

pub(crate) fn apply_with_accounts(
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
        if file.is_missing_metadata_only_target() {
            warn!(
                "not setting permissions on {}: it is absent; its content is not managed by mise",
                file.path.display()
            );
            continue;
        }
        let resource = file.plan()?;
        if dry_run && resource.action == ResourceAction::Unknown {
            unknown.push(resource);
            continue;
        }
        // Nothing in this entry can make such a target manageable, since it
        // never replaces its target, so it is reported and left, not an error.
        if resource.action == ResourceAction::Unknown
            && let Some(reason) = file.metadata_only_refusal()
        {
            warn!(
                "not setting permissions on {}: {reason}",
                file.path.display()
            );
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
        .is_yes()
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
pub(crate) fn validate_principals(
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
pub(crate) fn validate_principals(
    files: &[ManagedFileRequest],
    directories: &[ManagedDirectoryRequest],
    _accounts: Option<&super::accounts::AccountRequests>,
    _allow_pending_accounts: bool,
) -> Result<()> {
    if files.is_empty() && directories.is_empty() {
        return Ok(());
    }
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
            Self::SetFileMetadata { owner, group, .. } => Ok(owner.is_some() || group.is_some()),
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
            Self::SetFileMetadata { path, .. } => {
                format!("set permissions on file {}", path.display())
            }
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

pub(crate) fn apply_privileged_plan_from_stdin() -> Result<()> {
    let plan: PrivilegedPlan = serde_json::from_reader(std::io::stdin().lock())?;
    for action in plan.actions {
        action.apply()?;
    }
    Ok(())
}

pub(crate) fn inspect_privileged_files_from_stdin() -> Result<()> {
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
            Self::SetFileMetadata {
                path,
                owner,
                group,
                mode,
            } => set_file_metadata(
                &validate_privileged_target(path)?,
                owner.as_deref(),
                group.as_deref(),
                *mode,
            ),
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
        ManagedState::Present if request.is_metadata_only() => format!(
            "{} (content unmanaged)",
            desired_metadata(
                "file",
                request.mode,
                request.owner.as_deref(),
                request.group.as_deref(),
            )
        ),
        ManagedState::Present => desired_metadata(
            "file",
            request.mode,
            request.owner.as_deref(),
            request.group.as_deref(),
        ),
        ManagedState::Absent if request.rendered_empty => {
            "absent (template rendered empty)".to_string()
        }
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
        // Without content there is nothing to create the file from. Apply
        // warns and skips it; see `apply_with_accounts`.
        (ManagedState::Present, PathInspection::Missing) if request.is_metadata_only() => Ok(
            ResourcePlan::new(id, "absent", desired, ResourceAction::Noop),
        ),
        (ManagedState::Present, PathInspection::Missing) => Ok(ResourcePlan::new(
            id,
            "absent",
            desired,
            ResourceAction::Create,
        )),
        (ManagedState::Present, PathInspection::Present { current, .. })
            if request.metadata_only_refusal().is_some() =>
        {
            Ok(ResourcePlan::new(
                id,
                format!(
                    "{current} ({})",
                    request.metadata_only_refusal().unwrap_or_default()
                ),
                desired,
                ResourceAction::Unknown,
            ))
        }
        (
            ManagedState::Present,
            PathInspection::Present {
                kind,
                current,
                metadata_matches,
                content_matches,
                ..
            },
        ) => Ok(ResourcePlan::new(
            id,
            current,
            desired,
            if *kind != ManagedPathKind::File && !request.replace {
                ResourceAction::Unknown
            } else if *kind == ManagedPathKind::File
                && (request.content.is_none() || content_matches == &Some(true))
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
            // Absent and metadata-only files have no content to compare.
            expected_content: file.content.clone(),
            owner: file.owner.clone(),
            group: file.group.clone(),
            mode: file.mode,
            check_metadata: file.state == ManagedState::Present,
            check_parent_symlinks: file.is_metadata_only(),
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
            mode: Some(directory.mode),
            check_metadata: directory.state == ManagedState::Present,
            check_parent_symlinks: false,
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
            Some(request.mode),
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
    let owner_uid = metadata_uid(&metadata);
    // Only a change root will make needs the stricter walk, so skip it when
    // the current user owns the file and declares no ownership.
    let crosses_untrusted_symlink = request.check_parent_symlinks
        && kind == ManagedPathKind::File
        && !metadata_matches
        && (request.owner.is_some()
            || request.group.is_some()
            || current_uid().is_none_or(|uid| uid == 0 || owner_uid != Some(uid)))
        && crosses_untrusted_symlink(&path)?;
    Ok(PathInspection::Present {
        kind,
        current: describe_metadata_value(&metadata),
        metadata_matches,
        content_matches,
        owner_uid,
        crosses_untrusted_symlink,
    })
}

#[cfg(unix)]
fn current_uid() -> Option<u32> {
    Some(nix::unistd::geteuid().as_raw())
}

#[cfg(not(unix))]
fn current_uid() -> Option<u32> {
    None
}

#[cfg(unix)]
fn metadata_uid(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.uid())
}

#[cfg(not(unix))]
fn metadata_uid(_metadata: &fs::Metadata) -> Option<u32> {
    None
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

pub(crate) fn parse_mode(mode: Option<&str>, default: u32) -> Result<u32> {
    mode.map_or(Ok(default), parse_explicit_mode)
}

fn parse_explicit_mode(mode: &str) -> Result<u32> {
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

fn desired_metadata(
    kind: &str,
    mode: Option<u32>,
    owner: Option<&str>,
    group: Option<&str>,
) -> String {
    let mut desired = kind.to_string();
    if let Some(mode) = mode {
        desired.push_str(&format!(" mode {mode:04o}"));
    }
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
    mode: Option<u32>,
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
    let mode_matches = mode.is_none_or(|mode| metadata.permissions().mode() & 0o7777 == mode);
    Ok(mode_matches && owner_matches && group_matches)
}

#[cfg(not(unix))]
fn metadata_matches(
    _metadata: &fs::Metadata,
    _mode: Option<u32>,
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
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) => {}
        Err(error) => {
            return Err(error)
                .wrap_err_with(|| format!("failed to inspect directory {}", path.display()));
        }
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) if !replace => {
            bail!("refusing to replace non-directory path: {}", path.display())
        }
        Ok(_) => {
            fs::remove_file(path).wrap_err_with(|| {
                format!(
                    "failed to remove existing path before creating directory {}",
                    path.display()
                )
            })?;
        }
    }

    #[cfg(unix)]
    {
        let directory = open_or_create_directory_tree(path)
            .wrap_err_with(|| format!("failed to create directory {}", path.display()))?;
        set_descriptor_metadata(&directory, owner, group, Some(mode))
            .wrap_err_with(|| format!("failed to set metadata on directory {}", path.display()))
    }

    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
            .wrap_err_with(|| format!("failed to create directory {}", path.display()))?;
        set_metadata(path, owner, group, mode)
            .wrap_err_with(|| format!("failed to set metadata on directory {}", path.display()))
    }
}

/// A path component that is a symlink in a directory someone other than root
/// could have written, which the component-by-component walk refuses.
#[cfg(unix)]
#[derive(Debug)]
struct UntrustedSymlink(PathBuf);

#[cfg(unix)]
impl std::fmt::Display for UntrustedSymlink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "refusing to follow symlink {} from an untrusted parent directory",
            self.0.display()
        )
    }
}

#[cfg(unix)]
impl std::error::Error for UntrustedSymlink {}

#[cfg(unix)]
fn is_untrusted_symlink(error: &eyre::Report) -> bool {
    error
        .chain()
        .any(|error| error.downcast_ref::<UntrustedSymlink>().is_some())
}

/// Flags that open a directory only to look names up in it. Linux's `O_PATH`
/// needs search permission alone, so a directory such as a `0711` home still
/// opens. Elsewhere this falls back to `O_RDONLY`, which also needs read
/// permission; that walk normally runs as root, where it makes no difference.
#[cfg(target_os = "linux")]
fn search_directory_flags() -> nix::fcntl::OFlag {
    use nix::fcntl::OFlag;
    OFlag::O_PATH | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

#[cfg(all(unix, not(target_os = "linux")))]
fn search_directory_flags() -> nix::fcntl::OFlag {
    use nix::fcntl::OFlag;
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

/// Open an absolute directory path one component at a time without following
/// symlinks, creating missing components with process-default metadata. The
/// returned descriptor binds later metadata changes to the directory that was
/// actually opened instead of resolving the path again.
#[cfg(unix)]
fn open_or_create_directory_tree(path: &Path) -> Result<std::os::fd::OwnedFd> {
    open_or_create_directory_tree_inner(path, true, 0)
}

/// Open an existing absolute directory path the same way, without creating
/// anything: a symlink is only followed from a root-owned directory that no
/// one else can write, where only root could have placed it.
#[cfg(unix)]
fn open_directory_tree(path: &Path) -> Result<std::os::fd::OwnedFd> {
    open_or_create_directory_tree_inner(path, false, 0)
}

#[cfg(unix)]
fn open_or_create_directory_tree_inner(
    path: &Path,
    create: bool,
    followed_symlinks: usize,
) -> Result<std::os::fd::OwnedFd> {
    use nix::fcntl::{AtFlags, OFlag, open, openat};
    use nix::sys::stat::{Mode, SFlag, fstat, fstatat, mkdirat};

    if followed_symlinks > 40 {
        bail!(
            "too many symbolic links in managed directory {}",
            path.display()
        );
    }

    let components = path
        .strip_prefix(Path::new("/"))
        .wrap_err_with(|| format!("managed directory must be absolute: {}", path.display()))?
        .components()
        .map(|component| match component {
            std::path::Component::Normal(name) => Ok(name.to_os_string()),
            _ => bail!("invalid managed directory path: {}", path.display()),
        })
        .collect::<Result<Vec<_>>>()?;

    // Walking a path needs only search permission on each directory. The
    // directory being created is opened for reading because its descriptor
    // is later passed to fchown and fchmod.
    let read_flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW;
    let flags_for = |last: bool| {
        if create && last {
            read_flags
        } else {
            search_directory_flags()
        }
    };
    let mut directory = open(
        Path::new("/"),
        flags_for(components.is_empty()),
        Mode::empty(),
    )?;
    let mut current = PathBuf::from("/");
    for (index, name) in components.iter().enumerate() {
        let component_path = current.join(name);
        let flags = flags_for(index + 1 == components.len());
        directory = match openat(&directory, name.as_os_str(), flags, Mode::empty()) {
            Ok(directory) => directory,
            Err(open_error) => {
                let metadata = fstatat(&directory, name.as_os_str(), AtFlags::AT_SYMLINK_NOFOLLOW);
                if metadata.is_ok_and(|metadata| {
                    SFlag::from_bits_truncate(metadata.st_mode).contains(SFlag::S_IFLNK)
                }) {
                    let parent = fstat(&directory)?;
                    if parent.st_uid != 0 || parent.st_mode & 0o022 != 0 {
                        return Err(UntrustedSymlink(component_path).into());
                    }
                    let target = nix::fcntl::readlinkat(&directory, name.as_os_str())?;
                    let mut resolved = if Path::new(&target).is_absolute() {
                        PathBuf::from(target)
                    } else {
                        current.join(target)
                    };
                    resolved.extend(components.iter().skip(index + 1));
                    let resolved = resolved.absolutize()?.to_path_buf();
                    if resolved == Path::new("/") {
                        bail!(
                            "refusing to resolve managed directory {} to the filesystem root",
                            path.display()
                        );
                    }
                    return open_or_create_directory_tree_inner(
                        &resolved,
                        create,
                        followed_symlinks + 1,
                    );
                }
                if open_error != nix::errno::Errno::ENOENT || !create {
                    return Err(open_error).wrap_err_with(|| {
                        format!(
                            "failed to open path component {} without following symlinks",
                            component_path.display()
                        )
                    });
                }
                let created_by_us = match mkdirat(
                    &directory,
                    name.as_os_str(),
                    Mode::from_bits_truncate(0o777),
                ) {
                    Ok(()) => true,
                    Err(nix::errno::Errno::EEXIST) => false,
                    Err(error) => {
                        return Err(error).wrap_err_with(|| {
                            format!(
                                "failed to create path component {}",
                                component_path.display()
                            )
                        });
                    }
                };
                let created = openat(&directory, name.as_os_str(), flags, Mode::empty())
                    .wrap_err_with(|| {
                        format!(
                            "failed to open newly available path component {} without following symlinks",
                            component_path.display()
                        )
                    })?;
                let stat = nix::sys::stat::fstat(&created)?;
                if stat.st_uid != nix::unistd::geteuid().as_raw() {
                    if created_by_us {
                        bail!(
                            "created path component {} was replaced before it could be opened",
                            component_path.display()
                        );
                    } else {
                        bail!(
                            "path component {} was concurrently created by another user",
                            component_path.display()
                        );
                    }
                }
                created
            }
        };
        current.push(name);
    }
    Ok(directory)
}

/// Set an existing regular file's ownership and mode through a descriptor
/// opened without following a final symlink, so the change lands on the file
/// that was checked and never on a symlink's target. Content and inode are
/// left untouched.
///
/// As root, the parent directories are also walked without following
/// untrusted symlinks; see [`FileOpener`]. As any other user, a parent
/// symlink is followed like any other path lookup, since the change can only
/// reach what that user could change anyway.
#[cfg(unix)]
fn set_file_metadata(
    path: &Path,
    owner: Option<&str>,
    group: Option<&str>,
    mode: Option<u32>,
) -> Result<()> {
    use nix::fcntl::OFlag;

    let opener = FileOpener::new(path)?;
    // O_NONBLOCK keeps a FIFO swapped in after inspection from blocking the
    // open; it is refused below like any other non-regular file.
    let file = match opener.open(
        OFlag::O_RDONLY
            | OFlag::O_NOFOLLOW
            | OFlag::O_NONBLOCK
            | OFlag::O_NOCTTY
            | OFlag::O_CLOEXEC,
    ) {
        Ok(file) => file,
        // The owner of an unreadable file may still change its mode, so this
        // stays in-process, as status and dry-run assume.
        Err(nix::errno::Errno::EACCES) => {
            return set_unreadable_file_metadata(&opener, path, owner, group, mode);
        }
        Err(nix::errno::Errno::ELOOP) => bail!(
            "refusing to set permissions on symlink {}; it is never followed",
            path.display()
        ),
        Err(error) => {
            return Err(error).wrap_err_with(|| {
                format!(
                    "failed to open file {} without following symlinks",
                    path.display()
                )
            });
        }
    };
    ensure_regular_file(&file, path)?;
    set_descriptor_metadata(&file, owner, group, mode)
        .wrap_err_with(|| format!("failed to set metadata on file {}", path.display()))
}

/// Opens a file's final component without following it, and as root also
/// resolves its parent without following untrusted symlinks. `O_NOFOLLOW`
/// only guards the final component, so a user who can write an ancestor
/// could otherwise swap it for a symlink and redirect a privileged change to
/// a file such as `/etc/shadow`.
#[cfg(unix)]
enum FileOpener<'a> {
    /// Relative to a parent opened component by component.
    Parent(std::os::fd::OwnedFd, &'a std::ffi::OsStr),
    /// By path, following parent symlinks as an ordinary lookup does.
    Path(&'a Path),
}

#[cfg(unix)]
impl<'a> FileOpener<'a> {
    fn new(path: &'a Path) -> Result<Self> {
        if runs_as_root() {
            Self::strict(path)
        } else {
            Ok(Self::Path(path))
        }
    }

    fn strict(path: &'a Path) -> Result<Self> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            bail!("managed file has no parent: {}", path.display());
        };
        let parent = open_directory_tree(parent).wrap_err_with(|| {
            format!(
                "failed to open the parent of {} without following symlinks",
                path.display()
            )
        })?;
        Ok(Self::Parent(parent, name))
    }

    fn open(&self, flags: nix::fcntl::OFlag) -> nix::Result<std::os::fd::OwnedFd> {
        let mode = nix::sys::stat::Mode::empty();
        match self {
            Self::Parent(parent, name) => nix::fcntl::openat(parent, *name, flags, mode),
            Self::Path(path) => nix::fcntl::open(*path, flags, mode),
        }
    }
}

#[cfg(unix)]
fn runs_as_root() -> bool {
    nix::unistd::geteuid().is_root()
}

/// Whether reaching `path` crosses a symlink that the component-by-component
/// walk refuses when running as root.
#[cfg(unix)]
fn crosses_untrusted_symlink(path: &Path) -> Result<bool> {
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    match open_directory_tree(parent) {
        Ok(_) => Ok(false),
        Err(error) if is_untrusted_symlink(&error) => Ok(true),
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn crosses_untrusted_symlink(_path: &Path) -> Result<bool> {
    Ok(false)
}

#[cfg(unix)]
fn ensure_regular_file(file: &std::os::fd::OwnedFd, path: &Path) -> Result<()> {
    use nix::sys::stat::{SFlag, fstat};

    let stat = fstat(file)?;
    if stat.st_mode & SFlag::S_IFMT.bits() != SFlag::S_IFREG.bits() {
        bail!(
            "refusing to set permissions on non-file path: {}",
            path.display()
        );
    }
    Ok(())
}

/// Set metadata on a file the current user cannot read. Other Unix systems
/// (macOS, the BSDs) have no `O_PATH`, but implement `fchmodat` and
/// `fchownat` with `AT_SYMLINK_NOFOLLOW` directly. There they change a
/// symlink's own metadata rather than failing, so a link is refused first; one
/// swapped in after that check only has its own metadata changed, never its
/// target's. Root can open any file, so only a user-level change, which
/// follows parent symlinks anyway, gets here.
#[cfg(all(unix, not(target_os = "linux")))]
fn set_unreadable_file_metadata(
    opener: &FileOpener,
    path: &Path,
    owner: Option<&str>,
    group: Option<&str>,
    mode: Option<u32>,
) -> Result<()> {
    use nix::errno::Errno;
    use nix::fcntl::{AT_FDCWD, AtFlags};
    use nix::sys::stat::{FchmodatFlags, fchmodat};

    if !matches!(opener, FileOpener::Path(_)) {
        return Err(Errno::EACCES).wrap_err_with(|| {
            format!(
                "failed to open file {} without following symlinks",
                path.display()
            )
        });
    }
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        bail!(
            "refusing to set permissions on non-file path: {}",
            path.display()
        );
    }
    let (uid, gid) = resolve_owner_and_group(owner, group)?;
    let result = nix::unistd::fchownat(AT_FDCWD, path, uid, gid, AtFlags::AT_SYMLINK_NOFOLLOW)
        .and_then(|()| match mode {
            Some(mode) => fchmodat(
                AT_FDCWD,
                path,
                platform_mode(mode),
                FchmodatFlags::NoFollowSymlink,
            ),
            None => Ok(()),
        });
    result
        .map_err(unsupported_no_follow_as_permission_denied)
        .wrap_err_with(|| format!("failed to set metadata on file {}", path.display()))
}

/// Some Unix systems cannot change metadata without following symlinks
/// (`ENOTSUP`). Report that as a permission error so the change is retried
/// through the privileged helper, as it was before the in-process repair.
#[cfg(all(unix, any(not(target_os = "linux"), test)))]
fn unsupported_no_follow_as_permission_denied(error: nix::errno::Errno) -> nix::errno::Errno {
    use nix::errno::Errno;

    if error == Errno::ENOTSUP || error == Errno::EOPNOTSUPP {
        Errno::EACCES
    } else {
        error
    }
}

/// Set metadata on a file the current user cannot read. An `O_PATH`
/// descriptor needs no read permission and, with `O_NOFOLLOW`, refers to a
/// final symlink itself, which the regular-file check then refuses. Linux has
/// no `fchmod` for such a descriptor, so the change goes through its
/// `/proc/self/fd` entry, which resolves to the opened inode rather than to
/// the path again.
#[cfg(target_os = "linux")]
fn set_unreadable_file_metadata(
    opener: &FileOpener,
    path: &Path,
    owner: Option<&str>,
    group: Option<&str>,
    mode: Option<u32>,
) -> Result<()> {
    use nix::fcntl::OFlag;
    use std::os::fd::AsRawFd;

    let file = opener
        .open(OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC)
        .wrap_err_with(|| {
            format!(
                "failed to open file {} without following symlinks",
                path.display()
            )
        })?;
    ensure_regular_file(&file, path)?;
    let descriptor_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
    let (uid, gid) = resolve_owner_and_group(owner, group)?;
    let result = nix::unistd::chown(&descriptor_path, uid, gid)
        .map_err(eyre::Report::from)
        .and_then(|()| match mode {
            Some(mode) => nix::sys::stat::fchmodat(
                nix::fcntl::AT_FDCWD,
                &descriptor_path,
                platform_mode(mode),
                nix::sys::stat::FchmodatFlags::FollowSymlink,
            )
            .map_err(Into::into),
            None => Ok(()),
        });
    drop(file);
    result.wrap_err_with(|| format!("failed to set metadata on file {}", path.display()))
}

#[cfg(not(unix))]
fn set_file_metadata(
    _path: &Path,
    _owner: Option<&str>,
    _group: Option<&str>,
    _mode: Option<u32>,
) -> Result<()> {
    bail!("managed system files are only supported on Unix")
}

/// Change ownership, then mode, of an open file or directory. A `None` mode
/// leaves the permission bits as they are, apart from any setuid or setgid
/// bits the operating system clears on an ownership change.
#[cfg(unix)]
fn set_descriptor_metadata(
    descriptor: &std::os::fd::OwnedFd,
    owner: Option<&str>,
    group: Option<&str>,
    mode: Option<u32>,
) -> Result<()> {
    let (uid, gid) = resolve_owner_and_group(owner, group)?;
    nix::unistd::fchown(descriptor, uid, gid)?;
    // chown may clear setuid/setgid bits, so apply the requested mode last.
    if let Some(mode) = mode {
        nix::sys::stat::fchmod(descriptor, platform_mode(mode))?;
    }
    Ok(())
}

#[cfg(unix)]
fn resolve_owner_and_group(
    owner: Option<&str>,
    group: Option<&str>,
) -> Result<(Option<nix::unistd::Uid>, Option<nix::unistd::Gid>)> {
    let uid = owner
        .map(resolve_user)
        .transpose()?
        .map(nix::unistd::Uid::from_raw);
    let gid = group
        .map(resolve_group)
        .transpose()?
        .map(nix::unistd::Gid::from_raw);
    Ok((uid, gid))
}

#[cfg(unix)]
fn platform_mode(mode: u32) -> nix::sys::stat::Mode {
    [
        (0o4000, nix::sys::stat::Mode::S_ISUID),
        (0o2000, nix::sys::stat::Mode::S_ISGID),
        (0o1000, nix::sys::stat::Mode::S_ISVTX),
        (0o0400, nix::sys::stat::Mode::S_IRUSR),
        (0o0200, nix::sys::stat::Mode::S_IWUSR),
        (0o0100, nix::sys::stat::Mode::S_IXUSR),
        (0o0040, nix::sys::stat::Mode::S_IRGRP),
        (0o0020, nix::sys::stat::Mode::S_IWGRP),
        (0o0010, nix::sys::stat::Mode::S_IXGRP),
        (0o0004, nix::sys::stat::Mode::S_IROTH),
        (0o0002, nix::sys::stat::Mode::S_IWOTH),
        (0o0001, nix::sys::stat::Mode::S_IXOTH),
    ]
    .into_iter()
    .filter_map(|(bit, flag)| (mode & bit != 0).then_some(flag))
    .fold(nix::sys::stat::Mode::empty(), |mode, flag| mode | flag)
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
            phase: ManagedFilePhase::default(),
            path: PathBuf::from(path),
            content: (state == ManagedState::Present).then(|| "content".to_string()),
            owner: None,
            group: None,
            mode: Some(0o644),
            state,
            replace: false,
            notify: vec![],
            origin: ResourceOrigin {
                config: PathBuf::from("/mise.toml"),
                config_root: PathBuf::from("/"),
                environment: vec![],
                source: None,
            },
            rendered_empty: false,
            inspection: None,
        }
    }

    fn directory(path: &str, state: ManagedState) -> ManagedDirectoryRequest {
        ManagedDirectoryRequest {
            phase: ManagedFilePhase::default(),
            path: PathBuf::from(path),
            owner: None,
            group: None,
            mode: 0o755,
            state,
            recursive: false,
            replace: false,
            notify: vec![],
            origin: ResourceOrigin {
                config: PathBuf::from("/mise.toml"),
                config_root: PathBuf::from("/"),
                environment: vec![],
                source: None,
            },
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
    fn file_phases_respect_parent_creation_and_removal() {
        let mut child = file("/etc/vendor/key", ManagedState::Present);
        let mut parent = directory("/etc/vendor", ManagedState::Present);
        child.phase = ManagedFilePhase::PrePackages;
        assert!(validate_requests(&[child.clone()], &[parent.clone()]).is_err());
        parent.phase = ManagedFilePhase::PrePackages;
        assert!(validate_requests(&[child.clone()], &[parent.clone()]).is_ok());
        child.phase = ManagedFilePhase::PostPackages;
        assert!(validate_requests(&[child.clone()], &[parent.clone()]).is_ok());

        child.state = ManagedState::Absent;
        parent.state = ManagedState::Absent;
        assert!(validate_requests(&[child.clone()], &[parent.clone()]).is_err());
        child.phase = ManagedFilePhase::PrePackages;
        assert!(validate_requests(&[child.clone()], &[parent.clone()]).is_ok());
        parent.phase = ManagedFilePhase::PostPackages;
        assert!(validate_requests(&[child], &[parent]).is_ok());
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
            owner_uid: None,
            crosses_untrusted_symlink: false,
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
            owner_uid: None,
            crosses_untrusted_symlink: false,
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
            owner_uid: None,
            crosses_untrusted_symlink: false,
        });
        assert_eq!(absent_file.plan().unwrap().action, ResourceAction::Unknown);
        assert!(absent_file.operation().is_err());

        let mut absent_directory = directory("/opt/example", ManagedState::Absent);
        absent_directory.inspection = Some(PathInspection::Present {
            kind: ManagedPathKind::File,
            current: "file".to_string(),
            metadata_matches: true,
            content_matches: None,
            owner_uid: None,
            crosses_untrusted_symlink: false,
        });
        assert_eq!(
            absent_directory.plan().unwrap().action,
            ResourceAction::Unknown
        );
        assert!(absent_directory.operation().is_err());
    }

    fn metadata_only(inspection: PathInspection) -> ManagedFileRequest {
        let mut request = file("/opt/example", ManagedState::Present);
        request.content = None;
        request.mode = Some(0o600);
        request.inspection = Some(inspection);
        request
    }

    #[test]
    fn metadata_only_files_compare_only_declared_metadata() {
        let unchanged = metadata_only(PathInspection::Present {
            kind: ManagedPathKind::File,
            current: "file mode 0600".to_string(),
            metadata_matches: true,
            content_matches: None,
            owner_uid: None,
            crosses_untrusted_symlink: false,
        });
        assert_eq!(unchanged.plan().unwrap().action, ResourceAction::Noop);
        assert!(unchanged.operation().unwrap().is_none());

        let drifted = metadata_only(PathInspection::Present {
            kind: ManagedPathKind::File,
            current: "file mode 0644".to_string(),
            metadata_matches: false,
            content_matches: None,
            owner_uid: None,
            crosses_untrusted_symlink: false,
        });
        let plan = drifted.plan().unwrap();
        assert_eq!(plan.action, ResourceAction::Update);
        assert_eq!(plan.desired, "file mode 0600 (content unmanaged)");
        let Some(PrivilegedAction::SetFileMetadata { mode, .. }) = drifted.operation().unwrap()
        else {
            panic!("expected a metadata-only update");
        };
        assert_eq!(mode, Some(0o600));

        // Nothing to create the file from: skipped, never created.
        let missing = metadata_only(PathInspection::Missing);
        assert!(missing.is_missing_metadata_only_target());
        assert_eq!(missing.plan().unwrap().action, ResourceAction::Noop);
        assert!(missing.operation().unwrap().is_none());

        for kind in [
            ManagedPathKind::Symlink,
            ManagedPathKind::Directory,
            ManagedPathKind::Other,
        ] {
            let wrong_type = metadata_only(PathInspection::Present {
                kind,
                current: "symlink".to_string(),
                metadata_matches: false,
                content_matches: None,
                owner_uid: None,
                crosses_untrusted_symlink: false,
            });
            assert_eq!(wrong_type.plan().unwrap().action, ResourceAction::Unknown);
            let error = wrong_type.operation().unwrap_err().to_string();
            assert!(!error.contains("replace"), "unexpected error: {error}");
        }
    }

    #[test]
    fn metadata_only_files_require_metadata_and_reject_writes() {
        let path = Path::new("/opt/example");
        let validate = |toml: &str| {
            let config = toml::from_str::<ManagedFileTomlConfig>(toml).unwrap();
            validate_metadata_only(path, &config)
        };
        assert!(validate(r#"mode = "0600""#).is_ok());
        assert!(validate(r#"owner = "root""#).is_ok());
        assert!(validate(r#"group = "root""#).is_ok());
        assert!(validate("").is_err());
        for invalid in ["template = true", "remove_empty = true", "replace = true"] {
            let error = validate(&format!("mode = \"0600\"\n{invalid}")).unwrap_err();
            let key = invalid.split(' ').next().unwrap();
            assert!(
                error
                    .to_string()
                    .contains(&format!("{key} requires source or content")),
                "unexpected error: {error}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn sets_file_metadata_in_place() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config");
        fs::write(&path, "content").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        set_file_metadata(&path, None, None, Some(0o600)).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
        assert_eq!(metadata.ino(), inode);
        assert_eq!(fs::read_to_string(&path).unwrap(), "content");

        // The owner can change the mode of a file it cannot read, without
        // the privileged helper, on every Unix.
        if !nix::unistd::geteuid().is_root() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            set_file_metadata(&path, None, None, Some(0o600)).unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
                0o600
            );
            assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        }

        // An unmanaged mode is left alone.
        set_file_metadata(&path, None, None, None).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            0o600
        );

        // A symlink is refused rather than followed to its target.
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(set_file_metadata(&link, None, None, Some(0o644)).is_err());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            0o600
        );

        assert!(set_file_metadata(temp.path(), None, None, Some(0o700)).is_err());

        // The unreadable-file path refuses symlinks and directories too,
        // whichever way the parent was resolved.
        #[cfg(target_os = "linux")]
        {
            let unreadable = |path: &Path, strict: bool, mode| {
                let opener = if strict {
                    FileOpener::strict(path)?
                } else {
                    FileOpener::Path(path)
                };
                set_unreadable_file_metadata(&opener, path, None, None, Some(mode))
            };
            for strict in [true, false] {
                assert!(unreadable(&link, strict, 0o644).is_err());
                assert!(unreadable(temp.path(), strict, 0o700).is_err());
                assert_eq!(
                    fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
                    0o600
                );
            }
            unreadable(&path, true, 0o640).unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
                0o640
            );
        }
    }

    /// Root resolves a file's parents without following a symlink someone
    /// else could have planted; any other user follows them like a normal
    /// lookup, since the change can only reach what that user can change.
    #[cfg(unix)]
    #[test]
    fn parent_symlinks_are_only_refused_when_running_as_root() {
        use std::os::unix::fs::PermissionsExt;

        // Root-owned parents are trusted, so the refusal cannot be staged as
        // root in a temporary directory.
        if nix::unistd::geteuid().is_root() {
            return;
        }
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("config");
        fs::write(&target, "content").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let linked_parent = temp.path().join("linked");
        std::os::unix::fs::symlink(outside.path(), &linked_parent).unwrap();
        let through_link = linked_parent.join("config");

        assert!(crosses_untrusted_symlink(&through_link).unwrap());
        assert!(!crosses_untrusted_symlink(&target).unwrap());

        // The strict walk used as root refuses the link, reading or not.
        let error = FileOpener::strict(&through_link).err().unwrap();
        assert!(is_untrusted_symlink(&error), "unexpected error: {error:#}");
        assert!(format!("{error:#}").contains("refusing to follow symlink"));
        assert_eq!(mode(&target), 0o644);

        // A user-level change follows it, on every Unix, even when the file
        // cannot be read, so it matches the plan's user-level `update`.
        set_file_metadata(&through_link, None, None, Some(0o600)).unwrap();
        assert_eq!(mode(&target), 0o600);
        fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).unwrap();
        let mut unreadable = metadata_only(PathInspection::Present {
            kind: ManagedPathKind::File,
            current: "file mode 0000".to_string(),
            metadata_matches: false,
            content_matches: None,
            owner_uid: current_uid(),
            crosses_untrusted_symlink: true,
        });
        unreadable.path = through_link.clone();
        unreadable.mode = Some(0o640);
        assert_eq!(unreadable.plan().unwrap().action, ResourceAction::Update);
        unreadable.operation().unwrap().unwrap().apply().unwrap();
        assert_eq!(mode(&target), 0o640);

        // Walking a parent needs only search permission on Linux, so a
        // directory its owner cannot list still resolves strictly.
        #[cfg(target_os = "linux")]
        {
            let unlisted = temp.path().join("unlisted");
            fs::create_dir(&unlisted).unwrap();
            let inside = unlisted.join("config");
            fs::write(&inside, "content").unwrap();
            fs::set_permissions(&unlisted, fs::Permissions::from_mode(0o311)).unwrap();
            let opened = FileOpener::strict(&inside).map(|_| ());
            fs::set_permissions(&unlisted, fs::Permissions::from_mode(0o755)).unwrap();
            opened.unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_no_follow_is_retried_with_privilege() {
        use nix::errno::Errno;

        for unsupported in [Errno::ENOTSUP, Errno::EOPNOTSUPP] {
            let mapped = unsupported_no_follow_as_permission_denied(unsupported);
            assert_eq!(mapped, Errno::EACCES);
            let report = Err::<(), _>(mapped).wrap_err("wrapped").unwrap_err();
            assert!(is_permission_denied(&report));
        }
        assert_eq!(
            unsupported_no_follow_as_permission_denied(Errno::ENOENT),
            Errno::ENOENT
        );
    }

    #[test]
    fn plans_follow_the_parent_symlink_rule_apply_uses() {
        let through_symlink = |owner: Option<&str>, owner_uid: Option<u32>| {
            let mut request = metadata_only(PathInspection::Present {
                kind: ManagedPathKind::File,
                current: "file mode 0644".to_string(),
                metadata_matches: false,
                content_matches: None,
                owner_uid,
                crosses_untrusted_symlink: true,
            });
            request.owner = owner.map(str::to_string);
            request
        };

        // Declared ownership always runs as root, which refuses the link.
        let elevated = through_symlink(Some("root"), current_uid());
        let plan = elevated.plan().unwrap();
        assert_eq!(plan.action, ResourceAction::Unknown);
        assert!(
            plan.current.contains("declare the resolved path"),
            "{}",
            plan.current
        );
        assert!(elevated.operation().is_err());

        // A mode change to the user's own file runs as that user and follows it.
        if current_uid().is_some_and(|uid| uid != 0) {
            let user_level = through_symlink(None, current_uid());
            assert_eq!(user_level.plan().unwrap().action, ResourceAction::Update);
            assert!(user_level.operation().unwrap().is_some());

            // Someone else's file needs root to change.
            let other_owner = through_symlink(None, current_uid().map(|uid| uid + 1));
            assert_eq!(other_owner.plan().unwrap().action, ResourceAction::Unknown);
        }
    }

    #[test]
    fn metadata_only_changes_round_trip_through_the_privileged_helper() {
        let plan = PrivilegedPlan {
            actions: vec![PrivilegedAction::SetFileMetadata {
                path: PathBuf::from("/etc/example"),
                owner: Some("root".to_string()),
                group: None,
                mode: None,
            }],
        };
        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(json["actions"][0]["type"], "set_file_metadata");
        let parsed: PrivilegedPlan = serde_json::from_value(json).unwrap();
        let action = &parsed.actions[0];
        assert!(matches!(
            action,
            PrivilegedAction::SetFileMetadata { owner: Some(owner), mode: None, .. } if owner == "root"
        ));
        // Ownership changes are sent to the helper without a first attempt.
        assert!(action.requires_preemptive_elevation().unwrap());
        assert_eq!(action.description(), "set permissions on file /etc/example");
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
            owner_uid: None,
            crosses_untrusted_symlink: false,
        });

        let notifications = pending_notifications(&[changed, unsafe_change], &[]).unwrap();
        assert!(notifications.contains("example"));
        assert!(!notifications.contains("ignored"));
    }

    #[cfg(unix)]
    #[test]
    fn replaces_wrong_types_and_creates_missing_parents() {
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
        create_directory(&nested, None, None, 0o700, false).unwrap();
        assert!(undeclared_parent.is_dir());
        assert!(nested.is_dir());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(nested).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let blocking_file = temp.path().join("blocking-file");
        fs::write(&blocking_file, "content").unwrap();
        let blocked = blocking_file.join("nested");
        let error = create_directory(&blocked, None, None, 0o755, false).unwrap_err();
        assert!(
            format!("{error:#}")
                .contains(&format!("failed to create directory {}", blocked.display())),
            "unexpected error: {error:#}"
        );

        // Root-owned parents are trusted, so symlink traversal is permitted.
        if !nix::unistd::geteuid().is_root() {
            let external = tempfile::tempdir().unwrap();
            let symlink_parent = temp.path().join("symlink-parent");
            std::os::unix::fs::symlink(external.path(), &symlink_parent).unwrap();
            let escaped = symlink_parent.join("nested");
            let error = create_directory(&escaped, None, None, 0o755, false).unwrap_err();
            assert!(
                format!("{error:#}").contains("refusing to follow symlink"),
                "unexpected error: {error:#}"
            );
            assert!(!external.path().join("nested").exists());
        }
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

    fn remove_empty_request(toml: &str, config: &Config) -> Result<ManagedFileRequest> {
        ManagedFileRequest::from_toml(
            config,
            PathBuf::from("/opt/example/config"),
            toml::from_str(toml).unwrap(),
            Path::new("/"),
            file("/opt/example/config", ManagedState::Present).origin,
            &super::super::secrets::SecretValues::default(),
        )
    }

    #[tokio::test]
    async fn remove_empty_turns_a_blank_render_into_a_removal() {
        let config = Config::get().await.unwrap();
        for content in ["", " \n\t\n"] {
            let request = remove_empty_request(
                &format!("template = true\nremove_empty = true\ncontent = {content:?}"),
                &config,
            )
            .unwrap();
            assert_eq!(request.state, ManagedState::Absent);
            assert!(request.rendered_empty());
            assert_eq!(request.content, None);
        }

        let request = remove_empty_request(
            "template = true\nremove_empty = true\ncontent = \"{% if false %}x{% endif %}\"",
            &config,
        )
        .unwrap();
        assert_eq!(request.state, ManagedState::Absent);

        let request = remove_empty_request(
            "template = true\nremove_empty = true\ncontent = \" kept \"",
            &config,
        )
        .unwrap();
        assert_eq!(request.state, ManagedState::Present);
        assert_eq!(request.content.as_deref(), Some(" kept "));

        // Without remove_empty an empty render is ordinary empty content.
        let request = remove_empty_request("template = true\ncontent = \"\"", &config).unwrap();
        assert_eq!(request.state, ManagedState::Present);
        assert_eq!(request.content.as_deref(), Some(""));
    }

    #[tokio::test]
    async fn remove_empty_requires_a_present_template() {
        let config = Config::get().await.unwrap();
        let error = remove_empty_request("remove_empty = true\ncontent = \"\"", &config)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("remove_empty requires template = true"),
            "{error}"
        );
        let error = remove_empty_request(
            "template = true\nremove_empty = true\nstate = \"absent\"",
            &config,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("remove_empty applies only to present files"),
            "{error}"
        );
    }

    #[test]
    fn rendered_empty_files_plan_and_notify_as_removals() {
        let mut removed = file("/opt/example/config", ManagedState::Absent);
        removed.rendered_empty = true;
        removed.notify.push("example".to_string());
        removed.inspection = Some(PathInspection::Present {
            kind: ManagedPathKind::File,
            current: "file".to_string(),
            metadata_matches: true,
            content_matches: None,
            owner_uid: None,
            crosses_untrusted_symlink: false,
        });
        let plan = removed.plan().unwrap();
        assert_eq!(plan.action, ResourceAction::Remove);
        assert_eq!(plan.desired, "absent (template rendered empty)");
        assert!(matches!(
            removed.operation().unwrap(),
            Some(PrivilegedAction::RemoveFile { .. })
        ));
        assert!(
            pending_notifications(&[removed.clone()], &[])
                .unwrap()
                .contains("example")
        );

        removed.inspection = Some(PathInspection::Missing);
        assert_eq!(removed.plan().unwrap().action, ResourceAction::Noop);

        // A directory at the target is refused, as for any absent file.
        removed.inspection = Some(PathInspection::Present {
            kind: ManagedPathKind::Directory,
            current: "directory".to_string(),
            metadata_matches: false,
            content_matches: None,
            owner_uid: None,
            crosses_untrusted_symlink: false,
        });
        assert_eq!(removed.plan().unwrap().action, ResourceAction::Unknown);
        assert!(removed.operation().is_err());
    }

    #[test]
    fn rendered_empty_files_keep_their_declared_structure_rules() {
        let mut removed = file("/opt/example/nested/config", ManagedState::Absent);
        removed.rendered_empty = true;
        assert!(
            validate_requests(
                &[removed.clone()],
                &[directory("/opt/example", ManagedState::Absent)],
            )
            .is_err()
        );

        let mut parent = directory("/opt/example", ManagedState::Present);
        removed.phase = ManagedFilePhase::PrePackages;
        assert!(validate_requests(&[removed.clone()], &[parent.clone()]).is_err());
        parent.phase = ManagedFilePhase::PrePackages;
        assert!(validate_requests(&[removed], &[parent]).is_ok());
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
