//! Persistent formula state and typed post-install operations.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use eyre::{WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::api::Formula;
use super::prefix;
use crate::cmd::CmdLineRunner;
use crate::result::Result;
use crate::sandbox::SandboxConfig;

const MAX_FAILURE_LOG_BYTES: usize = 32 * 1024;

#[derive(Debug, Serialize)]
pub(super) struct PreparedFormulaLifecycle {
    formula: String,
    keg: PathBuf,
    formula_snapshot_sha256: Option<String>,
    steps: Vec<PreparedStep>,
}

impl PreparedFormulaLifecycle {
    /// Legacy repair must validate the installed formula snapshot against the
    /// artifact that produced it. Bottle snapshots are not guaranteed to have
    /// the same checksum as the current tap source for the same package
    /// version, so the repair preflight replaces the source checksum with the
    /// checksum from the currently pinned, verified bottle when applicable.
    pub(super) fn set_formula_snapshot_sha256(&mut self, sha256: String) {
        self.formula_snapshot_sha256 = Some(sha256);
    }
}

#[derive(Debug, Serialize)]
enum PreparedStep {
    Mkdir {
        path: PathBuf,
        guards: Vec<PreparedGuard>,
    },
    Remove {
        paths: Vec<PreparedPattern>,
        recursive: bool,
        symlink_target_contains: Option<String>,
        guards: Vec<PreparedGuard>,
    },
    Copy {
        sources: PreparedSources,
        target: PathBuf,
        recursive: bool,
        guards: Vec<PreparedGuard>,
    },
    Symlink {
        sources: PreparedSources,
        target: PathBuf,
        force: bool,
        guards: Vec<PreparedGuard>,
    },
    SetPermissions {
        paths: Vec<PreparedPattern>,
        permission: LifecyclePermissionKind,
        non_recursive: bool,
        guards: Vec<PreparedGuard>,
    },
    Run(PreparedRun),
}

#[derive(Debug, Serialize)]
struct PreparedRun {
    step_index: usize,
    executable: PathBuf,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    env: BTreeMap<String, String>,
    stdin_path: Option<PathBuf>,
    stdout_path: Option<PathBuf>,
    guards: Vec<PreparedGuard>,
    log_dir: PathBuf,
}

#[derive(Debug, Serialize)]
enum PreparedSources {
    One(PathBuf),
    Glob(PreparedPattern),
}

#[derive(Debug, Serialize)]
struct PreparedPattern {
    patterns: Vec<String>,
}

#[derive(Debug, Serialize)]
enum PreparedGuard {
    IfExists(PreparedPattern),
    UnlessExists(PreparedPattern),
    Platform(bool),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPathSpec {
    path: String,
    #[serde(default)]
    base: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGuard {
    condition: String,
    #[serde(default)]
    #[serde(rename = "id")]
    _id: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMkdir {
    #[serde(rename = "type")]
    kind: String,
    path: RawPathSpec,
    #[serde(default)]
    guards: Vec<RawGuard>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRemove {
    #[serde(rename = "type")]
    kind: String,
    paths: Vec<RawPathSpec>,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    symlink_target_contains: Option<String>,
    #[serde(default)]
    guards: Vec<RawGuard>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCopy {
    #[serde(rename = "type")]
    kind: String,
    source: RawPathSpec,
    target: RawPathSpec,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    source_glob: bool,
    #[serde(default)]
    guards: Vec<RawGuard>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSymlink {
    #[serde(rename = "type")]
    kind: String,
    source: RawPathSpec,
    target: RawPathSpec,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    source_glob: bool,
    #[serde(default)]
    guards: Vec<RawGuard>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSetPermissions {
    #[serde(rename = "type")]
    kind: String,
    paths: Vec<RawPathSpec>,
    permissions: String,
    #[serde(default)]
    non_recursive: bool,
    #[serde(default)]
    guards: Vec<RawGuard>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRun {
    #[serde(rename = "type")]
    kind: String,
    command: RawPathSpec,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    chdir: Option<RawPathSpec>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    stdin_path: Option<RawPathSpec>,
    #[serde(default)]
    stdout_path: Option<RawPathSpec>,
    #[serde(default)]
    guards: Vec<RawGuard>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LifecycleState {
    complete: bool,
    #[serde(default)]
    phase: LifecyclePhase,
    #[serde(default)]
    install_identity: Option<LifecycleInstallIdentity>,
    #[serde(default)]
    shared_state: Vec<LifecycleSharedState>,
    #[serde(default)]
    symlinks: Vec<LifecycleSymlink>,
    #[serde(default)]
    required_paths: Vec<PathBuf>,
    #[serde(default)]
    absent_patterns: Vec<String>,
    #[serde(default)]
    permissions: Vec<LifecyclePermission>,
    #[serde(default)]
    repair: Option<LifecycleRepairJournal>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LifecycleInstallIdentity {
    formula: String,
    receipt: LifecycleReceiptIdentity,
    formula_snapshot_sha256: String,
    incarnation: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct LifecycleReceiptIdentity {
    #[serde(default)]
    built_as_bottle: Option<bool>,
    #[serde(default)]
    poured_from_bottle: Option<bool>,
    #[serde(default)]
    time: Option<u64>,
    #[serde(default)]
    source_modified_time: Option<u64>,
    #[serde(default)]
    arch: Option<String>,
    #[serde(default)]
    source: LifecycleReceiptSource,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct LifecycleReceiptSource {
    #[serde(default)]
    spec: Option<String>,
    #[serde(default)]
    versions: LifecycleReceiptVersions,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    tap: Option<String>,
    #[serde(default)]
    tap_git_head: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct LifecycleReceiptVersions {
    #[serde(default)]
    stable: Option<String>,
    #[serde(default)]
    head: Option<String>,
    #[serde(default)]
    version_scheme: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LifecycleSharedState {
    source: PathBuf,
    target: PathBuf,
    #[serde(default)]
    kind: LifecycleSharedStateKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LifecycleSharedStateKind {
    #[default]
    File,
    Directory,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LifecycleRepairJournal {
    effects: Vec<LifecycleRepairEffect>,
    next: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LifecycleRepairEffect {
    Copy {
        source: PathBuf,
        target: PathBuf,
    },
    CreateDirectory {
        source: PathBuf,
        target: PathBuf,
    },
    Symlink {
        source: PathBuf,
        target: PathBuf,
    },
    SetPermissions {
        path: PathBuf,
        permission: LifecyclePermissionKind,
        identity: LifecyclePermissionIdentity,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum LifecycleHealth {
    Healthy,
    Repairable(Vec<String>),
    ReinstallRequired(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LifecycleInstallProgress {
    Absent,
    Complete,
    Incomplete,
}

pub(super) struct PreparedLifecycleRemoval {
    keg: PathBuf,
    state_path: PathBuf,
    state_directory: Option<DirectoryAncestry>,
    keg_ancestry: Option<DirectoryAncestry>,
    state_sha256: Option<String>,
    symlinks: Vec<PreparedLifecycleSymlinkRemoval>,
    disposition: LifecycleRemovalDisposition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryIdentity {
    path: PathBuf,
    device: u64,
    inode: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DirectoryAncestry(Vec<DirectoryIdentity>);

#[derive(Debug)]
struct PreparedLifecycleSymlinkRemoval {
    path: PathBuf,
    ancestry: DirectoryAncestry,
    device: u64,
    inode: u64,
    target: PathBuf,
}

enum LifecycleRemovalDisposition {
    Absent,
    CurrentMise,
    ProvenNativeStale {
        receipt: Box<LifecycleReceiptIdentity>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LifecyclePhase {
    #[default]
    Initial,
    SharedState,
    Complete,
}

#[derive(Debug, Serialize, Deserialize)]
struct LifecycleSymlink {
    source: PathBuf,
    target: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LifecyclePermissionKind {
    UserWrite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LifecyclePermission {
    path: PathBuf,
    permission: LifecyclePermissionKind,
    identity: LifecyclePermissionIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LifecyclePermissionIdentity {
    RegularFile {
        sha256: String,
        device: u64,
        inode: u64,
    },
    Directory {
        device: u64,
        inode: u64,
    },
    Symlink {
        target: PathBuf,
        target_identity: Box<LifecyclePermissionIdentity>,
    },
}

/// Compile lifecycle metadata into the only representation execution accepts.
/// This performs no filesystem mutation and must run for the complete mutation
/// set before the first keg is extracted or built.
pub(super) fn prepare(formula: &Formula, keg: &Path) -> Result<PreparedFormulaLifecycle> {
    if formula.post_install_defined {
        bail!(
            "brew:{} uses opaque Ruby post_install without a complete typed representation; no package state was changed",
            formula.name
        );
    }
    let mut steps = Vec::with_capacity(formula.post_install_steps.len());
    for (index, step) in formula.post_install_steps.iter().enumerate() {
        let kind = step.get("type").and_then(Value::as_str).unwrap_or("");
        let prepared = (|| match kind {
            "mkdir_p" => prepare_mkdir(formula, keg, parse_step(step)?),
            "remove" => prepare_remove(formula, keg, parse_step(step)?),
            "copy" => prepare_copy(formula, keg, parse_step(step)?),
            "run" => prepare_run(formula, keg, index, parse_step(step)?),
            "set_permissions" => {
                prepare_set_permissions(formula, keg, parse_step(step)?)
            }
            "symlink" => prepare_symlink(formula, keg, parse_step(step)?),
            _ => bail!("unsupported type {kind:?}"),
        })()
        .wrap_err_with(|| {
            format!(
                "brew:{} post-install step {index} type {kind:?} is invalid; no package state was changed",
                formula.name
            )
        })?;
        steps.push(prepared);
    }
    Ok(PreparedFormulaLifecycle {
        formula: formula.name.clone(),
        keg: keg.to_path_buf(),
        formula_snapshot_sha256: formula
            .ruby_source_checksum
            .as_ref()
            .and_then(|checksum| checksum.sha256.clone()),
        steps,
    })
}

pub(super) fn prepared_identity_sha256(prepared: &PreparedFormulaLifecycle) -> Result<String> {
    Ok(crate::hash::hash_sha256_to_str(&serde_json::to_string(
        prepared,
    )?))
}

fn parse_step<T: for<'de> Deserialize<'de>>(step: &Value) -> Result<T> {
    serde_json::from_value(step.clone()).map_err(Into::into)
}

fn prepare_mkdir(formula: &Formula, keg: &Path, raw: RawMkdir) -> Result<PreparedStep> {
    ensure_step_kind(&raw.kind, "mkdir_p")?;
    Ok(PreparedStep::Mkdir {
        path: resolve_write_path(formula, keg, &raw.path)?,
        guards: prepare_guards(formula, keg, raw.guards)?,
    })
}

fn prepare_remove(formula: &Formula, keg: &Path, raw: RawRemove) -> Result<PreparedStep> {
    ensure_step_kind(&raw.kind, "remove")?;
    let paths = raw
        .paths
        .iter()
        .map(|path| prepare_pattern(formula, keg, path, PathAccess::Write))
        .collect::<Result<Vec<_>>>()?;
    Ok(PreparedStep::Remove {
        paths,
        recursive: raw.recursive,
        symlink_target_contains: raw.symlink_target_contains,
        guards: prepare_guards(formula, keg, raw.guards)?,
    })
}

fn prepare_copy(formula: &Formula, keg: &Path, raw: RawCopy) -> Result<PreparedStep> {
    ensure_step_kind(&raw.kind, "copy")?;
    Ok(PreparedStep::Copy {
        sources: prepare_sources(formula, keg, &raw.source, raw.source_glob)?,
        target: resolve_write_path(formula, keg, &raw.target)?,
        recursive: raw.recursive,
        guards: prepare_guards(formula, keg, raw.guards)?,
    })
}

fn prepare_symlink(formula: &Formula, keg: &Path, raw: RawSymlink) -> Result<PreparedStep> {
    ensure_step_kind(&raw.kind, "symlink")?;
    Ok(PreparedStep::Symlink {
        sources: prepare_sources(formula, keg, &raw.source, raw.source_glob)?,
        target: resolve_write_path(formula, keg, &raw.target)?,
        force: raw.force,
        guards: prepare_guards(formula, keg, raw.guards)?,
    })
}

fn prepare_set_permissions(
    formula: &Formula,
    keg: &Path,
    raw: RawSetPermissions,
) -> Result<PreparedStep> {
    ensure_step_kind(&raw.kind, "set_permissions")?;
    if !cfg!(unix) {
        bail!(
            "brew:{} set_permissions requires Unix permission semantics; no package state was changed",
            formula.name
        );
    }
    let permission = match raw.permissions.as_str() {
        "u+w" => LifecyclePermissionKind::UserWrite,
        permissions => bail!("unsupported permissions mode {permissions:?}"),
    };
    Ok(PreparedStep::SetPermissions {
        paths: raw
            .paths
            .iter()
            .map(|path| prepare_pattern(formula, keg, path, PathAccess::Write))
            .collect::<Result<Vec<_>>>()?,
        permission,
        non_recursive: raw.non_recursive,
        guards: prepare_guards(formula, keg, raw.guards)?,
    })
}

fn prepare_run(formula: &Formula, keg: &Path, index: usize, raw: RawRun) -> Result<PreparedStep> {
    ensure_step_kind(&raw.kind, "run")?;
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        bail!(
            "brew:{} post-install step {index} requires run confinement unavailable on this platform; no package state was changed",
            formula.name
        );
    }
    let executable = resolve_read_path(formula, keg, &raw.command)?;
    let args = raw
        .args
        .iter()
        .map(|arg| expand_templates(formula, keg, arg))
        .collect::<Result<Vec<_>>>()?;
    let cwd = raw
        .chdir
        .as_ref()
        .map(|path| resolve_read_path(formula, keg, path))
        .transpose()?;
    let mut env = BTreeMap::new();
    for (key, value) in raw.env {
        if key.is_empty()
            || !key
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric())
        {
            bail!(
                "brew:{} post-install step {index} has invalid environment key {key:?}",
                formula.name
            );
        }
        env.insert(key, expand_templates(formula, keg, &value)?);
    }
    let stdin_path = raw
        .stdin_path
        .as_ref()
        .map(|path| resolve_read_path(formula, keg, path))
        .transpose()?;
    let stdout_path = raw
        .stdout_path
        .as_ref()
        .map(|path| resolve_write_path(formula, keg, path))
        .transpose()?;
    let identity = crate::hash::hash_to_str(&(prefix::prefix(), &formula.name, keg));
    Ok(PreparedStep::Run(PreparedRun {
        step_index: index,
        executable,
        args,
        cwd,
        env,
        stdin_path,
        stdout_path,
        guards: prepare_guards(formula, keg, raw.guards)?,
        log_dir: crate::dirs::STATE
            .join("brew-formula-lifecycle")
            .join("logs")
            .join(identity),
    }))
}

fn ensure_step_kind(actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("post-install step type changed while preparing metadata")
    }
    Ok(())
}

fn prepare_sources(
    formula: &Formula,
    keg: &Path,
    source: &RawPathSpec,
    glob: bool,
) -> Result<PreparedSources> {
    if glob {
        Ok(PreparedSources::Glob(prepare_pattern(
            formula,
            keg,
            source,
            PathAccess::Read,
        )?))
    } else {
        Ok(PreparedSources::One(resolve_read_path(
            formula, keg, source,
        )?))
    }
}

fn prepare_guards(
    formula: &Formula,
    keg: &Path,
    guards: Vec<RawGuard>,
) -> Result<Vec<PreparedGuard>> {
    guards
        .into_iter()
        .map(|guard| match guard.condition.as_str() {
            "if_exists" | "unless_exists" => {
                if guard.value.is_some() {
                    bail!("path guard must not contain value")
                }
                let path = guard
                    .path
                    .ok_or_else(|| eyre!("path guard is missing path"))?;
                let pattern = prepare_pattern(
                    formula,
                    keg,
                    &RawPathSpec {
                        path,
                        base: guard.base,
                    },
                    PathAccess::Read,
                )?;
                if guard.condition == "if_exists" {
                    Ok(PreparedGuard::IfExists(pattern))
                } else {
                    Ok(PreparedGuard::UnlessExists(pattern))
                }
            }
            "on" => {
                if guard.path.is_some() || guard.base.is_some() {
                    bail!("platform guard must not contain a path")
                }
                let matches = match guard.value.as_deref() {
                    Some("macos") => cfg!(target_os = "macos"),
                    Some("linux") => cfg!(target_os = "linux"),
                    value => bail!("unsupported post-install platform guard {value:?}"),
                };
                Ok(PreparedGuard::Platform(matches))
            }
            condition => bail!("unsupported post-install guard condition {condition:?}"),
        })
        .collect()
}

#[derive(Clone, Copy)]
enum PathAccess {
    Read,
    Write,
}

fn resolve_read_path(formula: &Formula, keg: &Path, spec: &RawPathSpec) -> Result<PathBuf> {
    let pattern = prepare_pattern(formula, keg, spec, PathAccess::Read)?;
    single_path(pattern, "read path")
}

fn resolve_write_path(formula: &Formula, keg: &Path, spec: &RawPathSpec) -> Result<PathBuf> {
    let pattern = prepare_pattern(formula, keg, spec, PathAccess::Write)?;
    single_path(pattern, "write path")
}

fn single_path(pattern: PreparedPattern, label: &str) -> Result<PathBuf> {
    if pattern.patterns.len() != 1 || has_glob_magic(&pattern.patterns[0]) {
        bail!("post-install {label} must resolve to one non-glob path")
    }
    Ok(PathBuf::from(&pattern.patterns[0]))
}

fn prepare_pattern(
    formula: &Formula,
    keg: &Path,
    spec: &RawPathSpec,
    access: PathAccess,
) -> Result<PreparedPattern> {
    let expanded = expand_templates(formula, keg, &spec.path)?;
    let base = match spec.base.as_deref() {
        Some(base) => Some(template_base(formula, keg, base)?),
        None => None,
    };
    let path = PathBuf::from(expanded);
    let path = if path.is_absolute() {
        if base.is_some() {
            bail!("absolute post-install path must not declare a base")
        }
        path
    } else {
        base.unwrap_or_else(|| keg.to_path_buf()).join(path)
    };
    let normalized = super::pour::lexical_normalize(&path);
    let patterns = expand_braces(&normalized.to_string_lossy());
    for pattern in &patterns {
        ensure_contained(Path::new(pattern), keg, access)?;
    }
    Ok(PreparedPattern { patterns })
}

fn ensure_contained(path: &Path, keg: &Path, access: PathAccess) -> Result<()> {
    let path = super::pour::lexical_normalize(path);
    let shared = super::pour::lexical_normalize(&prefix::prefix());
    let keg = super::pour::lexical_normalize(keg);
    let allowed = match access {
        PathAccess::Write => path.starts_with(&shared),
        PathAccess::Read => {
            path.starts_with(&shared)
                || path.starts_with(&keg)
                || [
                    "/System",
                    "/Library",
                    "/usr",
                    "/bin",
                    "/sbin",
                    "/etc",
                    "/private/etc",
                ]
                .iter()
                .any(|root| path.starts_with(root))
        }
    };
    if !allowed {
        bail!(
            "post-install path {} escapes the allowed {} roots",
            path.display(),
            match access {
                PathAccess::Read => "read",
                PathAccess::Write => "write",
            }
        );
    }
    Ok(())
}

fn has_glob_magic(value: &str) -> bool {
    value.contains(['*', '?', '['])
}

fn identity_marker_path(keg: &Path) -> PathBuf {
    keg.join(".brew/.mise-lifecycle-incarnation")
}

pub(super) fn capture_directory_ancestry(path: &Path) -> Result<DirectoryAncestry> {
    let path = super::pour::lexical_normalize(path);
    if !path.is_absolute() {
        bail!(
            "directory ancestry path is not absolute: {}",
            path.display()
        )
    }
    let mut paths = path.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
    paths.reverse();
    let mut identities = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = path
            .symlink_metadata()
            .wrap_err_with(|| format!("directory ancestry is missing: {}", path.display()))?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            bail!(
                "directory ancestry contains a non-real directory: {}",
                path.display()
            )
        }
        let (device, inode) = permission_device_inode(&metadata)?;
        identities.push(DirectoryIdentity {
            path,
            device,
            inode,
        });
    }
    Ok(DirectoryAncestry(identities))
}

pub(super) fn validate_directory_ancestry(expected: &DirectoryAncestry) -> Result<()> {
    for identity in &expected.0 {
        let metadata = identity.path.symlink_metadata().wrap_err_with(|| {
            format!(
                "directory ancestry changed after preflight: {}",
                identity.path.display()
            )
        })?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            bail!(
                "directory ancestry changed to a non-real directory: {}",
                identity.path.display()
            )
        }
        let (device, inode) = permission_device_inode(&metadata)?;
        if (device, inode) != (identity.device, identity.inode) {
            bail!(
                "directory ancestry identity changed after preflight: {}",
                identity.path.display()
            )
        }
    }
    Ok(())
}

pub(super) fn validate_lifecycle_keg_ancestry(keg: &Path) -> Result<()> {
    let prefix = super::pour::lexical_normalize(&prefix::prefix());
    let cellar = super::pour::lexical_normalize(&prefix::cellar());
    let keg = super::pour::lexical_normalize(keg);
    if !cellar.starts_with(&prefix) || !keg.starts_with(&cellar) {
        bail!(
            "lifecycle keg escapes the configured Cellar: {}",
            keg.display()
        )
    }
    let rack = keg
        .parent()
        .ok_or_else(|| eyre!("lifecycle keg has no formula rack: {}", keg.display()))?;
    if rack.parent() != Some(cellar.as_path()) {
        bail!(
            "lifecycle keg is not an immediate child of a formula rack: {}",
            keg.display()
        )
    }
    capture_directory_ancestry(&keg.join(".brew"))
        .wrap_err("lifecycle keg ancestry is not a chain of real directories")?;
    Ok(())
}

fn immutable_file_sha256(path: &Path, label: &str) -> Result<String> {
    let metadata = path
        .symlink_metadata()
        .wrap_err_with(|| format!("lifecycle {label} is missing: {}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!(
            "lifecycle {label} is not an immutable regular file: {}",
            path.display()
        )
    }
    crate::hash::file_hash_sha256(path, None)
}

fn read_receipt_contents(keg: &Path) -> Result<Vec<u8>> {
    let path = keg.join("INSTALL_RECEIPT.json");
    let metadata = path
        .symlink_metadata()
        .wrap_err_with(|| format!("lifecycle install receipt is missing: {}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!(
            "lifecycle install receipt is not an immutable regular file: {}",
            path.display()
        )
    }
    std::fs::read(&path).wrap_err_with(|| {
        format!(
            "could not read lifecycle install receipt: {}",
            path.display()
        )
    })
}

fn read_receipt_identity(keg: &Path) -> Result<LifecycleReceiptIdentity> {
    let path = keg.join("INSTALL_RECEIPT.json");
    serde_json::from_slice(&read_receipt_contents(keg)?)
        .wrap_err_with(|| format!("lifecycle install receipt is malformed: {}", path.display()))
}

fn read_proven_native_receipt_identity(keg: &Path) -> Result<LifecycleReceiptIdentity> {
    validate_lifecycle_keg_ancestry(keg)?;
    let path = keg.join("INSTALL_RECEIPT.json");
    let contents = read_receipt_contents(keg)?;
    let receipt: serde_json::Value = serde_json::from_slice(&contents)
        .wrap_err_with(|| format!("lifecycle install receipt is malformed: {}", path.display()))?;
    let homebrew_version = receipt
        .get("homebrew_version")
        .and_then(serde_json::Value::as_str)
        .filter(|version| !version.is_empty())
        .ok_or_else(|| eyre!("lifecycle install receipt has no Homebrew version"))?;
    if homebrew_version.ends_with(" (mise)") {
        bail!("lifecycle install receipt is still mise-owned")
    }
    serde_json::from_slice(&contents)
        .wrap_err_with(|| format!("lifecycle install receipt is malformed: {}", path.display()))
}

/// Hash the canonical immutable Tab projection used by lifecycle and pour finalization.
pub(super) fn receipt_identity_sha256(keg: &Path) -> Result<String> {
    let receipt = read_receipt_identity(keg)?;
    let canonical = serde_json::to_string(&receipt)?;
    Ok(crate::hash::hash_sha256_to_str(&canonical))
}

fn formula_name_from_keg(keg: &Path) -> Result<String> {
    keg.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| eyre!("lifecycle keg has no UTF-8 formula name: {}", keg.display()))
}

fn capture_install_identity(
    prepared: &PreparedFormulaLifecycle,
) -> Result<LifecycleInstallIdentity> {
    validate_lifecycle_keg_ancestry(&prepared.keg)?;
    let formula = formula_name_from_keg(&prepared.keg)?;
    if formula != prepared.formula {
        bail!(
            "brew:{} lifecycle formula does not match keg {}",
            prepared.formula,
            prepared.keg.display()
        )
    }
    let receipt = read_receipt_identity(&prepared.keg)?;
    let formula_snapshot_sha256 = immutable_file_sha256(
        &prepared
            .keg
            .join(".brew")
            .join(format!("{}.rb", prepared.formula)),
        "formula snapshot",
    )?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let incarnation = crate::hash::hash_sha256_to_str(&format!(
        "{}\0{}\0{}\0{}\0{}",
        prepared.keg.display(),
        serde_json::to_string(&receipt)?,
        formula_snapshot_sha256,
        std::process::id(),
        nonce
    ));
    let marker = identity_marker_path(&prepared.keg);
    let temporary = tempfile::Builder::new()
        .prefix(".mise-lifecycle-incarnation-")
        .tempfile_in(marker.parent().unwrap())?
        .into_temp_path();
    fs::write(&temporary, &incarnation)?;
    temporary
        .persist_noclobber(&marker)
        .map_err(|error| error.error)?;
    Ok(LifecycleInstallIdentity {
        formula,
        receipt,
        formula_snapshot_sha256,
        incarnation,
    })
}

fn validate_install_identity(keg: &Path, state: &LifecycleState) -> Result<()> {
    validate_lifecycle_keg_ancestry(keg)?;
    let expected = state
        .install_identity
        .as_ref()
        .ok_or_else(|| eyre!("lifecycle state has no bound install identity"))?;
    if formula_name_from_keg(keg)? != expected.formula {
        bail!("lifecycle state formula does not match current keg")
    }
    let marker = identity_marker_path(keg);
    let metadata = marker
        .symlink_metadata()
        .wrap_err("lifecycle install-incarnation marker is missing")?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!("lifecycle install-incarnation marker is not a regular file")
    }
    if crate::file::read_to_string(&marker)? != expected.incarnation {
        bail!("lifecycle install incarnation does not match current keg")
    }
    if read_receipt_identity(keg)? != expected.receipt {
        bail!("lifecycle install receipt does not match current keg")
    }
    let snapshot = keg.join(".brew").join(format!("{}.rb", expected.formula));
    if immutable_file_sha256(&snapshot, "formula snapshot")? != expected.formula_snapshot_sha256 {
        bail!("lifecycle formula snapshot does not match current keg")
    }
    Ok(())
}

pub(super) fn needs_repair(keg: &Path) -> bool {
    !matches!(health(keg, false), LifecycleHealth::Healthy)
}

pub(super) fn install_progress(keg: &Path) -> LifecycleInstallProgress {
    if validate_lifecycle_keg_ancestry(keg).is_err() {
        return LifecycleInstallProgress::Incomplete;
    }
    let path = state_path(keg);
    match read_state_if_present(&path) {
        Ok(None) => LifecycleInstallProgress::Absent,
        Ok(Some(state))
            if validate_install_identity(keg, &state).is_ok()
                && validate_shared_state(keg, &state).is_ok()
                && state.complete
                && state.phase == LifecyclePhase::Complete =>
        {
            LifecycleInstallProgress::Complete
        }
        Ok(Some(_)) | Err(_) => LifecycleInstallProgress::Incomplete,
    }
}

/// Classify formula lifecycle health without fetching metadata or mutating state.
/// A missing private state file is accepted for native Homebrew state only when
/// the observable shared defaults are present. Mise-owned legacy state remains
/// actionable because old mise versions never ran this lifecycle at all.
pub(super) fn health(keg: &Path, mise_owned: bool) -> LifecycleHealth {
    if let Err(error) = validate_lifecycle_keg_ancestry(keg) {
        return LifecycleHealth::ReinstallRequired(vec![format!(
            "lifecycle keg ancestry is invalid: {error}"
        )]);
    }
    let shared_missing = match shared_missing_paths(keg) {
        Ok(paths) => paths,
        Err(error) => {
            return LifecycleHealth::ReinstallRequired(vec![format!(
                "shared lifecycle source topology is invalid: {error}"
            )]);
        }
    };
    let path = state_path(keg);
    let state =
        match read_state_if_present(&path) {
            Ok(Some(state)) => state,
            Ok(None) if !mise_owned => return native_health(shared_missing),
            Ok(None) => {
                let mut reasons = vec![
                    "lifecycle state absent; install_etc_var and post-install were not recorded"
                        .to_string(),
                ];
                reasons.extend(shared_missing.into_iter().map(|(_, target)| {
                    format!("shared lifecycle path missing: {}", target.display())
                }));
                return LifecycleHealth::Repairable(reasons);
            }
            Err(_) if !mise_owned => return native_health(shared_missing),
            Err(error) => {
                return LifecycleHealth::ReinstallRequired(vec![format!(
                    "lifecycle state is unreadable at {}: {error}",
                    path.display(),
                )]);
            }
        };
    if let Err(error) = validate_install_identity(keg, &state) {
        if !mise_owned {
            return native_health(shared_missing);
        }
        return LifecycleHealth::ReinstallRequired(vec![format!(
            "lifecycle state does not match current keg installation: {error}"
        )]);
    }
    if !state.complete || state.phase != LifecyclePhase::Complete {
        return LifecycleHealth::ReinstallRequired(vec![
            "lifecycle stopped before completion; a post-install action may have an unknown outcome"
                .to_string(),
        ]);
    }
    if let Err(error) = validate_shared_state(keg, &state) {
        return LifecycleHealth::ReinstallRequired(vec![format!(
            "shared lifecycle provenance is invalid: {error}"
        )]);
    }

    let mut repairable = BTreeSet::new();
    let mut reinstall = BTreeSet::new();
    if state.repair.is_some() {
        repairable.insert("an idempotent lifecycle repair journal is incomplete".to_string());
    }
    for mapping in &state.shared_state {
        match shared_mapping_satisfied(mapping) {
            Ok(true) => {}
            Ok(false) => match symlink_metadata_if_exists(&mapping.target) {
                Ok(None) => {
                    repairable.insert(format!(
                        "shared lifecycle {} missing: {}",
                        match mapping.kind {
                            LifecycleSharedStateKind::File => "path",
                            LifecycleSharedStateKind::Directory => "directory",
                        },
                        mapping.target.display()
                    ));
                }
                Err(error) => {
                    reinstall.insert(format!(
                        "shared lifecycle path is unreadable at {}: {error}",
                        mapping.target.display()
                    ));
                }
                Ok(Some(_)) => {
                    reinstall.insert(format!(
                        "shared lifecycle target has ambiguous type: {}",
                        mapping.target.display()
                    ));
                }
            },
            Err(error) => {
                reinstall.insert(format!(
                    "shared lifecycle path is unreadable at {}: {error}",
                    mapping.target.display()
                ));
            }
        }
    }
    for link in &state.symlinks {
        match node_exists(&link.source) {
            Ok(false) => {
                reinstall.insert(format!(
                    "post-install symlink source is missing: {}",
                    link.source.display()
                ));
            }
            Err(error) => {
                reinstall.insert(format!(
                    "post-install symlink source is unreadable at {}: {error}",
                    link.source.display()
                ));
            }
            Ok(true) => match resolved_symlink_target_checked(&link.target) {
                Ok(Some(target)) if target == link.source => {}
                Ok(_) => match symlink_metadata_if_exists(&link.target) {
                    Ok(None) => {
                        repairable.insert(format!(
                            "post-install symlink is missing: {}",
                            link.target.display()
                        ));
                    }
                    Ok(Some(_)) => {
                        reinstall.insert(format!(
                            "post-install target has ambiguous ownership: {}",
                            link.target.display()
                        ));
                    }
                    Err(error) => {
                        reinstall.insert(format!(
                            "post-install target is unreadable at {}: {error}",
                            link.target.display()
                        ));
                    }
                },
                Err(error) => {
                    reinstall.insert(format!(
                        "post-install target is unreadable at {}: {error}",
                        link.target.display()
                    ));
                }
            },
        }
    }
    for required in &state.required_paths {
        if state
            .shared_state
            .iter()
            .any(|mapping| mapping.target == *required)
        {
            continue;
        }
        match node_exists(required) {
            Ok(true) => {}
            Ok(false) => match shared_default_source_exists(keg, required) {
                Ok(true) => {
                    repairable.insert(format!(
                        "shared lifecycle path missing: {}",
                        required.display()
                    ));
                }
                Ok(false) => {
                    reinstall.insert(format!(
                        "post-install output is missing and cannot be replayed safely: {}",
                        required.display()
                    ));
                }
                Err(error) => {
                    reinstall.insert(format!(
                        "shared lifecycle source is unreadable for {}: {error}",
                        required.display()
                    ));
                }
            },
            Err(error) => {
                reinstall.insert(format!(
                    "post-install output is unreadable at {}: {error}",
                    required.display()
                ));
            }
        }
    }
    for permission in &state.permissions {
        match permission_identity_matches(permission) {
            Ok(false) => {
                reinstall.insert(format!(
                    "post-install permission target identity changed: {}",
                    permission.path.display()
                ));
            }
            Err(error) => {
                reinstall.insert(format!(
                    "post-install permission target is unreadable at {}: {error}",
                    permission.path.display()
                ));
            }
            Ok(true) => match permission_satisfied(&permission.path, permission.permission) {
                Ok(true) => {}
                Ok(false) => {
                    repairable.insert(format!(
                        "post-install permission is missing at {}",
                        permission.path.display()
                    ));
                }
                Err(error) => {
                    reinstall.insert(format!(
                        "post-install permission target is unreadable at {}: {error}",
                        permission.path.display()
                    ));
                }
            },
        }
    }
    for pattern in &state.absent_patterns {
        match pattern_has_matches(pattern) {
            Ok(true) => {
                reinstall.insert(format!(
                    "post-install removal invariant no longer holds: {pattern}"
                ));
            }
            Ok(false) => {}
            Err(error) => {
                reinstall.insert(format!(
                    "post-install removal invariant is unreadable for {pattern}: {error}"
                ));
            }
        }
    }
    if !reinstall.is_empty() {
        LifecycleHealth::ReinstallRequired(reinstall.into_iter().collect())
    } else if !repairable.is_empty() {
        LifecycleHealth::Repairable(repairable.into_iter().collect())
    } else {
        LifecycleHealth::Healthy
    }
}

fn symlink_metadata_if_exists(path: &Path) -> Result<Option<fs::Metadata>> {
    match path.symlink_metadata() {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn pattern_has_matches(pattern: &str) -> Result<bool> {
    match glob::glob(pattern)?.next() {
        Some(path) => {
            path?;
            Ok(true)
        }
        None => Ok(false),
    }
}

fn node_exists(path: &Path) -> Result<bool> {
    match symlink_metadata_if_exists(path)? {
        Some(metadata) if metadata.file_type().is_symlink() => match path.metadata() {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        },
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

fn native_health(shared_missing: Vec<(PathBuf, PathBuf)>) -> LifecycleHealth {
    if shared_missing.is_empty() {
        LifecycleHealth::Healthy
    } else {
        LifecycleHealth::ReinstallRequired(
            shared_missing
                .into_iter()
                .map(|(_, target)| {
                    format!(
                        "native Homebrew shared path missing without mise repair provenance: {}",
                        target.display()
                    )
                })
                .collect(),
        )
    }
}

fn shared_authorities(keg: &Path) -> Result<Vec<(PathBuf, LifecycleSharedStateKind)>> {
    let mut sources = vec![];
    for root in ["etc", "var"] {
        let source_root = keg.join(".bottle").join(root);
        match source_root.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Err(error) => return Err(error.into()),
            Ok(_) => bail!(
                ".bottle/{root} source has ambiguous type: {}",
                source_root.display()
            ),
        }
        sources.push((source_root.clone(), LifecycleSharedStateKind::Directory));
        for entry in walkdir::WalkDir::new(source_root)
            .min_depth(1)
            .follow_links(false)
        {
            let entry = entry?;
            let file_type = entry.file_type();
            let kind = if file_type.is_dir() {
                LifecycleSharedStateKind::Directory
            } else if file_type.is_file() || file_type.is_symlink() {
                LifecycleSharedStateKind::File
            } else {
                bail!(
                    "shared lifecycle source has unsupported type: {}",
                    entry.path().display()
                )
            };
            sources.push((entry.into_path(), kind));
        }
    }
    sources.sort();
    Ok(sources)
}

fn validate_shared_mapping(keg: &Path, mapping: &LifecycleSharedState) -> bool {
    ["etc", "var"].into_iter().any(|root| {
        mapping
            .source
            .strip_prefix(keg.join(".bottle").join(root))
            .ok()
            .is_some_and(|relative| {
                let target = prefix::prefix().join(root).join(relative);
                let default = shared_default_path(&target);
                match mapping.kind {
                    LifecycleSharedStateKind::File => {
                        mapping.target == target || mapping.target == default
                    }
                    LifecycleSharedStateKind::Directory => mapping.target == target,
                }
            })
    })
}

fn shared_mapping_satisfied(mapping: &LifecycleSharedState) -> Result<bool> {
    match mapping.kind {
        LifecycleSharedStateKind::File => match symlink_metadata_if_exists(&mapping.target)? {
            Some(metadata) if metadata.file_type().is_symlink() => node_exists(&mapping.target),
            Some(metadata) => Ok(metadata.file_type().is_file()),
            None => Ok(false),
        },
        LifecycleSharedStateKind::Directory => Ok(symlink_metadata_if_exists(&mapping.target)?
            .is_some_and(|metadata| metadata.file_type().is_dir())),
    }
}

fn shared_default_source_exists(keg: &Path, required: &Path) -> Result<bool> {
    for root in ["etc", "var"] {
        let Ok(relative) = required.strip_prefix(prefix::prefix().join(root)) else {
            continue;
        };
        return Ok(
            symlink_metadata_if_exists(&keg.join(".bottle").join(root).join(relative))?.is_some(),
        );
    }
    Ok(false)
}

fn validate_shared_state(keg: &Path, state: &LifecycleState) -> Result<()> {
    let mut recorded_sources = BTreeSet::new();
    let mut recorded_targets = BTreeSet::new();
    for mapping in &state.shared_state {
        if !validate_shared_mapping(keg, mapping) || !state.required_paths.contains(&mapping.target)
        {
            bail!(
                "unowned source-to-target mapping {} -> {}",
                mapping.source.display(),
                mapping.target.display()
            )
        }
        if !recorded_sources.insert((mapping.source.clone(), mapping.kind))
            || !recorded_targets.insert(mapping.target.clone())
        {
            bail!("duplicate shared-state source or target mapping")
        }
    }
    let current_sources = shared_authorities(keg)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    if recorded_sources != current_sources {
        bail!("recorded shared-state sources do not match current keg")
    }
    Ok(())
}

fn shared_missing_paths(keg: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    shared_authorities(keg)?
        .into_iter()
        .filter_map(|(source, kind)| {
            ["etc", "var"].into_iter().find_map(|root| {
                let relative = source.strip_prefix(keg.join(".bottle").join(root)).ok()?;
                let target = prefix::prefix().join(root).join(relative);
                let mapping = LifecycleSharedState {
                    source: source.clone(),
                    target: target.clone(),
                    kind,
                };
                match shared_mapping_satisfied(&mapping) {
                    Ok(false) => Some(Ok((source.clone(), target))),
                    Ok(true) => None,
                    Err(error) => Some(Err(error)),
                }
            })
        })
        .collect::<Result<Vec<_>>>()
}

pub(super) async fn install(
    prepared: &PreparedFormulaLifecycle,
    predecessor_keg: Option<&Path>,
) -> Result<()> {
    let keg = &prepared.keg;
    let state_path = state_path(keg);
    let install_identity = capture_install_identity(prepared)?;
    write_state(
        &state_path,
        &LifecycleState {
            complete: false,
            phase: LifecyclePhase::Initial,
            install_identity: Some(install_identity.clone()),
            shared_state: vec![],
            symlinks: vec![],
            required_paths: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        },
    )?;
    let mut shared_state = vec![];
    let mut symlinks = vec![];
    let mut required_paths = vec![];
    let mut absent_patterns = vec![];
    let mut permissions = vec![];
    let result: Result<()> = async {
        for root in ["etc", "var"] {
            let installed = install_shared_tree(
                &prepared.formula,
                root,
                &keg.join(".bottle").join(root),
                &prefix::prefix().join(root),
                predecessor_keg,
            )?;
            required_paths.extend(installed.iter().map(|mapping| mapping.target.clone()));
            shared_state.extend(installed);
        }
        write_state(
            &state_path,
            &LifecycleState {
                complete: false,
                phase: LifecyclePhase::SharedState,
                install_identity: Some(install_identity.clone()),
                shared_state: shared_state.clone(),
                symlinks: vec![],
                required_paths: required_paths.clone(),
                absent_patterns: vec![],
                permissions: vec![],
                repair: None,
            },
        )?;
        for step in &prepared.steps {
            let effects = execute_step(prepared, step).await?;
            merge_step_effects(
                &mut symlinks,
                &mut required_paths,
                &mut absent_patterns,
                &mut permissions,
                effects,
            )?;
        }
        Ok(())
    }
    .await;
    if result.is_ok() {
        for permission in &permissions {
            validate_permission_identity(&permission.path, &permission.identity).wrap_err_with(
                || {
                    format!(
                        "brew:{} permission target changed before lifecycle completion: {}",
                        prepared.formula,
                        permission.path.display()
                    )
                },
            )?;
        }
        write_state(
            &state_path,
            &LifecycleState {
                complete: true,
                phase: LifecyclePhase::Complete,
                install_identity: Some(install_identity),
                shared_state,
                symlinks,
                required_paths,
                absent_patterns,
                permissions,
                repair: None,
            },
        )?;
    }
    result
}

/// Repair only effects whose ownership and retry behavior are proven. Legacy
/// mise receipts without lifecycle state may run the complete typed plan once,
/// after its embedded formula snapshot is matched to the authoritative API
/// checksum. An interrupted lifecycle is never replayed because a `run` or
/// removal step may already have taken effect.
pub(super) async fn repair(
    prepared: &PreparedFormulaLifecycle,
    mise_owned: bool,
    dry_run: bool,
) -> Result<bool> {
    if !preflight_repair(prepared, mise_owned)? {
        return Ok(false);
    }
    let keg = &prepared.keg;
    let initial_health = health(keg, mise_owned);
    match &initial_health {
        LifecycleHealth::Healthy | LifecycleHealth::ReinstallRequired(_) => unreachable!(),
        LifecycleHealth::Repairable(reasons) if dry_run => {
            miseprintln!(
                "repair {}/{}: {}",
                prepared.formula,
                keg.file_name().unwrap_or_default().to_string_lossy(),
                reasons.join("; ")
            );
            return Ok(true);
        }
        LifecycleHealth::Repairable(_) => {}
    }

    let path = state_path(keg);
    let Some(mut state) = read_state_if_present(&path)? else {
        validate_legacy_formula_snapshot(prepared)?;
        install(prepared, None).await?;
        return Ok(true);
    };
    validate_install_identity(keg, &state).wrap_err_with(|| {
        format!(
            "brew:{} requires reinstall: lifecycle state belongs to another install",
            prepared.formula
        )
    })?;
    validate_shared_state(keg, &state)?;
    if !state.complete || state.phase != LifecyclePhase::Complete {
        bail!(
            "brew:{} requires reinstall: lifecycle completion is unknown",
            prepared.formula
        );
    }
    let mut journal = match state.repair.take() {
        Some(journal) => journal,
        None => LifecycleRepairJournal {
            effects: repair_effects(&state)?,
            next: 0,
        },
    };
    validate_repair_journal(keg, &state, &journal)?;
    preflight_repair_effects(&journal.effects)?;
    state.repair = Some(journal.clone());
    write_state(&path, &state)?;
    while journal.next < journal.effects.len() {
        apply_repair_effect(&journal.effects[journal.next])?;
        journal.next += 1;
        state.repair = Some(journal.clone());
        write_state(&path, &state)?;
    }
    state.repair = None;
    write_state(&path, &state)?;
    match health(keg, true) {
        LifecycleHealth::Healthy => Ok(true),
        LifecycleHealth::Repairable(reasons) | LifecycleHealth::ReinstallRequired(reasons) => {
            bail!(
                "brew:{} lifecycle remains unhealthy after repair: {}",
                prepared.formula,
                reasons.join("; ")
            )
        }
    }
}

pub(super) fn preflight_repair(
    prepared: &PreparedFormulaLifecycle,
    mise_owned: bool,
) -> Result<bool> {
    let keg = &prepared.keg;
    match health(keg, mise_owned) {
        LifecycleHealth::Healthy => return Ok(false),
        LifecycleHealth::ReinstallRequired(reasons) => bail!(
            "brew:{} requires reinstall: {}",
            prepared.formula,
            reasons.join("; ")
        ),
        LifecycleHealth::Repairable(_) => {}
    }
    let path = state_path(keg);
    let Some(state) = read_state_if_present(&path)? else {
        validate_legacy_formula_snapshot(prepared)?;
        return Ok(true);
    };
    validate_install_identity(keg, &state).wrap_err_with(|| {
        format!(
            "brew:{} requires reinstall: lifecycle state belongs to another install",
            prepared.formula
        )
    })?;
    validate_shared_state(keg, &state)?;
    if !state.complete || state.phase != LifecyclePhase::Complete {
        bail!(
            "brew:{} requires reinstall: lifecycle completion is unknown",
            prepared.formula
        );
    }
    let journal = match state.repair.clone() {
        Some(journal) => journal,
        None => LifecycleRepairJournal {
            effects: repair_effects(&state)?,
            next: 0,
        },
    };
    validate_repair_journal(keg, &state, &journal)?;
    preflight_repair_effects(&journal.effects)?;
    Ok(true)
}

pub(super) fn requires_legacy_snapshot_evidence(prepared: &PreparedFormulaLifecycle) -> bool {
    matches!(
        symlink_metadata_if_exists(&state_path(&prepared.keg)),
        Ok(None)
    )
}

fn validate_legacy_formula_snapshot(prepared: &PreparedFormulaLifecycle) -> Result<()> {
    validate_lifecycle_keg_ancestry(&prepared.keg)?;
    let expected = prepared
        .formula_snapshot_sha256
        .as_deref()
        .ok_or_else(|| eyre!("authoritative formula snapshot checksum is unavailable"))?;
    let snapshot = prepared
        .keg
        .join(".brew")
        .join(format!("{}.rb", prepared.formula));
    if !snapshot.is_file() {
        bail!(
            "brew:{} requires reinstall: formula snapshot is missing at {}",
            prepared.formula,
            snapshot.display()
        )
    }
    let actual = crate::hash::file_hash_sha256(&snapshot, None)?;
    if actual != expected {
        bail!(
            "brew:{} requires reinstall: formula snapshot checksum does not match lifecycle metadata",
            prepared.formula
        )
    }
    Ok(())
}

fn repair_effects(state: &LifecycleState) -> Result<Vec<LifecycleRepairEffect>> {
    let mut effects = vec![];
    for mapping in &state.shared_state {
        if !shared_mapping_satisfied(mapping)? {
            effects.push(match mapping.kind {
                LifecycleSharedStateKind::File => LifecycleRepairEffect::Copy {
                    source: mapping.source.clone(),
                    target: mapping.target.clone(),
                },
                LifecycleSharedStateKind::Directory => LifecycleRepairEffect::CreateDirectory {
                    source: mapping.source.clone(),
                    target: mapping.target.clone(),
                },
            });
        }
    }
    for link in &state.symlinks {
        if resolved_symlink_target_checked(&link.target)?.as_ref() != Some(&link.source) {
            effects.push(LifecycleRepairEffect::Symlink {
                source: link.source.clone(),
                target: link.target.clone(),
            });
        }
    }
    for permission in &state.permissions {
        if !permission_identity_matches(permission)? {
            bail!(
                "post-install permission target identity changed: {}",
                permission.path.display()
            )
        }
        if !permission_satisfied(&permission.path, permission.permission)? {
            effects.push(LifecycleRepairEffect::SetPermissions {
                path: permission.path.clone(),
                permission: permission.permission,
                identity: permission.identity.clone(),
            });
        }
    }
    Ok(effects)
}

fn validate_repair_journal(
    keg: &Path,
    state: &LifecycleState,
    journal: &LifecycleRepairJournal,
) -> Result<()> {
    if journal.next > journal.effects.len() {
        bail!("lifecycle repair journal has an invalid checkpoint")
    }
    for effect in &journal.effects {
        match effect {
            LifecycleRepairEffect::Copy { source, target } => {
                let mapping_matches = state.shared_state.iter().any(|mapping| {
                    mapping.source == *source
                        && mapping.target == *target
                        && mapping.kind == LifecycleSharedStateKind::File
                });
                if !mapping_matches
                    || !validate_shared_mapping(
                        keg,
                        &LifecycleSharedState {
                            source: source.clone(),
                            target: target.clone(),
                            kind: LifecycleSharedStateKind::File,
                        },
                    )
                    || !state.required_paths.contains(target)
                {
                    bail!(
                        "lifecycle repair journal contains an unowned shared-state effect: {}",
                        target.display()
                    )
                }
            }
            LifecycleRepairEffect::CreateDirectory { source, target } => {
                let mapping_matches = state.shared_state.iter().any(|mapping| {
                    mapping.source == *source
                        && mapping.target == *target
                        && mapping.kind == LifecycleSharedStateKind::Directory
                });
                if !mapping_matches
                    || !validate_shared_mapping(
                        keg,
                        &LifecycleSharedState {
                            source: source.clone(),
                            target: target.clone(),
                            kind: LifecycleSharedStateKind::Directory,
                        },
                    )
                    || !state.required_paths.contains(target)
                {
                    bail!(
                        "lifecycle repair journal contains an unowned shared-state directory effect: {}",
                        target.display()
                    )
                }
            }
            LifecycleRepairEffect::Symlink { source, target } => {
                if !state
                    .symlinks
                    .iter()
                    .any(|link| link.source == *source && link.target == *target)
                {
                    bail!(
                        "lifecycle repair journal contains an unowned symlink effect: {}",
                        target.display()
                    )
                }
            }
            LifecycleRepairEffect::SetPermissions {
                path,
                permission,
                identity,
            } => {
                if !state.permissions.iter().any(|expected| {
                    expected.path == *path
                        && expected.permission == *permission
                        && expected.identity == *identity
                }) {
                    bail!(
                        "lifecycle repair journal contains an unowned permission effect: {}",
                        path.display()
                    )
                }
            }
        }
    }
    Ok(())
}

fn preflight_repair_effects(effects: &[LifecycleRepairEffect]) -> Result<()> {
    for effect in effects {
        match effect {
            LifecycleRepairEffect::Copy { source, target } => {
                validate_repair_copy_source(source)?;
                let target_metadata = symlink_metadata_if_exists(target)?;
                ensure_runtime_write_path(target, false)?;
                match target_metadata {
                    None => {}
                    Some(_) if files_equal(source, target)? => {}
                    Some(_) => bail!(
                        "lifecycle repair target has ambiguous ownership: {}",
                        target.display()
                    ),
                }
            }
            LifecycleRepairEffect::CreateDirectory { source, target } => {
                let source_metadata = source.symlink_metadata().wrap_err_with(|| {
                    format!(
                        "lifecycle repair directory source is missing: {}",
                        source.display()
                    )
                })?;
                if !source_metadata.file_type().is_dir() {
                    bail!(
                        "lifecycle repair directory source has ambiguous type: {}",
                        source.display()
                    )
                }
                let target_metadata = symlink_metadata_if_exists(target)?;
                ensure_runtime_write_path(target, false)?;
                match target_metadata {
                    None => {}
                    Some(metadata) if metadata.file_type().is_dir() => {}
                    Some(_) => bail!(
                        "lifecycle repair directory target has ambiguous ownership: {}",
                        target.display()
                    ),
                }
            }
            LifecycleRepairEffect::Symlink { source, target } => {
                if !node_exists(source)? {
                    bail!(
                        "lifecycle repair symlink source is missing: {}",
                        source.display()
                    )
                }
                let target_metadata = symlink_metadata_if_exists(target)?;
                ensure_runtime_write_path(target, false)?;
                match target_metadata {
                    None => {}
                    Some(_)
                        if resolved_symlink_target_checked(target)?.as_ref() == Some(source) => {}
                    Some(_) => bail!(
                        "lifecycle repair target has ambiguous ownership: {}",
                        target.display()
                    ),
                }
            }
            LifecycleRepairEffect::SetPermissions { path, identity, .. } => {
                validate_permission_identity(path, identity).wrap_err_with(|| {
                    format!(
                        "lifecycle repair permission target is missing or ambiguous: {}",
                        path.display()
                    )
                })?;
            }
        }
    }
    Ok(())
}

fn apply_repair_effect(effect: &LifecycleRepairEffect) -> Result<()> {
    match effect {
        LifecycleRepairEffect::Copy { source, target } => {
            validate_repair_copy_source(source)?;
            let target_metadata = symlink_metadata_if_exists(target)?;
            ensure_runtime_write_path(target, false)?;
            match target_metadata {
                None => atomic_copy_missing(source, target)?,
                Some(_) if files_equal(source, target)? => {}
                Some(_) => bail!(
                    "lifecycle repair target has ambiguous ownership: {}",
                    target.display()
                ),
            }
        }
        LifecycleRepairEffect::CreateDirectory { source, target } => {
            let source_metadata = source.symlink_metadata()?;
            if !source_metadata.file_type().is_dir() {
                bail!(
                    "lifecycle repair directory source has ambiguous type: {}",
                    source.display()
                )
            }
            let target_metadata = symlink_metadata_if_exists(target)?;
            ensure_runtime_write_path(target, false)?;
            match target_metadata {
                None => crate::file::create_dir_all(target)?,
                Some(metadata) if metadata.file_type().is_dir() => {}
                Some(_) => bail!(
                    "lifecycle repair directory target has ambiguous ownership: {}",
                    target.display()
                ),
            }
        }
        LifecycleRepairEffect::Symlink { source, target } => {
            if !node_exists(source)? {
                bail!(
                    "lifecycle repair symlink source is missing: {}",
                    source.display()
                )
            }
            let target_metadata = symlink_metadata_if_exists(target)?;
            ensure_runtime_write_path(target, false)?;
            match target_metadata {
                None => {
                    crate::file::create_dir_all(target.parent().unwrap())?;
                    crate::file::make_symlink(source, target)?;
                }
                Some(_) if resolved_symlink_target_checked(target)?.as_ref() == Some(source) => {}
                Some(_) => bail!(
                    "lifecycle repair target has ambiguous ownership: {}",
                    target.display()
                ),
            }
        }
        LifecycleRepairEffect::SetPermissions {
            path,
            permission,
            identity,
        } => {
            validate_permission_identity(path, identity)?;
            apply_permission(path, *permission)?;
            validate_permission_identity(path, identity)?;
        }
    }
    Ok(())
}

fn validate_repair_copy_source(source: &Path) -> Result<()> {
    let Some(metadata) = symlink_metadata_if_exists(source)? else {
        bail!("lifecycle repair source is missing: {}", source.display())
    };
    if metadata.file_type().is_file() || (metadata.file_type().is_symlink() && node_exists(source)?)
    {
        return Ok(());
    }
    bail!(
        "lifecycle repair source has ambiguous type: {}",
        source.display()
    )
}

fn atomic_copy_missing(source: &Path, destination: &Path) -> Result<()> {
    crate::file::create_dir_all(destination.parent().unwrap())?;
    let temporary = tempfile::Builder::new()
        .prefix(".mise-lifecycle-repair-")
        .tempfile_in(destination.parent().unwrap())?
        .into_temp_path();
    let metadata = source.symlink_metadata()?;
    if metadata.file_type().is_symlink() {
        fs::remove_file(&temporary)?;
        crate::file::make_symlink(&fs::read_link(source)?, &temporary)?;
    } else if metadata.file_type().is_file() {
        fs::copy(source, &temporary)?;
        fs::set_permissions(&temporary, metadata.permissions())?;
    } else {
        bail!("copy source has unsupported type: {}", source.display())
    }
    temporary
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

fn state_path(keg: &Path) -> PathBuf {
    let identity = crate::hash::hash_to_str(&(
        prefix::prefix(),
        keg.parent().and_then(Path::file_name),
        keg.file_name(),
    ));
    crate::dirs::STATE
        .join("brew-formula-lifecycle")
        .join(format!("{identity}.json"))
}

fn state_directory_identity(path: &Path) -> Result<Option<DirectoryAncestry>> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("lifecycle state path has no parent: {}", path.display()))?;
    if symlink_metadata_if_exists(parent)?.is_none() {
        return Ok(None);
    }
    Ok(Some(capture_directory_ancestry(parent).wrap_err_with(
        || {
            format!(
                "lifecycle state directory ancestry is invalid: {}",
                parent.display()
            )
        },
    )?))
}

fn ensure_real_directory_chain(path: &Path) -> Result<DirectoryAncestry> {
    let path = super::pour::lexical_normalize(path);
    if !path.is_absolute() {
        bail!("directory path is not absolute: {}", path.display())
    }
    let mut paths = path.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
    paths.reverse();
    for path in paths {
        match symlink_metadata_if_exists(&path)? {
            None => fs::create_dir(&path)?,
            Some(metadata)
                if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
            Some(_) => bail!("directory path is not a real directory: {}", path.display()),
        }
    }
    capture_directory_ancestry(&path)
}

fn ensure_state_directory(path: &Path) -> Result<DirectoryAncestry> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("lifecycle state path has no parent: {}", path.display()))?;
    ensure_real_directory_chain(parent).wrap_err_with(|| {
        format!(
            "could not establish trusted lifecycle state directory: {}",
            parent.display()
        )
    })
}

fn validate_state_directory(path: &Path, expected: Option<&DirectoryAncestry>) -> Result<()> {
    match (state_directory_identity(path)?, expected) {
        (None, None) => {}
        (Some(actual), Some(expected)) if &actual == expected => {
            validate_directory_ancestry(expected)?;
        }
        _ => bail!("lifecycle state directory changed after preflight"),
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn test_state_path(keg: &Path) -> PathBuf {
    state_path(keg)
}

pub(super) fn prepare_remove_owned_state(keg: &Path) -> Result<PreparedLifecycleRemoval> {
    let path = state_path(keg);
    let state_directory = state_directory_identity(&path)?;
    let Some(metadata) = symlink_metadata_if_exists(&path)? else {
        return Ok(PreparedLifecycleRemoval {
            keg: keg.to_path_buf(),
            state_path: path,
            state_directory,
            keg_ancestry: None,
            state_sha256: None,
            symlinks: vec![],
            disposition: LifecycleRemovalDisposition::Absent,
        });
    };
    validate_lifecycle_keg_ancestry(keg)?;
    let keg_ancestry = capture_directory_ancestry(&keg.join(".brew"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!(
            "refusing to remove non-file lifecycle state: {}",
            path.display()
        )
    }
    let state_sha256 = immutable_file_sha256(&path, "private state")?;
    let parsed = read_state_if_present(&path);
    if let Ok(Some(state)) = &parsed
        && validate_install_identity(keg, state).is_ok()
    {
        let symlinks = prepare_lifecycle_symlink_removals(state)?;
        return Ok(PreparedLifecycleRemoval {
            keg: keg.to_path_buf(),
            state_path: path,
            state_directory,
            keg_ancestry: Some(keg_ancestry),
            state_sha256: Some(state_sha256),
            symlinks,
            disposition: LifecycleRemovalDisposition::CurrentMise,
        });
    }

    if symlink_metadata_if_exists(&identity_marker_path(keg))?.is_some() {
        let mismatch = parsed
            .err()
            .map(|error| format!(": {error}"))
            .unwrap_or_default();
        bail!("refusing to remove lifecycle state not bound to the current install{mismatch}")
    }
    let receipt = read_proven_native_receipt_identity(keg)
        .wrap_err("could not prove stale lifecycle state belongs to a native replacement")?;
    Ok(PreparedLifecycleRemoval {
        keg: keg.to_path_buf(),
        state_path: path,
        state_directory,
        keg_ancestry: Some(keg_ancestry),
        state_sha256: Some(state_sha256),
        symlinks: vec![],
        disposition: LifecycleRemovalDisposition::ProvenNativeStale {
            receipt: Box::new(receipt),
        },
    })
}

pub(super) fn remove_owned_state_prepared(prepared: PreparedLifecycleRemoval) -> Result<()> {
    match &prepared.disposition {
        LifecycleRemovalDisposition::Absent => {
            if symlink_metadata_if_exists(&prepared.state_path)?.is_some() {
                bail!("lifecycle state appeared after removal preflight")
            }
        }
        LifecycleRemovalDisposition::CurrentMise => {
            validate_prepared_keg_ancestry(&prepared)?;
            validate_state_directory(&prepared.state_path, prepared.state_directory.as_ref())?;
            validate_prepared_state(&prepared)?;
            let state = read_state_if_present(&prepared.state_path)?
                .ok_or_else(|| eyre!("lifecycle state disappeared after removal preflight"))?;
            validate_install_identity(&prepared.keg, &state)
                .wrap_err("lifecycle install identity changed after removal preflight")?;
            remove_prepared_lifecycle_symlinks(&prepared.symlinks)?;
            validate_prepared_state(&prepared)?;
            validate_prepared_keg_ancestry(&prepared)?;
            validate_install_identity(&prepared.keg, &state)
                .wrap_err("lifecycle install identity changed during state removal")?;
            crate::file::remove_file(&prepared.state_path)?;
            validate_prepared_keg_ancestry(&prepared)?;
            validate_install_identity(&prepared.keg, &state)
                .wrap_err("lifecycle install identity changed before marker removal")?;
            crate::file::remove_file(identity_marker_path(&prepared.keg))?;
        }
        LifecycleRemovalDisposition::ProvenNativeStale { receipt } => {
            validate_prepared_keg_ancestry(&prepared)?;
            validate_state_directory(&prepared.state_path, prepared.state_directory.as_ref())?;
            validate_prepared_state(&prepared)?;
            if symlink_metadata_if_exists(&identity_marker_path(&prepared.keg))?.is_some() {
                bail!("lifecycle install-incarnation marker appeared after removal preflight")
            }
            if &read_proven_native_receipt_identity(&prepared.keg)? != receipt.as_ref() {
                bail!("native install receipt changed after lifecycle removal preflight")
            }
            validate_prepared_state(&prepared)?;
            validate_prepared_keg_ancestry(&prepared)?;
            crate::file::remove_file(&prepared.state_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn remove_owned_state(keg: &Path) -> Result<()> {
    remove_owned_state_prepared(prepare_remove_owned_state(keg)?)
}

fn validate_prepared_state(prepared: &PreparedLifecycleRemoval) -> Result<()> {
    validate_state_directory(&prepared.state_path, prepared.state_directory.as_ref())?;
    let expected = prepared
        .state_sha256
        .as_deref()
        .ok_or_else(|| eyre!("prepared lifecycle removal has no state identity"))?;
    if immutable_file_sha256(&prepared.state_path, "private state")? != expected {
        bail!("lifecycle state changed after removal preflight")
    }
    Ok(())
}

fn validate_prepared_keg_ancestry(prepared: &PreparedLifecycleRemoval) -> Result<()> {
    let expected = prepared
        .keg_ancestry
        .as_ref()
        .ok_or_else(|| eyre!("prepared lifecycle removal has no keg ancestry identity"))?;
    validate_directory_ancestry(expected)
}

fn prepare_lifecycle_symlink_removals(
    state: &LifecycleState,
) -> Result<Vec<PreparedLifecycleSymlinkRemoval>> {
    let mut prepared = vec![];
    for link in &state.symlinks {
        let Some(metadata) = symlink_metadata_if_exists(&link.target)? else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let target = fs::read_link(&link.target)?;
        let resolved = if target.is_absolute() {
            target.clone()
        } else {
            link.target
                .parent()
                .ok_or_else(|| eyre!("symlink has no parent: {}", link.target.display()))?
                .join(&target)
        };
        if super::pour::lexical_normalize(&resolved) != link.source {
            continue;
        }
        let (device, inode) = permission_device_inode(&metadata)?;
        let current = link.target.symlink_metadata()?;
        if !current.file_type().is_symlink()
            || permission_device_inode(&current)? != (device, inode)
            || fs::read_link(&link.target)? != target
        {
            bail!(
                "lifecycle symlink changed during removal preflight: {}",
                link.target.display()
            )
        }
        let parent = link
            .target
            .parent()
            .ok_or_else(|| eyre!("lifecycle symlink has no parent: {}", link.target.display()))?;
        prepared.push(PreparedLifecycleSymlinkRemoval {
            path: link.target.clone(),
            ancestry: capture_directory_ancestry(parent)?,
            device,
            inode,
            target,
        });
    }
    Ok(prepared)
}

fn remove_prepared_lifecycle_symlinks(symlinks: &[PreparedLifecycleSymlinkRemoval]) -> Result<()> {
    for link in symlinks {
        let Some(metadata) = symlink_metadata_if_exists(&link.path)? else {
            // Maintenance may already have removed the same public link.
            continue;
        };
        validate_directory_ancestry(&link.ancestry)?;
        if !metadata.file_type().is_symlink()
            || permission_device_inode(&metadata)? != (link.device, link.inode)
            || fs::read_link(&link.path)? != link.target
        {
            bail!(
                "lifecycle symlink changed after removal preflight: {}",
                link.path.display()
            )
        }
        crate::file::remove_file(&link.path)?;
    }
    Ok(())
}

#[cfg(test)]
fn remove_lifecycle_symlinks(state: &LifecycleState) -> Result<()> {
    remove_prepared_lifecycle_symlinks(&prepare_lifecycle_symlink_removals(state)?)
}

fn write_state(path: &Path, state: &LifecycleState) -> Result<()> {
    let directory = ensure_state_directory(path)?;
    if let Some(metadata) = symlink_metadata_if_exists(path)?
        && (!metadata.file_type().is_file() || metadata.file_type().is_symlink())
    {
        bail!(
            "refusing to overwrite non-file lifecycle state: {}",
            path.display()
        )
    }
    validate_state_directory(path, Some(&directory))?;
    crate::file::write_atomic(path, serde_json::to_vec_pretty(state)?)?;
    validate_state_directory(path, Some(&directory))
}

fn read_state_if_present(path: &Path) -> Result<Option<LifecycleState>> {
    let Some(directory) = state_directory_identity(path)? else {
        return Ok(None);
    };
    let Some(metadata) = symlink_metadata_if_exists(path)? else {
        return Ok(None);
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!("lifecycle state is not a regular file: {}", path.display())
    }
    let contents = crate::file::read_to_string(path)
        .wrap_err_with(|| format!("could not read lifecycle state at {}", path.display()))?;
    let state = serde_json::from_str(&contents)
        .wrap_err_with(|| format!("lifecycle state is malformed at {}", path.display()))?;
    validate_state_directory(path, Some(&directory))?;
    Ok(Some(state))
}

fn install_shared_tree(
    formula: &str,
    root: &str,
    source_root: &Path,
    destination_root: &Path,
    predecessor_keg: Option<&Path>,
) -> Result<Vec<LifecycleSharedState>> {
    match source_root.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Err(error) => return Err(error.into()),
        Ok(_) => bail!(
            "brew:{formula} .bottle/{root} source has ambiguous type: {}",
            source_root.display()
        ),
    }
    install_shared_directory(destination_root, destination_root)?;
    let mut installed_paths = vec![LifecycleSharedState {
        source: source_root.to_path_buf(),
        target: destination_root.to_path_buf(),
        kind: LifecycleSharedStateKind::Directory,
    }];
    for entry in walkdir::WalkDir::new(source_root)
        .min_depth(1)
        .follow_links(false)
    {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source_root)?;
        let destination = destination_root.join(relative);
        if entry.file_type().is_dir() {
            install_shared_directory(destination_root, &destination)?;
            installed_paths.push(LifecycleSharedState {
                source: entry.path().to_path_buf(),
                target: destination,
                kind: LifecycleSharedStateKind::Directory,
            });
            continue;
        }
        let destination = install_destination(
            formula,
            root,
            entry.path(),
            relative,
            &destination,
            predecessor_keg,
        )?;
        atomic_copy_with_authority(entry.path(), &destination.path, &destination.authority)?;
        installed_paths.push(LifecycleSharedState {
            source: entry.path().to_path_buf(),
            target: destination.path,
            kind: LifecycleSharedStateKind::File,
        });
    }
    Ok(installed_paths)
}

fn install_shared_directory(destination_root: &Path, destination: &Path) -> Result<()> {
    let relative = destination
        .strip_prefix(destination_root)
        .wrap_err_with(|| {
            format!(
                "shared lifecycle directory target escapes {}: {}",
                destination_root.display(),
                destination.display()
            )
        })?;
    let mut current = destination_root.to_path_buf();
    install_shared_directory_component(&current)?;
    for component in relative.components() {
        current.push(component.as_os_str());
        install_shared_directory_component(&current)?;
    }
    Ok(())
}

fn install_shared_directory_component(path: &Path) -> Result<()> {
    match path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
        }
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Err(error) => return Err(error.into()),
        Ok(_) => bail!(
            "shared lifecycle directory target has ambiguous ownership: {}",
            path.display()
        ),
    }
    Ok(())
}

fn install_destination(
    formula: &str,
    root: &str,
    source: &Path,
    relative: &Path,
    destination: &Path,
    predecessor_keg: Option<&Path>,
) -> Result<InstallDestination> {
    let destination_metadata = symlink_metadata_if_exists(destination)?;
    let authority = match destination_metadata {
        None => {
            return Ok(InstallDestination {
                path: destination.to_path_buf(),
                authority: AtomicCopyTargetAuthority::Missing,
            });
        }
        Some(metadata) if metadata.file_type().is_file() || metadata.file_type().is_symlink() => {
            atomic_copy_target_authority(destination)?
        }
        Some(_) => AtomicCopyTargetAuthority::Missing,
    };
    match &authority {
        AtomicCopyTargetAuthority::Existing(_) if files_equal(source, destination)? => {
            return Ok(InstallDestination {
                path: destination.to_path_buf(),
                authority,
            });
        }
        AtomicCopyTargetAuthority::Existing(_) => {}
        AtomicCopyTargetAuthority::Missing => {}
    }
    if let Some(predecessor_keg) = predecessor_keg {
        let old_default = predecessor_keg.join(".bottle").join(root).join(relative);
        if symlink_metadata_if_exists(&old_default)?.is_some()
            && files_equal(&old_default, destination)?
        {
            return Ok(InstallDestination {
                path: destination.to_path_buf(),
                authority,
            });
        }
    }
    let default = shared_default_path(destination);
    debug!(
        "brew:{} preserving modified {}; writing new default to {}",
        formula,
        destination.display(),
        default.display()
    );
    Ok(InstallDestination {
        authority: atomic_copy_target_authority(&default)?,
        path: default,
    })
}

fn shared_default_path(destination: &Path) -> PathBuf {
    let mut default = destination.as_os_str().to_os_string();
    default.push(".default");
    default.into()
}

struct InstallDestination {
    path: PathBuf,
    authority: AtomicCopyTargetAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AtomicCopyTargetAuthority {
    Missing,
    Existing(AtomicCopyTargetIdentity),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AtomicCopyTargetIdentity {
    File {
        sha256: String,
        device: u64,
        inode: u64,
    },
    Symlink {
        target: PathBuf,
        device: u64,
        inode: u64,
    },
}

fn atomic_copy_target_authority(destination: &Path) -> Result<AtomicCopyTargetAuthority> {
    let Some(metadata) = symlink_metadata_if_exists(destination)? else {
        return Ok(AtomicCopyTargetAuthority::Missing);
    };
    let (device, inode) = permission_device_inode(&metadata)?;
    if metadata.file_type().is_file() {
        return Ok(AtomicCopyTargetAuthority::Existing(
            AtomicCopyTargetIdentity::File {
                sha256: crate::hash::file_hash_sha256(destination, None)?,
                device,
                inode,
            },
        ));
    }
    if metadata.file_type().is_symlink() {
        return Ok(AtomicCopyTargetAuthority::Existing(
            AtomicCopyTargetIdentity::Symlink {
                target: fs::read_link(destination)?,
                device,
                inode,
            },
        ));
    }
    bail!("copy target has ambiguous type: {}", destination.display())
}

pub(super) fn atomic_copy(source: &Path, destination: &Path) -> Result<()> {
    let authority = atomic_copy_target_authority(destination)?;
    atomic_copy_with_authority(source, destination, &authority)
}

fn atomic_copy_with_authority(
    source: &Path,
    destination: &Path,
    authority: &AtomicCopyTargetAuthority,
) -> Result<()> {
    crate::file::create_dir_all(destination.parent().unwrap())?;
    let temporary = tempfile::Builder::new()
        .prefix(".mise-lifecycle-copy-")
        .tempfile_in(destination.parent().unwrap())?
        .into_temp_path();
    let metadata = source.symlink_metadata()?;
    if metadata.file_type().is_symlink() {
        fs::remove_file(&temporary)?;
        crate::file::make_symlink(&fs::read_link(source)?, &temporary)?;
    } else if metadata.file_type().is_file() {
        fs::copy(source, &temporary)?;
        fs::set_permissions(&temporary, metadata.permissions())?;
    } else {
        bail!("copy source has unsupported type: {}", source.display())
    }
    persist_staged_node(temporary, destination, authority)
}

fn atomic_symlink_with_authority(
    source: &Path,
    destination: &Path,
    authority: &AtomicCopyTargetAuthority,
) -> Result<()> {
    crate::file::create_dir_all(destination.parent().unwrap())?;
    let temporary = tempfile::Builder::new()
        .prefix(".mise-lifecycle-symlink-")
        .tempfile_in(destination.parent().unwrap())?
        .into_temp_path();
    fs::remove_file(&temporary)?;
    crate::file::make_symlink(source, &temporary)?;
    persist_staged_node(temporary, destination, authority)
}

fn persist_staged_node(
    temporary: tempfile::TempPath,
    destination: &Path,
    authority: &AtomicCopyTargetAuthority,
) -> Result<()> {
    match authority {
        AtomicCopyTargetAuthority::Missing => {
            temporary
                .persist_noclobber(destination)
                .map_err(|error| error.error)?;
        }
        AtomicCopyTargetAuthority::Existing(expected) => {
            if atomic_copy_target_authority(destination)?
                != AtomicCopyTargetAuthority::Existing(expected.clone())
            {
                bail!(
                    "copy target changed after lifecycle preflight: {}",
                    destination.display()
                )
            }
            temporary
                .persist(destination)
                .map_err(|error| error.error)?;
        }
    }
    Ok(())
}

fn files_equal(left: &Path, right: &Path) -> Result<bool> {
    let left_metadata = left.symlink_metadata()?;
    let right_metadata = right.symlink_metadata()?;
    if left_metadata.file_type().is_symlink() && right_metadata.file_type().is_symlink() {
        return Ok(fs::read_link(left)? == fs::read_link(right)?);
    }
    if left_metadata.is_file()
        && right_metadata.is_file()
        && left_metadata.len() == right_metadata.len()
    {
        return Ok(fs::read(left)? == fs::read(right)?);
    }
    Ok(false)
}

#[derive(Default)]
struct StepEffects {
    symlinks: Vec<LifecycleSymlink>,
    required_paths: Vec<PathBuf>,
    absent_patterns: Vec<String>,
    permissions: Vec<LifecyclePermission>,
    removed_paths: Vec<(PathBuf, bool)>,
}

/// Fold ordered lifecycle effects into final-state invariants. Homebrew
/// formulae commonly remove an old tree and recreate it later in the same
/// post-install block (Node's npm tree is one example). Recording every
/// intermediate removal as a permanent invariant makes healthy final state
/// look broken. Conversely, a later removal must retire earlier output
/// invariants.
fn merge_step_effects(
    symlinks: &mut Vec<LifecycleSymlink>,
    required_paths: &mut Vec<PathBuf>,
    absent_patterns: &mut Vec<String>,
    permissions: &mut Vec<LifecyclePermission>,
    effects: StepEffects,
) -> Result<()> {
    for (removed, recursive) in &effects.removed_paths {
        let removes = |path: &Path| path == removed || (*recursive && path.starts_with(removed));
        symlinks.retain(|link| !removes(&link.target));
        required_paths.retain(|path| !removes(path));
        permissions.retain(|permission| !removes(&permission.path));
    }
    absent_patterns.extend(effects.absent_patterns);

    for created in effects
        .required_paths
        .iter()
        .chain(effects.symlinks.iter().map(|link| &link.target))
    {
        absent_patterns.retain(|pattern| !created_path_supersedes_absence(pattern, created));
    }
    symlinks.extend(effects.symlinks);
    required_paths.extend(effects.required_paths);
    for permission in effects.permissions {
        if let Some(existing) = permissions.iter_mut().find(|existing| {
            existing.path == permission.path && existing.permission == permission.permission
        }) {
            *existing = permission;
        } else {
            permissions.push(permission);
        }
    }
    Ok(())
}

fn created_path_supersedes_absence(pattern: &str, created: &Path) -> bool {
    if !has_glob_magic(pattern) {
        let removed = Path::new(pattern);
        return created == removed || created.starts_with(removed);
    }
    glob::Pattern::new(pattern).is_ok_and(|pattern| pattern.matches_path(created))
}

async fn execute_step(
    prepared: &PreparedFormulaLifecycle,
    step: &PreparedStep,
) -> Result<StepEffects> {
    let guards = match step {
        PreparedStep::Mkdir { guards, .. }
        | PreparedStep::Remove { guards, .. }
        | PreparedStep::Copy { guards, .. }
        | PreparedStep::Symlink { guards, .. }
        | PreparedStep::SetPermissions { guards, .. } => guards,
        PreparedStep::Run(run) => &run.guards,
    };
    if !guards_match(guards)? {
        return Ok(StepEffects::default());
    }
    match step {
        PreparedStep::Mkdir { path, .. } => {
            ensure_runtime_write_path(path, true)?;
            crate::file::create_dir_all(path)?;
            Ok(StepEffects {
                required_paths: vec![path.clone()],
                ..Default::default()
            })
        }
        PreparedStep::Remove {
            paths,
            recursive,
            symlink_target_contains,
            ..
        } => {
            let mut absent_patterns = vec![];
            let mut removed_paths = vec![];
            for pattern in paths {
                absent_patterns.extend(pattern.patterns.clone());
                for path in expand_pattern(pattern)? {
                    ensure_runtime_write_path(&path, false)?;
                    remove_prepared_node(&path, *recursive, symlink_target_contains.as_deref())?;
                    removed_paths.push((path, *recursive));
                }
            }
            Ok(StepEffects {
                absent_patterns,
                removed_paths,
                ..Default::default()
            })
        }
        PreparedStep::Copy {
            sources,
            target,
            recursive,
            ..
        } => {
            let sources = resolve_sources(sources)?;
            let target_is_directory = symlink_metadata_if_exists(target)?
                .is_some_and(|metadata| metadata.file_type().is_dir());
            let directory_target = sources.len() > 1 || target_is_directory;
            ensure_runtime_write_path(target, directory_target)?;
            if directory_target {
                crate::file::create_dir_all(target)?;
            }
            let mut required_paths = vec![];
            for source in sources {
                let destination = if directory_target {
                    target.join(required_file_name(&source)?)
                } else {
                    target.clone()
                };
                ensure_runtime_write_path(&destination, false)?;
                let package_owned = path_is_within_keg(&prepared.keg, &destination);
                if *recursive {
                    required_paths.extend(copy_recursive(&source, &destination, package_owned)?);
                } else if package_owned {
                    atomic_copy(&source, &destination)?;
                } else {
                    atomic_copy_missing(&source, &destination)?;
                    required_paths.push(destination);
                }
            }
            Ok(StepEffects {
                required_paths,
                ..Default::default()
            })
        }
        PreparedStep::Symlink {
            sources,
            target,
            force,
            ..
        } => {
            let sources = resolve_sources(sources)?;
            if sources.is_empty() {
                return Ok(StepEffects::default());
            }
            let directory_target = sources.len() > 1 || target.is_dir();
            ensure_runtime_write_path(target, directory_target)?;
            if directory_target {
                crate::file::create_dir_all(target)?;
            }
            let mut links = vec![];
            for source in sources {
                let destination = if directory_target {
                    target.join(required_file_name(&source)?)
                } else {
                    target.clone()
                };
                ensure_runtime_write_path(&destination, false)?;
                let source = super::pour::lexical_normalize(&source);
                let authority = atomic_copy_target_authority(&destination)?;
                if matches!(authority, AtomicCopyTargetAuthority::Existing(_))
                    && !force
                    && resolved_symlink_target_checked(&destination)?.as_ref() != Some(&source)
                {
                    bail!(
                        "post-install target already exists: {}",
                        destination.display()
                    );
                }
                // Formula DSL `ln_s`/`ln_sf` receives the resolved source
                // path. These lifecycle links are absolute; only keg/public
                // topology uses Homebrew's relative-link convention.
                atomic_symlink_with_authority(&source, &destination, &authority)?;
                links.push(LifecycleSymlink {
                    source,
                    target: destination,
                });
            }
            Ok(StepEffects {
                symlinks: links,
                ..Default::default()
            })
        }
        PreparedStep::SetPermissions {
            paths,
            permission,
            non_recursive,
            ..
        } => {
            let mut targets = BTreeSet::new();
            for pattern in paths {
                for path in expand_pattern(pattern)? {
                    targets.insert(path.clone());
                    if !non_recursive && path.is_dir() {
                        for entry in walkdir::WalkDir::new(&path).follow_links(false) {
                            targets.insert(entry?.path().to_path_buf());
                        }
                    }
                }
            }
            let mut permissions = Vec::with_capacity(targets.len());
            for path in targets {
                apply_permission(&path, *permission)?;
                permissions.push(LifecyclePermission {
                    identity: capture_permission_identity(&path)?,
                    path,
                    permission: *permission,
                });
            }
            Ok(StepEffects {
                permissions,
                ..Default::default()
            })
        }
        PreparedStep::Run(run) => execute_run(prepared, run).await,
    }
}

fn path_is_within_keg(keg: &Path, path: &Path) -> bool {
    super::pour::lexical_normalize(path).starts_with(super::pour::lexical_normalize(keg))
}

fn permission_target(path: &Path) -> Result<PathBuf> {
    let metadata = path.symlink_metadata()?;
    let target = if metadata.file_type().is_symlink() {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };
    ensure_runtime_write_path(&target, true)?;
    Ok(target)
}

fn capture_permission_identity(path: &Path) -> Result<LifecyclePermissionIdentity> {
    let metadata = path.symlink_metadata()?;
    let target = permission_target(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(LifecyclePermissionIdentity::Symlink {
            target: target.clone(),
            target_identity: Box::new(capture_direct_permission_identity(&target)?),
        });
    }
    capture_direct_permission_identity(&target)
}

fn capture_direct_permission_identity(path: &Path) -> Result<LifecyclePermissionIdentity> {
    let metadata = path.symlink_metadata()?;
    let (device, inode) = permission_device_inode(&metadata)?;
    if metadata.file_type().is_file() {
        return Ok(LifecyclePermissionIdentity::RegularFile {
            sha256: crate::hash::file_hash_sha256(path, None)?,
            device,
            inode,
        });
    }
    if metadata.file_type().is_dir() {
        return Ok(LifecyclePermissionIdentity::Directory { device, inode });
    }
    bail!(
        "post-install permission target has unsupported type: {}",
        path.display()
    )
}

#[cfg(unix)]
fn permission_device_inode(metadata: &fs::Metadata) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn permission_device_inode(_metadata: &fs::Metadata) -> Result<(u64, u64)> {
    bail!("formula lifecycle permissions require Unix identity semantics")
}

fn permission_identity_matches(permission: &LifecyclePermission) -> Result<bool> {
    Ok(capture_permission_identity(&permission.path)? == permission.identity)
}

fn validate_permission_identity(path: &Path, expected: &LifecyclePermissionIdentity) -> Result<()> {
    if capture_permission_identity(path)? != *expected {
        bail!(
            "post-install permission target identity changed: {}",
            path.display()
        )
    }
    Ok(())
}

fn apply_permission(path: &Path, permission: LifecyclePermissionKind) -> Result<()> {
    let target = permission_target(path)?;
    apply_permission_unchecked(&target, permission)
}

#[cfg(unix)]
fn apply_permission_unchecked(path: &Path, permission: LifecyclePermissionKind) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = path.metadata()?.permissions();
    let mode = match permission {
        LifecyclePermissionKind::UserWrite => permissions.mode() | 0o200,
    };
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn apply_permission_unchecked(_path: &Path, _permission: LifecyclePermissionKind) -> Result<()> {
    bail!("formula lifecycle permissions require Unix permission semantics")
}

#[cfg(unix)]
fn permission_satisfied(path: &Path, permission: LifecyclePermissionKind) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = path.metadata()?;
    Ok(match permission {
        LifecyclePermissionKind::UserWrite => metadata.permissions().mode() & 0o200 != 0,
    })
}

#[cfg(not(unix))]
fn permission_satisfied(_path: &Path, _permission: LifecyclePermissionKind) -> Result<bool> {
    Ok(false)
}

fn remove_prepared_node(
    path: &Path,
    recursive: bool,
    symlink_target_contains: Option<&str>,
) -> Result<()> {
    let Some(metadata) = symlink_metadata_if_exists(path)? else {
        return Ok(());
    };
    if let Some(required) = symlink_target_contains
        && (!metadata.file_type().is_symlink()
            || !fs::read_link(path)?.to_string_lossy().contains(required))
    {
        return Ok(());
    }
    if metadata.file_type().is_symlink() || !recursive {
        crate::file::remove_file(path)?;
    } else {
        crate::file::remove_all(path)?;
    }
    Ok(())
}

fn guards_match(guards: &[PreparedGuard]) -> Result<bool> {
    for guard in guards {
        let matches = match guard {
            PreparedGuard::IfExists(pattern) => paths_exist(&expand_pattern(pattern)?)?,
            PreparedGuard::UnlessExists(pattern) => !paths_exist(&expand_pattern(pattern)?)?,
            PreparedGuard::Platform(matches) => *matches,
        };
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

fn paths_exist(paths: &[PathBuf]) -> Result<bool> {
    for path in paths {
        if node_exists(path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn execute_run(
    prepared: &PreparedFormulaLifecycle,
    run: &PreparedRun,
) -> Result<StepEffects> {
    let temp = crate::dirs::CACHE
        .join("system-brew")
        .join("post-install")
        .join(crate::hash::hash_to_str(&(
            &prepared.formula,
            &prepared.keg,
            run.step_index,
        )));
    crate::file::create_dir_all(&temp)?;
    crate::file::create_dir_all(&run.log_dir)?;
    let stdout_log = run.log_dir.join(format!("{}.stdout.log", run.step_index));
    let stderr_log = run.log_dir.join(format!("{}.stderr.log", run.step_index));

    let (stdout, stdout_publish) = match &run.stdout_path {
        Some(path) => {
            ensure_runtime_write_path(path, false)?;
            prepare_run_stdout(prepared, path)?
        }
        None => (open_truncated(&stdout_log)?, None),
    };
    let stderr = open_truncated(&stderr_log)?;
    let stdin = match &run.stdin_path {
        Some(path) => Stdio::from(File::open(path)?),
        None => Stdio::null(),
    };

    let shared_write_targets = run_shared_write_targets(run)?;
    let env = run_environment(prepared, run, &temp)?;
    let mut allow_write = vec![prepared.keg.clone(), run.log_dir.clone(), temp.clone()];
    allow_write.extend(shared_write_targets.iter().cloned());
    if let Some(publish) = &stdout_publish {
        allow_write.push(publish.temporary.to_path_buf());
    }
    let mut sandbox = SandboxConfig {
        deny_write: true,
        deny_net: true,
        deny_env: true,
        allow_write,
        deny_system_temp_write: true,
        ..Default::default()
    };
    sandbox.resolve_paths();
    let mut command = CmdLineRunner::new(&run.executable)
        .args(&run.args)
        .with_sandbox(sandbox);
    command.apply_sandbox().await?;
    command = command
        .env_clear()
        .envs(&env)
        .stdin(stdin)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if let Some(cwd) = &run.cwd {
        command = command.current_dir(cwd);
    }
    if let Err(error) = command.execute_async().await {
        let stdout = log_tail(
            stdout_publish
                .as_ref()
                .map(|publish| publish.temporary.as_ref())
                .unwrap_or(&stdout_log),
        )?;
        let stderr = log_tail(&stderr_log)?;
        return Err(error).wrap_err_with(|| {
            format!(
                "brew:{} post-install run step {} failed\nstdout tail:\n{}\nstderr tail:\n{}",
                prepared.formula, run.step_index, stdout, stderr
            )
        });
    }

    if let Some(publish) = stdout_publish {
        persist_staged_node(publish.temporary, &publish.destination, &publish.authority)?;
    }
    let mut required_paths = vec![];
    for path in shared_write_targets {
        if symlink_metadata_if_exists(&path)?.is_some() {
            required_paths.push(path);
        }
    }
    if let Some(path) = &run.stdout_path {
        required_paths.push(path.clone());
    }
    Ok(StepEffects {
        required_paths,
        ..Default::default()
    })
}

fn run_shared_write_targets(run: &PreparedRun) -> Result<BTreeSet<PathBuf>> {
    let shared = prefix::prefix();
    let mut targets = BTreeSet::new();
    for argument in &run.args {
        let path = super::pour::lexical_normalize(Path::new(argument));
        if path.starts_with(&shared) {
            ensure_runtime_write_path(&path, true)?;
            targets.insert(path);
        }
    }
    Ok(targets)
}

struct RunStdoutPublish {
    temporary: tempfile::TempPath,
    destination: PathBuf,
    authority: AtomicCopyTargetAuthority,
}

fn prepare_run_stdout(
    prepared: &PreparedFormulaLifecycle,
    destination: &Path,
) -> Result<(File, Option<RunStdoutPublish>)> {
    let authority = atomic_copy_target_authority(destination)?;
    if !path_is_within_keg(&prepared.keg, destination)
        && matches!(authority, AtomicCopyTargetAuthority::Existing(_))
    {
        bail!(
            "post-install stdout target has unproven ownership: {}",
            destination.display()
        )
    }
    if matches!(
        authority,
        AtomicCopyTargetAuthority::Existing(AtomicCopyTargetIdentity::Symlink { .. })
    ) {
        bail!(
            "post-install stdout target is an ambiguous symlink: {}",
            destination.display()
        )
    }
    crate::file::create_dir_all(destination.parent().unwrap())?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".mise-lifecycle-stdout-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let temporary = builder.tempfile_in(destination.parent().unwrap())?;
    if matches!(authority, AtomicCopyTargetAuthority::Existing(_)) {
        temporary
            .as_file()
            .set_permissions(destination.metadata()?.permissions())?;
    }
    let stdout = temporary.reopen()?;
    Ok((
        stdout,
        Some(RunStdoutPublish {
            temporary: temporary.into_temp_path(),
            destination: destination.to_path_buf(),
            authority,
        }),
    ))
}

fn run_environment(
    prepared: &PreparedFormulaLifecycle,
    run: &PreparedRun,
    temp: &Path,
) -> Result<BTreeMap<String, String>> {
    let shared = prefix::prefix();
    let path = std::env::join_paths([
        prepared.keg.join("bin"),
        prepared.keg.join("sbin"),
        shared.join("bin"),
        shared.join("sbin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ])?;
    let mut env = BTreeMap::from([
        ("HOME".to_string(), temp.to_string_lossy().into_owned()),
        ("LANG".to_string(), "C".to_string()),
        ("LC_ALL".to_string(), "C".to_string()),
        ("PATH".to_string(), path.to_string_lossy().into_owned()),
        (
            "HOMEBREW_PREFIX".to_string(),
            shared.to_string_lossy().into_owned(),
        ),
        (
            "HOMEBREW_CELLAR".to_string(),
            prefix::cellar().to_string_lossy().into_owned(),
        ),
        ("TMPDIR".to_string(), temp.to_string_lossy().into_owned()),
    ]);
    env.extend(run.env.clone());
    Ok(env)
}

fn open_truncated(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?)
}

fn log_tail(path: &Path) -> Result<String> {
    if !path.is_file() {
        return Ok(String::new());
    }
    let bytes = fs::read(path)?;
    let start = bytes.len().saturating_sub(MAX_FAILURE_LOG_BYTES);
    Ok(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

fn resolve_sources(sources: &PreparedSources) -> Result<Vec<PathBuf>> {
    match sources {
        PreparedSources::One(path) => Ok(vec![path.clone()]),
        PreparedSources::Glob(pattern) => expand_pattern(pattern),
    }
}

fn expand_pattern(pattern: &PreparedPattern) -> Result<Vec<PathBuf>> {
    let mut paths = vec![];
    for pattern in &pattern.patterns {
        if has_glob_magic(pattern) {
            for path in glob::glob(pattern)? {
                paths.push(path?);
            }
        } else {
            paths.push(PathBuf::from(pattern));
        }
    }
    Ok(paths)
}

fn required_file_name(path: &Path) -> Result<&std::ffi::OsStr> {
    path.file_name()
        .ok_or_else(|| eyre!("post-install source has no file name: {}", path.display()))
}

fn ensure_runtime_write_path(path: &Path, include_final: bool) -> Result<()> {
    ensure_contained(path, &prefix::cellar(), PathAccess::Write)?;
    let shared = super::pour::lexical_normalize(&prefix::prefix());
    let normalized = super::pour::lexical_normalize(path);
    let relative = normalized.strip_prefix(&shared)?;
    let components = relative.components().collect::<Vec<_>>();
    let count = if include_final {
        components.len()
    } else {
        components.len().saturating_sub(1)
    };
    let mut current = shared;
    for component in components.into_iter().take(count) {
        current.push(component);
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                "post-install write path traverses symlink {}",
                current.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum RecursiveCopyTargetIdentity {
    Node(AtomicCopyTargetIdentity),
    Directory { device: u64, inode: u64 },
}

fn recursive_copy_target_identity(path: &Path) -> Result<Option<RecursiveCopyTargetIdentity>> {
    let Some(metadata) = symlink_metadata_if_exists(path)? else {
        return Ok(None);
    };
    if metadata.file_type().is_dir() {
        let (device, inode) = permission_device_inode(&metadata)?;
        return Ok(Some(RecursiveCopyTargetIdentity::Directory {
            device,
            inode,
        }));
    }
    let AtomicCopyTargetAuthority::Existing(identity) = atomic_copy_target_authority(path)? else {
        unreachable!("existing recursive copy target returned missing authority")
    };
    Ok(Some(RecursiveCopyTargetIdentity::Node(identity)))
}

fn copy_recursive(source: &Path, target: &Path, package_owned: bool) -> Result<Vec<PathBuf>> {
    let destination = target.to_path_buf();
    let source_metadata = source.symlink_metadata()?;
    if !source_metadata.file_type().is_dir() {
        let authority = atomic_copy_target_authority(&destination)?;
        if !package_owned && matches!(authority, AtomicCopyTargetAuthority::Existing(_)) {
            bail!(
                "post-install recursive copy target has unproven ownership: {}",
                destination.display()
            )
        }
        atomic_copy_with_authority(source, &destination, &authority)?;
        return Ok(vec![destination]);
    }
    let entries = walkdir::WalkDir::new(source)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .map(|entry| {
            let entry = entry?;
            if !entry.file_type().is_dir()
                && !entry.file_type().is_file()
                && !entry.file_type().is_symlink()
            {
                bail!(
                    "post-install recursive copy source has unsupported type: {}",
                    entry.path().display()
                )
            }
            Ok(entry)
        })
        .collect::<Result<Vec<_>>>()?;
    let target_identity = recursive_copy_target_identity(&destination)?;
    if target_identity.is_some() && !package_owned {
        bail!(
            "post-install recursive copy target has unproven ownership: {}",
            destination.display()
        )
    }
    crate::file::create_dir_all(destination.parent().unwrap())?;
    let staging = tempfile::Builder::new()
        .prefix(".mise-lifecycle-tree-")
        .tempdir_in(destination.parent().unwrap())?;
    let mut outputs = vec![destination.clone()];
    for entry in entries {
        let relative = entry.path().strip_prefix(source)?;
        let staged_output = staging.path().join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir(&staged_output)?;
        } else {
            atomic_copy_with_authority(
                entry.path(),
                &staged_output,
                &AtomicCopyTargetAuthority::Missing,
            )?;
        }
        outputs.push(destination.join(relative));
    }
    fs::set_permissions(staging.path(), source_metadata.permissions())?;
    match target_identity {
        None => rename_noclobber(staging.path(), &destination)?,
        Some(expected) => {
            if recursive_copy_target_identity(&destination)?.as_ref() != Some(&expected) {
                bail!(
                    "post-install recursive copy target changed before replacement: {}",
                    destination.display()
                )
            }
            rename_exchange(staging.path(), &destination)?;
        }
    }
    Ok(outputs)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn path_to_c_string(path: &Path) -> Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| eyre!("path contains a NUL byte: {}", path.display()))
}

#[cfg(target_os = "linux")]
fn rename_noclobber(source: &Path, destination: &Path) -> Result<()> {
    let source = path_to_c_string(source)?;
    let destination = path_to_c_string(destination)?;
    let result = unsafe {
        nix::libc::syscall(
            nix::libc::SYS_renameat2,
            nix::libc::AT_FDCWD,
            source.as_ptr(),
            nix::libc::AT_FDCWD,
            destination.as_ptr(),
            nix::libc::RENAME_NOREPLACE,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn rename_exchange(source: &Path, destination: &Path) -> Result<()> {
    let source = path_to_c_string(source)?;
    let destination = path_to_c_string(destination)?;
    let result = unsafe {
        nix::libc::syscall(
            nix::libc::SYS_renameat2,
            nix::libc::AT_FDCWD,
            source.as_ptr(),
            nix::libc::AT_FDCWD,
            destination.as_ptr(),
            nix::libc::RENAME_EXCHANGE,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn rename_noclobber(source: &Path, destination: &Path) -> Result<()> {
    let source = path_to_c_string(source)?;
    let destination = path_to_c_string(destination)?;
    let result = unsafe {
        nix::libc::renamex_np(
            source.as_ptr(),
            destination.as_ptr(),
            nix::libc::RENAME_EXCL,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn rename_exchange(source: &Path, destination: &Path) -> Result<()> {
    let source = path_to_c_string(source)?;
    let destination = path_to_c_string(destination)?;
    let result = unsafe {
        nix::libc::renamex_np(
            source.as_ptr(),
            destination.as_ptr(),
            nix::libc::RENAME_SWAP,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn rename_noclobber(_source: &Path, _destination: &Path) -> Result<()> {
    bail!("atomic recursive lifecycle copy is unsupported on this platform")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn rename_exchange(_source: &Path, _destination: &Path) -> Result<()> {
    bail!("atomic recursive lifecycle replacement is unsupported on this platform")
}

fn resolved_symlink_target_checked(path: &Path) -> Result<Option<PathBuf>> {
    let Some(metadata) = symlink_metadata_if_exists(path)? else {
        return Ok(None);
    };
    if !metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let target = fs::read_link(path)?;
    let target = if target.is_absolute() {
        target
    } else {
        path.parent()
            .ok_or_else(|| eyre!("symlink has no parent: {}", path.display()))?
            .join(target)
    };
    Ok(Some(super::pour::lexical_normalize(&target)))
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close_offset) = pattern[open + 1..].find('}') else {
        return vec![pattern.to_string()];
    };
    let close = open + 1 + close_offset;
    let mut expanded = vec![];
    for choice in pattern[open + 1..close].split(',') {
        let value = format!("{}{}{}", &pattern[..open], choice, &pattern[close + 1..]);
        expanded.extend(expand_braces(&value));
    }
    expanded
}

fn expand_templates(formula: &Formula, keg: &Path, value: &str) -> Result<String> {
    let mut output = value.to_string();
    for token in [
        "HOMEBREW_PREFIX",
        "HOMEBREW_CELLAR",
        "prefix",
        "opt_prefix",
        "bin",
        "sbin",
        "lib",
        "libexec",
        "share",
        "pkgshare",
        "var",
        "etc",
        "pkgetc",
        "bash_completion",
        "zsh_completion",
        "fish_completion",
        "pwsh_completion",
    ] {
        let replacement = template_base(formula, keg, token)?;
        output = output.replace(&format!("{{{{{token}}}}}"), &replacement.to_string_lossy());
    }
    let version = formula.versions.stable.as_deref().unwrap_or_default();
    let mut version_parts = version.split('.');
    let major = version_parts.next().unwrap_or_default();
    let minor = version_parts.next();
    let major_minor = minor.map_or_else(|| major.to_string(), |minor| format!("{major}.{minor}"));
    output = output.replace("{{version.major_minor}}", &major_minor);
    output = output.replace("{{version.major}}", major);
    output = output.replace("{{formula_name}}", &formula.name);
    if output.contains("{{") {
        bail!("unsupported post-install template in {value:?}");
    }
    Ok(output)
}

fn template_base(formula: &Formula, keg: &Path, base: &str) -> Result<PathBuf> {
    let shared = prefix::prefix();
    match base {
        "HOMEBREW_PREFIX" | "homebrew_prefix" => Ok(shared),
        "HOMEBREW_CELLAR" => Ok(prefix::cellar()),
        "prefix" => Ok(keg.to_path_buf()),
        "opt_prefix" => Ok(shared.join("opt").join(&formula.name)),
        "bin" | "sbin" | "lib" | "libexec" | "share" => Ok(keg.join(base)),
        "frameworks" => Ok(keg.join("Frameworks")),
        "pkgshare" => Ok(keg.join("share").join(&formula.name)),
        "var" | "etc" => Ok(shared.join(base)),
        "pkgetc" => Ok(shared.join("etc").join(&formula.name)),
        "bash_completion" => Ok(keg.join("etc/bash_completion.d")),
        "zsh_completion" => Ok(keg.join("share/zsh/site-functions")),
        "fish_completion" => Ok(keg.join("share/fish/vendor_completions.d")),
        "pwsh_completion" => Ok(keg.join("share/pwsh/completions")),
        _ => bail!("unsupported post-install path base {base:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formula(steps: Vec<Value>) -> Formula {
        serde_json::from_value(serde_json::json!({
            "name": "openssl@3",
            "versions": {"stable": "1"},
            "bottle": {},
            "post_install_steps": steps
        }))
        .unwrap()
    }

    #[test]
    fn rejects_unsupported_steps_before_install() {
        let error = prepare(
            &formula(vec![serde_json::json!({"type": "touch"})]),
            &prefix::cellar().join("openssl@3/1"),
        )
        .unwrap_err();
        let error = format!("{error:?}");
        assert!(error.contains("unsupported type"));
        assert!(error.contains("no package state was changed"));
    }

    #[test]
    fn accepts_ca_certificates_and_openssl_steps() {
        let ca = formula(vec![serde_json::json!({
            "command": {"base": "libexec", "path": "post-install"},
            "type": "run",
            "args": ["{{pkgshare}}/cacert.pem", "{{pkgetc}}/cert.pem"]
        })]);
        let openssl = formula(vec![serde_json::json!({
            "source": {"path": "{{etc}}/ca-certificates/cert.pem"},
            "target": {"path": "{{pkgetc}}/cert.pem"},
            "force": true,
            "type": "symlink"
        })]);
        let keg = prefix::cellar().join("openssl@3/1");
        prepare(&ca, &keg).unwrap();
        prepare(&openssl, &keg).unwrap();
    }

    #[test]
    fn accepts_python_venv_permission_steps() {
        let python = formula(vec![serde_json::json!({
            "paths": [{
                "base": "frameworks",
                "path": "Python.framework/Versions/{{version.major_minor}}/lib/python{{version.major_minor}}/venv/scripts/**/*"
            }],
            "permissions": "u+w",
            "non_recursive": true,
            "guards": [{"condition": "on", "value": "macos", "id": "1"}],
            "type": "set_permissions"
        })]);
        prepare(&python, &prefix::cellar().join("python@3.14/1")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn user_write_permission_is_idempotent() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("script");
        crate::file::write(&path, "script")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;
        assert!(!permission_satisfied(
            &path,
            LifecyclePermissionKind::UserWrite
        )?);
        apply_permission_unchecked(&path, LifecyclePermissionKind::UserWrite)?;
        apply_permission_unchecked(&path, LifecyclePermissionKind::UserWrite)?;
        assert!(permission_satisfied(
            &path,
            LifecyclePermissionKind::UserWrite
        )?);
        assert_eq!(path.metadata()?.permissions().mode() & 0o777, 0o644);
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn permission_repair_rejects_replacement_at_same_path() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        let target = keg.join("Frameworks/script");
        crate::file::create_dir_all(snapshot.parent().unwrap())?;
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&snapshot, "class OpensslAT3; end")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":false,"installed_as_dependency":true,"built_as_bottle":true,"poured_from_bottle":true,"time":123,"source_modified_time":100,"arch":"arm64","source":{"spec":"stable","versions":{"stable":"1","head":null,"version_scheme":0},"path":"/api/formula.jws.json","tap":"homebrew/core","tap_git_head":"core-head"}}"#,
        )?;
        crate::file::write(&target, "owned")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444))?;
        let prepared = prepare(
            &formula(vec![serde_json::json!({
                "paths": [{"base": "frameworks", "path": "script"}],
                "permissions": "u+w",
                "non_recursive": true,
                "type": "set_permissions"
            })]),
            &keg,
        )?;
        install(&prepared, None).await?;
        assert!(permission_satisfied(
            &target,
            LifecyclePermissionKind::UserWrite
        )?);

        fs::set_permissions(&target, fs::Permissions::from_mode(0o444))?;
        assert!(matches!(health(&keg, true), LifecycleHealth::Repairable(_)));
        assert!(repair(&prepared, true, false).await?);
        assert!(permission_satisfied(
            &target,
            LifecyclePermissionKind::UserWrite
        )?);

        crate::file::remove_file(&target)?;
        crate::file::write(&target, "foreign")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444))?;
        assert!(matches!(
            health(&keg, true),
            LifecycleHealth::ReinstallRequired(_)
        ));
        assert!(preflight_repair(&prepared, true).is_err());
        assert!(repair(&prepared, true, false).await.is_err());
        assert_eq!(crate::file::read_to_string(&target)?, "foreign");
        assert_eq!(target.metadata()?.permissions().mode() & 0o777, 0o444);
        remove_owned_state(&keg)?;
        Ok(())
    }

    #[test]
    fn legacy_bottle_snapshot_can_differ_from_current_tap_source() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut env = crate::test::EnvVarGuard::new();
        let prefix = crate::file::desymlink_path(tmp.path());
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        crate::file::create_dir_all(snapshot.parent().unwrap())?;
        crate::file::write(&snapshot, "bottle build-time formula\n")?;
        let bottle_sha256 = crate::hash::file_hash_sha256(&snapshot, None)?;

        let mut formula = formula(vec![]);
        formula.ruby_source_checksum = Some(super::super::api::RubySourceChecksum {
            sha256: Some("current-tap-source-checksum".to_string()),
        });
        let mut prepared = prepare(&formula, &keg)?;
        assert!(validate_legacy_formula_snapshot(&prepared).is_err());
        prepared.set_formula_snapshot_sha256(bottle_sha256);
        validate_legacy_formula_snapshot(&prepared)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn native_replacement_cannot_apply_stale_mise_lifecycle_state() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        let shared_source = keg.join(".bottle/etc/openssl@3/openssl.cnf");
        let shared_target = prefix.join("etc/openssl@3/openssl.cnf");
        let link_source = keg.join("bin/openssl");
        let link_target = prefix.join("bin/openssl");
        for path in [&snapshot, &shared_source, &shared_target, &link_source] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        crate::file::write(&snapshot, "class OpensslAT3; end")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":false,"installed_as_dependency":true,"built_as_bottle":true,"poured_from_bottle":true,"time":123,"source_modified_time":100,"arch":"arm64","source":{"spec":"stable","versions":{"stable":"1","head":null,"version_scheme":0},"path":"/api/formula.jws.json","tap":"homebrew/core","tap_git_head":"core-head"}}"#,
        )?;
        crate::file::write(&shared_source, "default")?;
        crate::file::write(&shared_target, "default")?;
        crate::file::write(&link_source, "binary")?;
        let prepared = prepare(&formula(vec![]), &keg)?;
        let identity = capture_install_identity(&prepared)?;
        let state = LifecycleState {
            complete: true,
            phase: LifecyclePhase::Complete,
            install_identity: Some(identity),
            shared_state: vec![LifecycleSharedState {
                source: shared_source,
                target: shared_target.clone(),
                kind: LifecycleSharedStateKind::File,
            }],
            symlinks: vec![LifecycleSymlink {
                source: link_source.clone(),
                target: link_target.clone(),
            }],
            required_paths: vec![shared_target.clone()],
            absent_patterns: vec![],
            permissions: vec![],
            repair: Some(LifecycleRepairJournal {
                effects: vec![LifecycleRepairEffect::Symlink {
                    source: link_source,
                    target: link_target.clone(),
                }],
                next: 0,
            }),
        };
        write_state(&state_path(&keg), &state)?;

        // A marker-preserving mutation is ambiguous even with a native Tab.
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"source":{"tap_git_head":"vendor-head","tap":"vendor/tools","path":"/api/formula.jws.json","versions":{"version_scheme":0,"head":null,"stable":"1"},"spec":"stable"},"arch":"arm64","source_modified_time":100,"time":123,"poured_from_bottle":true,"built_as_bottle":true,"installed_as_dependency":false,"installed_on_request":true,"homebrew_version":"6.0.17"}"#,
        )?;

        assert_eq!(health(&keg, false), LifecycleHealth::Healthy);
        assert!(matches!(
            health(&keg, true),
            LifecycleHealth::ReinstallRequired(_)
        ));
        assert!(!preflight_repair(&prepared, false)?);
        assert!(link_target.symlink_metadata().is_err());
        assert!(remove_owned_state(&keg).is_err());
        assert!(state_path(&keg).is_file());
        crate::file::remove_file(identity_marker_path(&keg))?;

        // A real same-version Homebrew replacement removes the old keg marker.
        // Its stale private state may then be discarded, but no stale effect is
        // applied or removed.
        let original_state = crate::file::read_to_string(state_path(&keg))?;
        let removal = prepare_remove_owned_state(&keg)?;
        crate::file::write(state_path(&keg), format!("{original_state}\n"))?;
        assert!(remove_owned_state_prepared(removal).is_err());
        assert!(state_path(&keg).is_file());
        crate::file::write(state_path(&keg), original_state)?;
        remove_owned_state_prepared(prepare_remove_owned_state(&keg)?)?;
        assert!(state_path(&keg).symlink_metadata().is_err());
        assert!(shared_target.is_file());
        assert!(link_target.symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn native_replacement_ignores_malformed_stale_private_state() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        crate::file::create_dir_all(keg.join(".brew"))?;
        let path = state_path(&keg);
        crate::file::create_dir_all(path.parent().unwrap())?;
        crate::file::write(&path, "malformed stale mise state")?;

        assert_eq!(health(&keg, false), LifecycleHealth::Healthy);
        assert!(matches!(
            health(&keg, true),
            LifecycleHealth::ReinstallRequired(_)
        ));
        assert!(!preflight_repair(&prepare(&formula(vec![]), &keg)?, false)?);
        crate::file::remove_file(path)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_brew_directory_cannot_reuse_copied_identity() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        crate::file::create_dir_all(snapshot.parent().unwrap())?;
        crate::file::write(&snapshot, "class OpensslAT3; end")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17 (mise)","built_as_bottle":true,"poured_from_bottle":true,"time":123,"source_modified_time":100,"arch":"arm64","source":{"spec":"stable","versions":{"stable":"1","head":null,"version_scheme":0},"path":"/api/formula.jws.json","tap":"homebrew/core","tap_git_head":"core-head"}}"#,
        )?;
        let prepared = prepare(&formula(vec![]), &keg)?;
        let state = LifecycleState {
            complete: true,
            phase: LifecyclePhase::Complete,
            install_identity: Some(capture_install_identity(&prepared)?),
            shared_state: vec![],
            symlinks: vec![],
            required_paths: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        };
        let private_state = state_path(&keg);
        write_state(&private_state, &state)?;
        let external_brew = tmp.path().join("external-brew");
        fs::rename(keg.join(".brew"), &external_brew)?;
        crate::file::make_symlink(&external_brew, &keg.join(".brew"))?;

        assert!(matches!(
            health(&keg, true),
            LifecycleHealth::ReinstallRequired(_)
        ));
        assert!(remove_owned_state(&keg).is_err());
        assert!(private_state.is_file());
        assert!(external_brew.join(".mise-lifecycle-incarnation").is_file());

        crate::file::remove_file(private_state)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn install_identity_marker_publish_is_noclobber() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        let external = tmp.path().join("external");
        crate::file::create_dir_all(snapshot.parent().unwrap())?;
        crate::file::write(&snapshot, "class OpensslAT3; end")?;
        crate::file::write(&external, "preserve")?;
        crate::file::make_symlink(&external, &identity_marker_path(&keg))?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17 (mise)","source":{"versions":{"stable":"1"},"tap":"homebrew/core"}}"#,
        )?;

        assert!(capture_install_identity(&prepare(&formula(vec![]), &keg)?).is_err());
        assert_eq!(crate::file::read_to_string(external)?, "preserve");
        assert!(
            identity_marker_path(&keg)
                .symlink_metadata()?
                .file_type()
                .is_symlink()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cellar_ancestry_is_reinstall_required() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let external_cellar = tmp.path().join("external-cellar");
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        crate::file::create_dir_all(&prefix)?;
        crate::file::create_dir_all(external_cellar.join("openssl@3/1/.brew"))?;
        crate::file::make_symlink(&external_cellar, &prefix.join("Cellar"))?;
        let keg = prefix.join("Cellar/openssl@3/1");

        assert!(matches!(
            health(&keg, false),
            LifecycleHealth::ReinstallRequired(_)
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn repair_restores_exact_default_mapping_without_repouring() -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        let source = keg.join(".bottle/etc/openssl@3/openssl.cnf");
        let user_config = prefix.join("etc/openssl@3/openssl.cnf");
        let default = PathBuf::from(format!("{}.default", user_config.display()));
        let keg_binary = keg.join("bin/openssl");
        let public_link = prefix.join("bin/openssl");
        for path in [&snapshot, &source, &user_config, &keg_binary] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        crate::file::write(&snapshot, "class OpensslAT3; end")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":false,"installed_as_dependency":true,"built_as_bottle":true,"poured_from_bottle":true,"time":123,"source_modified_time":100,"arch":"arm64","source":{"spec":"stable","versions":{"stable":"1","head":null,"version_scheme":0},"path":"/api/formula.jws.json","tap":"homebrew/core","tap_git_head":"core-head"}}"#,
        )?;
        crate::file::write(&source, "new-default")?;
        crate::file::write(&user_config, "user-modified")?;
        crate::file::write(&keg_binary, "binary")?;
        crate::file::create_dir_all(public_link.parent().unwrap())?;
        crate::file::make_symlink(&keg_binary, &public_link)?;
        let prepared = prepare(&formula(vec![]), &keg)?;
        install(&prepared, None).await?;
        assert_eq!(crate::file::read_to_string(&default)?, "new-default");

        let keg_inode = keg.metadata()?.ino();
        let receipt_inode = keg.join("INSTALL_RECEIPT.json").metadata()?.ino();
        let public_link_inode = public_link.symlink_metadata()?.ino();
        // Homebrew legitimately rewrites these mutable Tab flags. Key order
        // and unrelated mutable fields are not part of lifecycle authority.
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"source":{"tap_git_head":"core-head","tap":"homebrew/core","path":"/api/formula.jws.json","versions":{"version_scheme":0,"head":null,"stable":"1"},"spec":"stable"},"arch":"arm64","source_modified_time":100,"time":123,"poured_from_bottle":true,"built_as_bottle":true,"installed_as_dependency":false,"installed_on_request":true,"homebrew_version":"6.0.17 (mise)"}"#,
        )?;
        assert_eq!(health(&keg, true), LifecycleHealth::Healthy);
        crate::file::remove_file(&default)?;
        assert!(matches!(health(&keg, true), LifecycleHealth::Repairable(_)));

        assert!(repair(&prepared, true, false).await?);
        assert_eq!(crate::file::read_to_string(&user_config)?, "user-modified");
        assert_eq!(crate::file::read_to_string(&default)?, "new-default");
        assert_eq!(keg.metadata()?.ino(), keg_inode);
        assert_eq!(
            keg.join("INSTALL_RECEIPT.json").metadata()?.ino(),
            receipt_inode
        );
        assert_eq!(public_link.symlink_metadata()?.ino(), public_link_inode);
        assert_eq!(fs::read_link(&public_link)?, keg_binary);
        remove_owned_state(&keg)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn repair_restores_empty_shared_directories_without_repouring() -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        let etc_source = keg.join(".bottle/etc/openssl@3/empty/nested");
        let var_source = keg.join(".bottle/var/openssl@3/cache/empty");
        let etc_target = prefix.join("etc/openssl@3/empty/nested");
        let var_target = prefix.join("var/openssl@3/cache/empty");
        let user_config = prefix.join("etc/openssl@3/user.conf");
        let keg_binary = keg.join("bin/openssl");
        let public_link = prefix.join("bin/openssl");
        for path in [&snapshot, &user_config, &keg_binary] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        for path in [&etc_source, &var_source] {
            crate::file::create_dir_all(path)?;
        }
        crate::file::write(&snapshot, "class OpensslAT3; end")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":false,"installed_as_dependency":true,"built_as_bottle":true,"poured_from_bottle":true,"time":123,"source_modified_time":100,"arch":"arm64","source":{"spec":"stable","versions":{"stable":"1","head":null,"version_scheme":0},"path":"/api/formula.jws.json","tap":"homebrew/core","tap_git_head":"core-head"}}"#,
        )?;
        crate::file::write(&user_config, "user-owned")?;
        crate::file::write(&keg_binary, "binary")?;
        crate::file::create_dir_all(public_link.parent().unwrap())?;
        crate::file::make_symlink(&keg_binary, &public_link)?;

        let prepared = prepare(&formula(vec![]), &keg)?;
        install(&prepared, None).await?;
        let state: LifecycleState =
            serde_json::from_str(&crate::file::read_to_string(state_path(&keg))?)?;
        for (source, target) in [
            (etc_source.clone(), etc_target.clone()),
            (var_source.clone(), var_target.clone()),
        ] {
            assert!(state.shared_state.contains(&LifecycleSharedState {
                source,
                target,
                kind: LifecycleSharedStateKind::Directory,
            }));
        }
        assert!(etc_target.is_dir());
        assert!(var_target.is_dir());

        let keg_inode = keg.metadata()?.ino();
        let receipt_inode = keg.join("INSTALL_RECEIPT.json").metadata()?.ino();
        let public_link_inode = public_link.symlink_metadata()?.ino();
        fs::remove_dir(&etc_target)?;
        fs::remove_dir(&var_target)?;
        assert!(matches!(health(&keg, true), LifecycleHealth::Repairable(_)));

        assert!(repair(&prepared, true, false).await?);
        assert!(etc_target.is_dir());
        assert!(var_target.is_dir());
        assert_eq!(crate::file::read_to_string(&user_config)?, "user-owned");
        assert_eq!(keg.metadata()?.ino(), keg_inode);
        assert_eq!(
            keg.join("INSTALL_RECEIPT.json").metadata()?.ino(),
            receipt_inode
        );
        assert_eq!(public_link.symlink_metadata()?.ino(), public_link_inode);
        assert_eq!(fs::read_link(&public_link)?, keg_binary);
        remove_owned_state(&keg)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shared_directory_type_conflict_fails_closed_before_repair() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        let source = keg.join(".bottle/etc/openssl@3/empty");
        let target = prefix.join("etc/openssl@3/empty");
        let external = tmp.path().join("external");
        let sentinel = external.join("user-data");
        crate::file::create_dir_all(snapshot.parent().unwrap())?;
        crate::file::create_dir_all(&source)?;
        crate::file::create_dir_all(&external)?;
        crate::file::write(&snapshot, "class OpensslAT3; end")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":false,"installed_as_dependency":true,"built_as_bottle":true,"poured_from_bottle":true,"time":123,"source_modified_time":100,"arch":"arm64","source":{"spec":"stable","versions":{"stable":"1","head":null,"version_scheme":0},"path":"/api/formula.jws.json","tap":"homebrew/core","tap_git_head":"core-head"}}"#,
        )?;
        crate::file::write(&sentinel, "preserve")?;

        let prepared = prepare(&formula(vec![]), &keg)?;
        install(&prepared, None).await?;
        fs::remove_dir(&target)?;
        crate::file::make_symlink(&external, &target)?;
        let state_before = crate::file::read_to_string(state_path(&keg))?;

        assert!(matches!(
            health(&keg, true),
            LifecycleHealth::ReinstallRequired(_)
        ));
        let error = preflight_repair(&prepared, true).unwrap_err().to_string();
        assert!(error.contains("ambiguous type"));
        assert!(repair(&prepared, true, false).await.is_err());
        assert_eq!(crate::file::read_to_string(&sentinel)?, "preserve");
        assert!(target.symlink_metadata()?.file_type().is_symlink());
        assert_eq!(crate::file::read_to_string(state_path(&keg))?, state_before);
        remove_owned_state(&keg)?;
        Ok(())
    }

    #[test]
    fn shared_file_mapping_does_not_accept_directory_target() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        crate::file::write(&source, "default")?;
        crate::file::create_dir_all(&target)?;
        assert!(!shared_mapping_satisfied(&LifecycleSharedState {
            source,
            target,
            kind: LifecycleSharedStateKind::File,
        })?);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn shared_file_mapping_does_not_accept_special_target() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("target.sock");
        crate::file::write(&source, "default")?;
        let _socket = std::os::unix::net::UnixListener::bind(&target)?;
        assert!(!shared_mapping_satisfied(&LifecycleSharedState {
            source,
            target,
            kind: LifecycleSharedStateKind::File,
        })?);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn shared_source_special_type_is_rejected() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = tmp.path().join("Cellar/foo/1");
        let socket = keg.join(".bottle/etc/foo.sock");
        crate::file::create_dir_all(socket.parent().unwrap())?;
        let _listener = std::os::unix::net::UnixListener::bind(&socket)?;
        assert!(shared_authorities(&keg).is_err());
        Ok(())
    }

    #[test]
    fn enotdir_is_not_treated_as_absent_state_or_repair_path() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let blocked = tmp.path().join("blocked");
        let source = tmp.path().join("source");
        let missing_target = tmp.path().join("missing-target");
        crate::file::write(&blocked, "not-a-directory")?;
        crate::file::write(&source, "default")?;

        let unreadable = blocked.join("child");
        assert!(read_state_if_present(&unreadable).is_err());
        assert!(symlink_metadata_if_exists(&unreadable).is_err());

        let source_error = LifecycleRepairEffect::Copy {
            source: unreadable.clone(),
            target: missing_target.clone(),
        };
        assert!(preflight_repair_effects(&[source_error]).is_err());
        assert!(missing_target.symlink_metadata().is_err());

        let target_error = LifecycleRepairEffect::Copy {
            source: source.clone(),
            target: unreadable,
        };
        assert!(preflight_repair_effects(&[target_error]).is_err());
        assert_eq!(crate::file::read_to_string(source)?, "default");
        Ok(())
    }

    #[test]
    fn repair_copy_rejects_directory_source() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        crate::file::create_dir_all(&source)?;
        let effect = LifecycleRepairEffect::Copy {
            source,
            target: tmp.path().join("target"),
        };
        let error = preflight_repair_effects(&[effect]).unwrap_err().to_string();
        assert!(error.contains("ambiguous type"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn node_exists_follows_dangling_symlink_chain() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");
        crate::file::make_symlink(&second, &first)?;
        crate::file::make_symlink(&tmp.path().join("missing"), &second)?;
        assert!(!node_exists(&first)?);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_does_not_satisfy_exists_guard() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let dangling = tmp.path().join("dangling");
        crate::file::make_symlink(&tmp.path().join("missing"), &dangling)?;
        assert!(!paths_exist(&[dangling])?);
        Ok(())
    }

    #[test]
    fn comparison_errors_never_authorize_equality() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let blocked = tmp.path().join("blocked");
        crate::file::write(&blocked, "not-a-directory")?;
        assert!(files_equal(&blocked.join("left"), &blocked.join("right")).is_err());
        Ok(())
    }

    #[test]
    fn repair_copy_publish_never_overwrites_existing_target() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        crate::file::write(&source, "default")?;
        crate::file::write(&target, "user-owned")?;
        assert!(atomic_copy_missing(&source, &target).is_err());
        assert_eq!(crate::file::read_to_string(target)?, "user-owned");
        Ok(())
    }

    #[test]
    fn atomic_copy_preserves_old_deterministic_temporary_name() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        let old_temporary = tmp.path().join(".target.mise-new");
        crate::file::write(&source, "new")?;
        crate::file::write(&target, "old")?;
        crate::file::write(&old_temporary, "user-owned")?;

        atomic_copy(&source, &target)?;

        assert_eq!(crate::file::read_to_string(target)?, "new");
        assert_eq!(crate::file::read_to_string(old_temporary)?, "user-owned");
        Ok(())
    }

    #[test]
    fn atomic_copy_missing_authority_rejects_target_race() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        crate::file::write(&source, "new")?;
        let authority = atomic_copy_target_authority(&target)?;
        assert_eq!(authority, AtomicCopyTargetAuthority::Missing);
        crate::file::write(&target, "foreign")?;

        assert!(atomic_copy_with_authority(&source, &target, &authority).is_err());
        assert_eq!(crate::file::read_to_string(target)?, "foreign");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn repair_copy_publish_preserves_symlink_source() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        crate::file::make_symlink(Path::new("relative-payload"), &source)?;
        atomic_copy_missing(&source, &target)?;
        assert!(target.symlink_metadata()?.file_type().is_symlink());
        assert_eq!(fs::read_link(target)?, PathBuf::from("relative-payload"));
        Ok(())
    }

    #[test]
    fn invalid_absent_pattern_is_not_treated_as_satisfied() {
        assert!(pattern_has_matches("[").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn lifecycle_state_atomic_write_rejects_symlink_target() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = crate::file::desymlink_path(tmp.path());
        let external = root.join("external");
        let state_path = root.join("state.json");
        crate::file::write(&external, "preserve")?;
        crate::file::make_symlink(&external, &state_path)?;
        let state = LifecycleState {
            complete: false,
            phase: LifecyclePhase::Initial,
            install_identity: None,
            shared_state: vec![],
            symlinks: vec![],
            required_paths: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        };

        assert!(write_state(&state_path, &state).is_err());
        assert_eq!(crate::file::read_to_string(external)?, "preserve");
        assert!(state_path.symlink_metadata()?.file_type().is_symlink());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn lifecycle_state_rejects_symlinked_parent_directory() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = crate::file::desymlink_path(tmp.path());
        let external = root.join("external");
        let linked_parent = root.join("state-parent");
        crate::file::create_dir_all(&external)?;
        crate::file::make_symlink(&external, &linked_parent)?;
        let state = LifecycleState {
            complete: false,
            phase: LifecyclePhase::Initial,
            install_identity: None,
            shared_state: vec![],
            symlinks: vec![],
            required_paths: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        };

        assert!(write_state(&linked_parent.join("state.json"), &state).is_err());
        assert!(external.read_dir()?.next().is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn lifecycle_state_rejects_symlinked_ancestor_before_parent() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = crate::file::desymlink_path(tmp.path());
        let external = root.join("external");
        let linked_ancestor = root.join("state-root");
        crate::file::create_dir_all(external.join("nested"))?;
        crate::file::make_symlink(&external, &linked_ancestor)?;
        let state = LifecycleState {
            complete: false,
            phase: LifecyclePhase::Initial,
            install_identity: None,
            shared_state: vec![],
            symlinks: vec![],
            required_paths: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        };

        assert!(write_state(&linked_ancestor.join("nested/state.json"), &state).is_err());
        assert!(external.join("nested").read_dir()?.next().is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn removal_token_rejects_state_directory_swap() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = crate::file::desymlink_path(tmp.path());
        let directory = root.join("state");
        let moved = root.join("moved-state");
        let keg = root.join("Cellar/foo/1");
        let state_path = directory.join("state.json");
        crate::file::create_dir_all(&directory)?;
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::write(&state_path, "state")?;
        let prepared = PreparedLifecycleRemoval {
            keg: keg.clone(),
            state_path: state_path.clone(),
            state_directory: state_directory_identity(&state_path)?,
            keg_ancestry: Some(capture_directory_ancestry(&keg.join(".brew"))?),
            state_sha256: Some(crate::hash::file_hash_sha256(&state_path, None)?),
            symlinks: vec![],
            disposition: LifecycleRemovalDisposition::CurrentMise,
        };
        fs::rename(&directory, &moved)?;
        crate::file::create_dir_all(&directory)?;

        assert!(remove_owned_state_prepared(prepared).is_err());
        assert!(directory.is_dir());
        assert!(moved.is_dir());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn removal_token_rejects_same_content_brew_directory_swap() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let snapshot = keg.join(".brew/openssl@3.rb");
        crate::file::create_dir_all(snapshot.parent().unwrap())?;
        crate::file::write(&snapshot, "class OpensslAT3; end")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17 (mise)","source":{"spec":"stable","versions":{"stable":"1"},"tap":"homebrew/core"}}"#,
        )?;
        let prepared = prepare(&formula(vec![]), &keg)?;
        let state = LifecycleState {
            complete: true,
            phase: LifecyclePhase::Complete,
            install_identity: Some(capture_install_identity(&prepared)?),
            shared_state: vec![],
            symlinks: vec![],
            required_paths: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        };
        let private_state = state_path(&keg);
        write_state(&private_state, &state)?;
        let removal = prepare_remove_owned_state(&keg)?;
        let marker = crate::file::read_to_string(identity_marker_path(&keg))?;
        let moved = keg.join(".brew-original");
        fs::rename(keg.join(".brew"), &moved)?;
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::write(keg.join(".brew/openssl@3.rb"), "class OpensslAT3; end")?;
        crate::file::write(identity_marker_path(&keg), marker)?;

        assert!(remove_owned_state_prepared(removal).is_err());
        assert!(private_state.is_file());
        assert!(identity_marker_path(&keg).is_file());
        Ok(())
    }

    #[test]
    fn prepared_identity_binds_resolved_plan_and_snapshot() -> Result<()> {
        let keg = prefix::cellar().join("openssl@3/1");
        let mut first = prepare(&formula(vec![]), &keg)?;
        let original = prepared_identity_sha256(&first)?;
        first.set_formula_snapshot_sha256("snapshot-a".into());
        assert_ne!(prepared_identity_sha256(&first)?, original);

        let second = prepare(
            &formula(vec![serde_json::json!({
                "type": "mkdir_p",
                "path": {"base": "prefix", "path": "generated"}
            })]),
            &keg,
        )?;
        assert_ne!(prepared_identity_sha256(&second)?, original);
        Ok(())
    }

    #[test]
    fn shared_directory_install_conflict_preserves_user_file() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = tmp.path().join("Cellar/foo/1");
        let source = keg.join(".bottle/etc/foo/conflict");
        let target = tmp.path().join("etc/foo/conflict");
        crate::file::create_dir_all(&source)?;
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&target, "user-owned")?;

        let error = install_shared_tree(
            "foo",
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("ambiguous ownership"));
        assert_eq!(crate::file::read_to_string(target)?, "user-owned");
        Ok(())
    }

    #[tokio::test]
    async fn typed_copy_does_not_overwrite_unknown_shared_file() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let source = keg.join("share/openssl@3/generated");
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(source.parent().unwrap())?;
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&source, "new")?;
        crate::file::write(&target, "user-owned")?;
        let prepared = prepare(
            &formula(vec![serde_json::json!({
                "type": "copy",
                "source": {"base": "share", "path": "openssl@3/generated"},
                "target": {"base": "pkgetc", "path": "generated"}
            })]),
            &keg,
        )?;

        assert!(execute_step(&prepared, &prepared.steps[0]).await.is_err());
        assert_eq!(crate::file::read_to_string(target)?, "user-owned");
        Ok(())
    }

    #[tokio::test]
    async fn recursive_copy_does_not_replace_unknown_shared_tree() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let source = keg.join("share/openssl@3/generated");
        let target = prefix.join("etc/openssl@3/destination");
        let existing_tree = target.join("generated");
        let sentinel = existing_tree.join("user-owned");
        crate::file::create_dir_all(&source)?;
        crate::file::create_dir_all(&existing_tree)?;
        crate::file::write(source.join("new"), "new")?;
        crate::file::write(&sentinel, "preserve")?;
        let prepared = prepare(
            &formula(vec![serde_json::json!({
                "type": "copy",
                "source": {"base": "share", "path": "openssl@3/generated"},
                "target": {"base": "pkgetc", "path": "destination"},
                "recursive": true
            })]),
            &keg,
        )?;

        assert!(execute_step(&prepared, &prepared.steps[0]).await.is_err());
        assert_eq!(crate::file::read_to_string(sentinel)?, "preserve");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn recursive_copy_preflights_full_source_before_exchange() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("Cellar/foo/1/target");
        let sentinel = target.join("sentinel");
        crate::file::create_dir_all(&source)?;
        crate::file::create_dir_all(&target)?;
        crate::file::write(&sentinel, "preserve")?;
        let socket = source.join("unsupported.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket)?;

        assert!(copy_recursive(&source, &target, true).is_err());
        assert_eq!(crate::file::read_to_string(sentinel)?, "preserve");
        Ok(())
    }

    #[test]
    fn recursive_tree_publish_is_noclobber() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let staged = tempfile::Builder::new()
            .prefix("staged-")
            .tempdir_in(tmp.path())?;
        let target = tmp.path().join("target");
        crate::file::write(staged.path().join("new"), "new")?;
        crate::file::create_dir_all(&target)?;
        crate::file::write(target.join("foreign"), "preserve")?;

        assert!(rename_noclobber(staged.path(), &target).is_err());
        assert_eq!(
            crate::file::read_to_string(target.join("foreign"))?,
            "preserve"
        );
        assert!(staged.path().join("new").is_file());
        Ok(())
    }

    #[test]
    fn recursive_owned_tree_replacement_is_atomic_exchange() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("Cellar/foo/1/target");
        crate::file::create_dir_all(&source)?;
        crate::file::create_dir_all(&target)?;
        crate::file::write(source.join("new"), "new")?;
        crate::file::write(target.join("old"), "old")?;

        let outputs = copy_recursive(&source, &target, true)?;

        assert!(outputs.contains(&target.join("new")));
        assert_eq!(crate::file::read_to_string(target.join("new"))?, "new");
        assert!(target.join("old").symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_stdout_does_not_truncate_unknown_shared_file() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&target, "user-owned")?;
        let prepared = prepare(
            &formula(vec![serde_json::json!({
                "type": "run",
                "command": {"path": "/bin/echo"},
                "args": ["new"],
                "stdout_path": {"base": "pkgetc", "path": "generated"}
            })]),
            &keg,
        )?;

        assert!(execute_step(&prepared, &prepared.steps[0]).await.is_err());
        assert_eq!(crate::file::read_to_string(target)?, "user-owned");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_write_allowlist_contains_only_explicit_shared_arguments() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let declared = prefix.join("etc/openssl@3/declared");
        let embedded = prefix.join("etc/openssl@3/embedded");
        crate::file::create_dir_all(declared.parent().unwrap())?;
        let prepared = prepare(
            &formula(vec![serde_json::json!({
                "type": "run",
                "command": {"path": "/bin/sh"},
                "args": [
                    declared,
                    format!("printf unsafe > {}", embedded.display())
                ]
            })]),
            &keg,
        )?;
        let PreparedStep::Run(run) = &prepared.steps[0] else {
            unreachable!()
        };

        assert_eq!(run_shared_write_targets(run)?, BTreeSet::from([declared]));
        Ok(())
    }

    #[test]
    fn preserves_modified_config_as_default() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = tmp.path().join("Cellar/foo/2");
        let source = keg.join(".bottle/etc/foo/config");
        let destination = tmp.path().join("etc/foo/config");
        crate::file::create_dir_all(source.parent().unwrap())?;
        crate::file::create_dir_all(destination.parent().unwrap())?;
        crate::file::write(&source, "new")?;
        crate::file::write(&destination, "user")?;
        let installed = install_shared_tree(
            "openssl@3",
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
            None,
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "user");
        let default = PathBuf::from(format!("{}.default", destination.display()));
        assert_eq!(crate::file::read_to_string(&default)?, "new");
        assert_eq!(installed.len(), 3);
        assert_eq!(
            installed.last(),
            Some(&LifecycleSharedState {
                source,
                target: default,
                kind: LifecycleSharedStateKind::File,
            })
        );
        Ok(())
    }

    #[test]
    fn upgrades_untouched_old_default() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rack = tmp.path().join("Cellar/foo");
        let old = rack.join("1/.bottle/etc/foo/config");
        let keg = rack.join("2");
        let source = keg.join(".bottle/etc/foo/config");
        let destination = tmp.path().join("etc/foo/config");
        for path in [&old, &source, &destination] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        crate::file::write(&old, "old")?;
        crate::file::write(&destination, "old")?;
        crate::file::write(&source, "new")?;
        let installed = install_shared_tree(
            "openssl@3",
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
            Some(&rack.join("1")),
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "new");
        assert_eq!(installed.len(), 3);
        assert_eq!(
            installed.last(),
            Some(&LifecycleSharedState {
                source,
                target: destination.clone(),
                kind: LifecycleSharedStateKind::File,
            })
        );
        assert!(!PathBuf::from(format!("{}.default", destination.display())).exists());
        Ok(())
    }

    #[test]
    fn ignores_non_predecessor_keg_defaults() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rack = tmp.path().join("Cellar/foo");
        let stale = rack.join("1/.bottle/etc/foo/config");
        let predecessor = rack.join("2");
        let active = predecessor.join(".bottle/etc/foo/config");
        let keg = rack.join("3");
        let source = keg.join(".bottle/etc/foo/config");
        let destination = tmp.path().join("etc/foo/config");
        for path in [&stale, &active, &source, &destination] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        crate::file::write(&stale, "user-selected")?;
        crate::file::write(&active, "active-default")?;
        crate::file::write(&destination, "user-selected")?;
        crate::file::write(&source, "new-default")?;

        let installed = install_shared_tree(
            "foo",
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
            Some(&predecessor),
        )?;

        assert_eq!(crate::file::read_to_string(&destination)?, "user-selected");
        let default = PathBuf::from(format!("{}.default", destination.display()));
        assert_eq!(crate::file::read_to_string(&default)?, "new-default");
        assert_eq!(installed.len(), 3);
        assert_eq!(
            installed.last(),
            Some(&LifecycleSharedState {
                source,
                target: default,
                kind: LifecycleSharedStateKind::File,
            })
        );
        Ok(())
    }

    #[test]
    fn old_default_comparison_stays_within_shared_root() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rack = tmp.path().join("Cellar/foo");
        let old_var = rack.join("1/.bottle/var/foo/config");
        let keg = rack.join("2");
        let source = keg.join(".bottle/etc/foo/config");
        let destination = tmp.path().join("etc/foo/config");
        for path in [&old_var, &source, &destination] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        crate::file::write(&old_var, "user")?;
        crate::file::write(&destination, "user")?;
        crate::file::write(&source, "new")?;
        let installed = install_shared_tree(
            "openssl@3",
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
            Some(&rack.join("1")),
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "user");
        assert_eq!(installed.len(), 3);
        assert_eq!(
            installed.last(),
            Some(&LifecycleSharedState {
                source,
                target: PathBuf::from(format!("{}.default", destination.display())),
                kind: LifecycleSharedStateKind::File,
            })
        );
        Ok(())
    }

    #[test]
    fn shared_state_preserves_removed_upstream_and_type_conflicts() -> Result<()> {
        for root in ["etc", "var"] {
            let tmp = tempfile::tempdir()?;
            let rack = tmp.path().join("Cellar/foo");
            let old_default = rack.join("1/.bottle").join(root).join("foo/removed");
            let keg = rack.join("2");
            let destination = tmp.path().join(root).join("foo/removed");
            crate::file::create_dir_all(old_default.parent().unwrap())?;
            crate::file::create_dir_all(destination.parent().unwrap())?;
            crate::file::write(&old_default, "old-default")?;
            crate::file::write(&destination, "user-kept")?;

            let installed = install_shared_tree(
                "foo",
                root,
                &keg.join(".bottle").join(root),
                &tmp.path().join(root),
                None,
            )?;
            assert!(installed.is_empty());
            assert_eq!(crate::file::read_to_string(&destination)?, "user-kept");

            let source = keg.join(".bottle").join(root).join("foo/conflict");
            let destination = tmp.path().join(root).join("foo/conflict");
            crate::file::create_dir_all(source.parent().unwrap())?;
            crate::file::write(&source, "new-default")?;
            crate::file::create_dir_all(&destination)?;
            install_shared_tree(
                "foo",
                root,
                &keg.join(".bottle").join(root),
                &tmp.path().join(root),
                None,
            )?;
            assert!(destination.is_dir());
            assert_eq!(
                crate::file::read_to_string(PathBuf::from(format!(
                    "{}.default",
                    destination.display()
                )))?,
                "new-default"
            );
        }
        Ok(())
    }

    #[test]
    fn preparation_rejects_unknown_fields_and_path_escape() {
        let keg = prefix::cellar().join("openssl@3/1");
        for step in [
            serde_json::json!({
                "type": "mkdir_p",
                "path": {"base": "prefix", "path": "generated"},
                "surprise": true
            }),
            serde_json::json!({
                "type": "mkdir_p",
                "path": {"base": "prefix", "path": "../../../../outside"}
            }),
            serde_json::json!({
                "type": "mkdir_p",
                "path": {"base": "unknown", "path": "outside"}
            }),
            serde_json::json!({
                "type": "mkdir_p",
                "path": {"base": "prefix", "path": "{{unknown}}"}
            }),
        ] {
            let error = prepare(&formula(vec![step]), &keg).unwrap_err();
            let error = format!("{error:?}");
            assert!(
                error.contains("unknown field")
                    || error.contains("escapes")
                    || error.contains("unsupported post-install path base")
                    || error.contains("unsupported post-install template")
            );
        }
    }

    #[test]
    fn preparation_expands_multiple_brace_groups() {
        assert_eq!(
            expand_braces("/x/{a,b}/{c,d}"),
            ["/x/a/c", "/x/a/d", "/x/b/c", "/x/b/d"]
        );
    }

    #[test]
    fn ordered_effects_describe_final_state_not_intermediate_removals() -> Result<()> {
        let root = PathBuf::from("/opt/homebrew/lib/node_modules/npm");
        let mut symlinks = vec![LifecycleSymlink {
            source: PathBuf::from("/old/source"),
            target: root.join("old-link"),
        }];
        let mut required_paths = vec![root.join("old-file")];
        let mut absent_patterns = vec![];
        let mut permissions = vec![LifecyclePermission {
            path: root.join("old-file"),
            permission: LifecyclePermissionKind::UserWrite,
            identity: LifecyclePermissionIdentity::Directory {
                device: 0,
                inode: 0,
            },
        }];

        merge_step_effects(
            &mut symlinks,
            &mut required_paths,
            &mut absent_patterns,
            &mut permissions,
            StepEffects {
                absent_patterns: vec![root.to_string_lossy().into_owned()],
                removed_paths: vec![(root.clone(), true)],
                ..Default::default()
            },
        )?;
        assert!(symlinks.is_empty());
        assert!(required_paths.is_empty());
        assert!(permissions.is_empty());
        assert_eq!(absent_patterns, [root.to_string_lossy()]);

        let recreated = root.join("bin/npm-cli.js");
        merge_step_effects(
            &mut symlinks,
            &mut required_paths,
            &mut absent_patterns,
            &mut permissions,
            StepEffects {
                required_paths: vec![recreated.clone()],
                ..Default::default()
            },
        )?;
        assert!(absent_patterns.is_empty());
        assert_eq!(required_paths, [recreated]);
        Ok(())
    }

    #[test]
    fn guards_cover_platform_if_exists_and_unless_exists() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let present = tmp.path().join("present");
        let absent = tmp.path().join("absent");
        crate::file::write(&present, "present")?;
        assert!(guards_match(&[
            PreparedGuard::Platform(true),
            PreparedGuard::IfExists(PreparedPattern {
                patterns: vec![present.to_string_lossy().into_owned()],
            }),
            PreparedGuard::UnlessExists(PreparedPattern {
                patterns: vec![absent.to_string_lossy().into_owned()],
            }),
        ])?);
        assert!(!guards_match(&[PreparedGuard::Platform(false)])?);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn recursive_remove_unlinks_dangling_symlink_without_following_it() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("outside-target");
        let dangling = tmp.path().join("dangling");
        crate::file::make_symlink(&target, &dangling)?;
        remove_prepared_node(&dangling, true, None)?;
        assert!(dangling.symlink_metadata().is_err());
        assert!(target.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn run_preflight_rejects_invalid_cwd_redirection_and_environment() {
        let keg = prefix::cellar().join("openssl@3/1");
        for step in [
            serde_json::json!({
                "type": "run",
                "command": {"base": "bin", "path": "tool"},
                "chdir": {"path": "/outside"}
            }),
            serde_json::json!({
                "type": "run",
                "command": {"base": "bin", "path": "tool"},
                "stdout_path": {"path": "/outside"}
            }),
            serde_json::json!({
                "type": "run",
                "command": {"base": "bin", "path": "tool"},
                "env": {"BAD-KEY": "value"}
            }),
        ] {
            let error = prepare(&formula(vec![step]), &keg).unwrap_err();
            assert!(format!("{error:?}").contains("no package state was changed"));
        }
    }

    #[test]
    fn run_environment_contains_only_fixed_and_typed_keys() -> Result<()> {
        let formula = formula(vec![serde_json::json!({
            "type": "run",
            "command": {"base": "bin", "path": "tool"},
            "env": {"FORMULA_KEY": "formula-value"}
        })]);
        let prepared = prepare(&formula, &prefix::cellar().join("openssl@3/1"))?;
        let PreparedStep::Run(run) = &prepared.steps[0] else {
            panic!("expected prepared run")
        };
        let env = run_environment(&prepared, run, Path::new("/private/tmp/mise-private"))?;
        assert_eq!(
            env.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "FORMULA_KEY",
                "HOME",
                "HOMEBREW_CELLAR",
                "HOMEBREW_PREFIX",
                "LANG",
                "LC_ALL",
                "PATH",
                "TMPDIR",
            ]
        );
        Ok(())
    }

    #[test]
    fn opaque_post_install_fails_closed() {
        let mut formula = formula(vec![]);
        formula.post_install_defined = true;
        let error = prepare(&formula, &prefix::cellar().join("openssl@3/1")).unwrap_err();
        assert!(error.to_string().contains("opaque Ruby post_install"));
    }

    #[test]
    fn prune_removes_only_unchanged_lifecycle_symlinks() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = crate::file::desymlink_path(tmp.path());
        let source = root.join("source");
        let replacement = root.join("replacement");
        let target = root.join("target");
        crate::file::write(&source, "source")?;
        crate::file::write(&replacement, "replacement")?;
        crate::file::make_symlink(&source, &target)?;
        let state = LifecycleState {
            complete: true,
            phase: LifecyclePhase::Complete,
            install_identity: None,
            shared_state: vec![],
            symlinks: vec![LifecycleSymlink {
                source: source.clone(),
                target: target.clone(),
            }],
            required_paths: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        };
        remove_lifecycle_symlinks(&state)?;
        assert!(!target.exists());

        crate::file::make_symlink(&replacement, &target)?;
        remove_lifecycle_symlinks(&state)?;
        assert_eq!(fs::read_link(target)?, replacement);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn prepared_lifecycle_symlink_removal_rejects_foreign_swap() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = crate::file::desymlink_path(tmp.path());
        let source = root.join("source");
        let target = root.join("target");
        crate::file::write(&source, "source")?;
        crate::file::make_symlink(&source, &target)?;
        let state = LifecycleState {
            complete: true,
            phase: LifecyclePhase::Complete,
            install_identity: None,
            shared_state: vec![],
            symlinks: vec![LifecycleSymlink {
                source,
                target: target.clone(),
            }],
            required_paths: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        };
        let prepared = prepare_lifecycle_symlink_removals(&state)?;
        crate::file::remove_file(&target)?;
        crate::file::write(&target, "foreign")?;

        assert!(remove_prepared_lifecycle_symlinks(&prepared).is_err());
        assert_eq!(crate::file::read_to_string(target)?, "foreign");
        Ok(())
    }
}
