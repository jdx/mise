//! Persistent formula state and typed post-install operations.

use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::fs::{self, File};
#[cfg(unix)]
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use eyre::{WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(unix)]
use sha2::{Digest, Sha256};

use super::api::Formula;
use super::prefix;
use crate::cmd::CmdLineRunner;
use crate::result::Result;
use crate::sandbox::SandboxConfig;

const MAX_FAILURE_LOG_BYTES: usize = 32 * 1024;
// Audited Homebrew ca-certificates 2026-08-13 recipe/helper. Any recipe or
// helper update must be re-audited and these pins updated; drift fails closed.
#[cfg(target_os = "macos")]
const AUDITED_CA_CERTIFICATES_FORMULA_SHA256: &str =
    "69dcb65421d8cae528a62542ea4870af0b1be5f90d98389a6e3cb5278d4d8af3";
#[cfg(target_os = "macos")]
const AUDITED_CA_CERTIFICATES_HELPER_SHA256: &str =
    "0d382c7231f5f0378947729c5815f7fb8f6c12c16396172513109b13bd59b900";

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
    pub(super) fn bind_bottle_formula_snapshot_sha256(&mut self, sha256: String) -> Result<()> {
        self.formula_snapshot_sha256 = Some(sha256);
        #[cfg(target_os = "macos")]
        if self
            .steps
            .iter()
            .any(|step| matches!(step, PreparedStep::AuditedCaCertificates(_)))
            && self.formula_snapshot_sha256.as_deref()
                != Some(AUDITED_CA_CERTIFICATES_FORMULA_SHA256)
        {
            bail!(
                "brew:ca-certificates bottle formula snapshot is not the audited macOS lifecycle recipe; no package state was changed"
            )
        }
        Ok(())
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
    #[cfg(not(target_os = "macos"))]
    Run(PreparedRun),
    #[cfg(target_os = "macos")]
    AuditedCaCertificates(PreparedRun),
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
    run_files: Vec<LifecycleRunFile>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LifecycleRunFile {
    path: PathBuf,
    sha256: String,
    #[serde(default)]
    metadata_sha256: Option<String>,
    device: u64,
    inode: u64,
    mode: u32,
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
    if cwd
        .as_ref()
        .is_some_and(|path| !path_is_within_keg(keg, path))
    {
        bail!(
            "brew:{} post-install step {index} working directory must remain inside its keg; no package state was changed",
            formula.name
        )
    }
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
    if stdin_path
        .as_ref()
        .is_some_and(|path| !path_is_within_keg(keg, path))
    {
        bail!(
            "brew:{} post-install step {index} stdin must remain inside its keg; no package state was changed",
            formula.name
        )
    }
    let stdout_path = raw
        .stdout_path
        .as_ref()
        .map(|path| resolve_write_path(formula, keg, path))
        .transpose()?;
    let identity = crate::hash::hash_to_str(&(prefix::prefix(), &formula.name, keg));
    let run = PreparedRun {
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
    };
    #[cfg(target_os = "macos")]
    {
        let audited_ca_shape = formula.name == "ca-certificates"
            && run.executable == keg.join("libexec/post-install")
            && run.args
                == [
                    keg.join("share/ca-certificates/cacert.pem")
                        .to_string_lossy()
                        .into_owned(),
                    prefix::prefix()
                        .join("etc/ca-certificates/cert.pem")
                        .to_string_lossy()
                        .into_owned(),
                ]
            && run.cwd.is_none()
            && run.env.is_empty()
            && run.stdin_path.is_none()
            && run.stdout_path.is_none()
            && run.guards.is_empty();
        if audited_ca_shape {
            return Ok(PreparedStep::AuditedCaCertificates(run));
        }
        bail!(
            "brew:{} post-install step {index} requires generic macOS process containment that is unavailable; no package state was changed",
            formula.name
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        if !run
            .guards
            .iter()
            .any(|guard| matches!(guard, PreparedGuard::Platform(false)))
        {
            crate::sandbox::ensure_strict_formula_execution_available(&format!(
                "brew:{} post-install step {index}",
                formula.name
            ))?;
        }
        Ok(PreparedStep::Run(run))
    }
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
    for file in &state.run_files {
        match lifecycle_run_file_matches(file) {
            Ok(true) => {}
            Ok(false) => {
                reinstall.insert(format!(
                    "post-install regular-file identity or contents changed: {}",
                    file.path.display()
                ));
            }
            Err(error) => {
                reinstall.insert(format!(
                    "post-install regular-file output is unreadable at {}: {error}",
                    file.path.display()
                ));
            }
        }
    }
    for required in &state.required_paths {
        if state
            .shared_state
            .iter()
            .any(|mapping| mapping.target == *required)
            || state.run_files.iter().any(|file| file.path == *required)
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

fn lifecycle_run_file_matches(expected: &LifecycleRunFile) -> Result<bool> {
    #[cfg(unix)]
    {
        let Some(parent_path) = expected.path.parent() else {
            return Ok(false);
        };
        let Some(name) = expected.path.file_name() else {
            return Ok(false);
        };
        let parent = BoundRunSharedParent::open_existing(parent_path)?;
        let Some(current) = open_bound_run_file(&parent, name)? else {
            return Ok(false);
        };
        Ok(run_file_device(current.device)? == expected.device
            && current.inode == expected.inode
            && run_file_mode(current.mode)? == expected.mode
            && current.sha256 == expected.sha256
            && expected
                .metadata_sha256
                .as_ref()
                .is_none_or(|digest| current.metadata_sha256 == *digest))
    }
    #[cfg(not(unix))]
    {
        let Some(metadata) = symlink_metadata_if_exists(&expected.path)? else {
            return Ok(false);
        };
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Ok(false);
        }
        let (device, inode) = permission_device_inode(&metadata)?;
        Ok(device == expected.device
            && inode == expected.inode
            && regular_file_mode(&metadata) == expected.mode
            && crate::hash::file_hash_sha256(&expected.path, None)? == expected.sha256)
    }
}

#[cfg(not(unix))]
fn regular_file_mode(_metadata: &fs::Metadata) -> u32 {
    0
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
    let mut run_paths = BTreeSet::new();
    for file in &state.run_files {
        ensure_runtime_write_path(&file.path, true)?;
        if !state.required_paths.contains(&file.path)
            || state
                .shared_state
                .iter()
                .any(|mapping| mapping.target == file.path)
            || state.symlinks.iter().any(|link| link.target == file.path)
            || !run_paths.insert(file.path.clone())
        {
            bail!(
                "invalid or duplicate typed post-install regular-file effect: {}",
                file.path.display()
            )
        }
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
            run_files: vec![],
            absent_patterns: vec![],
            permissions: vec![],
            repair: None,
        },
    )?;
    let mut shared_state = vec![];
    let mut symlinks = vec![];
    let mut required_paths = vec![];
    let mut run_files = vec![];
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
                run_files: vec![],
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
                &mut run_files,
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
                run_files,
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
    run_files: Vec<LifecycleRunFile>,
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
    run_files: &mut Vec<LifecycleRunFile>,
    absent_patterns: &mut Vec<String>,
    permissions: &mut Vec<LifecyclePermission>,
    effects: StepEffects,
) -> Result<()> {
    for (removed, recursive) in &effects.removed_paths {
        let removes = |path: &Path| path == removed || (*recursive && path.starts_with(removed));
        symlinks.retain(|link| !removes(&link.target));
        required_paths.retain(|path| !removes(path));
        run_files.retain(|file| !removes(&file.path));
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
    for file in effects.run_files {
        if let Some(existing) = run_files
            .iter_mut()
            .find(|existing| existing.path == file.path)
        {
            *existing = file;
        } else {
            run_files.push(file);
        }
    }
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
        #[cfg(not(target_os = "macos"))]
        PreparedStep::Run(run) => &run.guards,
        #[cfg(target_os = "macos")]
        PreparedStep::AuditedCaCertificates(run) => &run.guards,
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
        #[cfg(not(target_os = "macos"))]
        PreparedStep::Run(run) => execute_run(prepared, run, None).await,
        #[cfg(target_os = "macos")]
        PreparedStep::AuditedCaCertificates(run) => {
            if prepared.formula_snapshot_sha256.as_deref()
                != Some(AUDITED_CA_CERTIFICATES_FORMULA_SHA256)
            {
                bail!(
                    "brew:ca-certificates formula snapshot is not the audited macOS lifecycle recipe"
                )
            }
            execute_run(prepared, run, Some(AUDITED_CA_CERTIFICATES_HELPER_SHA256)).await
        }
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
    audited_executable_sha256: Option<&str>,
) -> Result<StepEffects> {
    #[cfg(unix)]
    let audited_executable = if let Some(expected_sha256) = audited_executable_sha256 {
        let parent_path = run
            .executable
            .parent()
            .ok_or_else(|| eyre!("audited post-install helper has no parent"))?;
        let parent = BoundRunSharedParent::open_existing(parent_path)?;
        let name = run
            .executable
            .file_name()
            .ok_or_else(|| eyre!("audited post-install helper has no filename"))?
            .to_os_string();
        let identity = open_bound_run_file(&parent, &name)?
            .ok_or_else(|| eyre!("audited post-install helper is missing"))?;
        if identity.sha256 != expected_sha256 {
            bail!("audited post-install helper contents changed; no package state was changed")
        }
        Some((parent, name, identity))
    } else {
        None
    };
    #[cfg(not(unix))]
    let _ = audited_executable_sha256;
    let temp_base = crate::dirs::CACHE
        .join("system-brew")
        .join("post-install")
        .join(crate::hash::hash_to_str(&(
            &prepared.formula,
            &prepared.keg,
            run.step_index,
        )));
    let temp_guard = BoundRunPrivateTree::create(&temp_base, "run-")?;
    let temp = temp_guard.path().to_path_buf();
    #[cfg(unix)]
    let (execution_executable, audited_copy) = if let Some((_, _, identity)) = &audited_executable {
        let name = std::ffi::OsString::from("audited-post-install");
        let mut copy = create_bound_run_file(&temp_guard.parent, &name)?;
        copy_open_file_to(&identity.file, &mut copy)?;
        copy_run_file_metadata(&identity.file, &copy)?;
        copy.sync_all()?;
        temp_guard.parent.sync()?;
        let copy = open_bound_run_file(&temp_guard.parent, &name)?
            .ok_or_else(|| eyre!("private audited post-install helper copy is missing"))?;
        if copy.sha256 != identity.sha256 {
            bail!("private audited post-install helper copy changed")
        }
        (temp.join(&name), Some((name, copy)))
    } else {
        (run.executable.clone(), None)
    };
    #[cfg(not(unix))]
    let execution_executable = run.executable.clone();
    #[cfg(unix)]
    let audited_input = if audited_executable.is_some() {
        let path = run
            .args
            .first()
            .ok_or_else(|| eyre!("audited post-install helper has no source input"))?;
        Some(open_run_stdin(&prepared.keg, Path::new(path))?)
    } else {
        None
    };
    #[cfg(unix)]
    let audited_input_copy = if let Some((_, _, identity)) = &audited_input {
        let name = std::ffi::OsString::from("audited-source-input");
        let mut copy = create_bound_run_file(&temp_guard.parent, &name)?;
        copy_open_file_to(&identity.file, &mut copy)?;
        copy_run_file_metadata(&identity.file, &copy)?;
        copy.sync_all()?;
        temp_guard.parent.sync()?;
        let copy = open_bound_run_file(&temp_guard.parent, &name)?
            .ok_or_else(|| eyre!("private audited post-install source copy is missing"))?;
        if copy.sha256 != identity.sha256 {
            bail!("private audited post-install source copy changed")
        }
        Some((name, copy))
    } else {
        None
    };
    #[cfg(unix)]
    let log_parent = BoundRunSharedParent::open_private_beneath(Path::new("/"), &run.log_dir)?;
    #[cfg(unix)]
    let (stdout_log_writer, _stdout_log_path) =
        create_unique_run_log(&log_parent, &format!("{}-stdout-", run.step_index))?;
    #[cfg(unix)]
    let (stderr_log_writer, _stderr_log_path) =
        create_unique_run_log(&log_parent, &format!("{}-stderr-", run.step_index))?;
    #[cfg(not(unix))]
    let (stdout_log_writer, _stdout_log_path) = {
        crate::file::create_dir_all(&run.log_dir)?;
        let path = run.log_dir.join(format!("{}.stdout.log", run.step_index));
        (open_truncated(&path)?, path)
    };
    #[cfg(not(unix))]
    let (stderr_log_writer, _stderr_log_path) = {
        let path = run.log_dir.join(format!("{}.stderr.log", run.step_index));
        (open_truncated(&path)?, path)
    };
    let mut stdout_log_reader = stdout_log_writer.try_clone()?;
    let mut stderr_log_reader = stderr_log_writer.try_clone()?;

    let shared_write_targets = run_shared_write_targets(run)?;
    let mut shared_writes = prepare_run_shared_writes(
        &prepared.keg,
        &shared_write_targets,
        run.stdout_path.as_deref(),
        &temp,
    )?;
    let mut rewritten_args = shared_writes.rewrite_args(&run.args);
    #[cfg(unix)]
    if audited_input_copy.is_some() {
        rewritten_args[0] = temp.join("audited-source-input").display().to_string();
    }
    let stdout = match &run.stdout_path {
        Some(path) => shared_writes.open_stdout(path)?,
        None => stdout_log_writer,
    };
    let stderr = stderr_log_writer;
    #[cfg(unix)]
    let bound_stdin = run
        .stdin_path
        .as_ref()
        .map(|path| open_run_stdin(&prepared.keg, path))
        .transpose()?;
    #[cfg(unix)]
    let stdin = match &bound_stdin {
        Some((parent, name, identity)) => {
            validate_bound_run_file_identity(parent, name, identity)?;
            let mut child = open_bound_run_file(parent, name)?
                .ok_or_else(|| eyre!("post-install stdin disappeared before execution"))?;
            if (child.device, child.inode) != (identity.device, identity.inode)
                || child.sha256 != identity.sha256
                || child.metadata_sha256 != identity.metadata_sha256
            {
                bail!("post-install stdin changed before execution")
            }
            child.file.seek(std::io::SeekFrom::Start(0))?;
            Stdio::from(child.file)
        }
        None => Stdio::null(),
    };
    #[cfg(not(unix))]
    let stdin = match &run.stdin_path {
        Some(path) => Stdio::from(File::open(path)?),
        None => Stdio::null(),
    };

    let env = run_environment(prepared, run, &temp, audited_executable_sha256.is_some())?;
    let read_paths = vec![run.executable.clone()];
    let mut sandbox = lifecycle_run_sandbox(
        &prepared.keg,
        &read_paths,
        &_stdout_log_path,
        &_stderr_log_path,
        &temp,
        None,
        audited_executable_sha256.is_none(),
    );
    sandbox.resolve_paths();
    #[cfg(target_os = "linux")]
    sandbox.bind_formula_execution_paths()?;
    let cwd = run.cwd.as_deref().unwrap_or(&prepared.keg);
    #[cfg(unix)]
    let bound_cwd = BoundRunSharedParent::open_existing(cwd)?;
    #[cfg(unix)]
    let command =
        CmdLineRunner::new(&execution_executable).current_dir_fd(nix::unistd::dup(bound_cwd.fd())?);
    #[cfg(not(unix))]
    let command = CmdLineRunner::new(&execution_executable).current_dir(cwd);
    let mut command = command
        .args(&rewritten_args)
        .with_process_group_cleanup()
        .with_sandbox(sandbox);
    shared_writes.validate()?;
    #[cfg(unix)]
    bound_cwd.validate()?;
    #[cfg(unix)]
    if let Some((parent, name, identity)) = &bound_stdin {
        validate_bound_run_file_identity(parent, name, identity)?;
    }
    command.apply_sandbox().await?;
    command = command
        .env_clear()
        .envs(&env)
        .stdin(stdin)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if let Err(error) = command.execute_async().await {
        let stdout = if let Some(path) = &run.stdout_path {
            shared_writes.stdout_tail(path)?
        } else {
            log_tail_file(&mut stdout_log_reader)?
        };
        let stderr = log_tail_file(&mut stderr_log_reader)?;
        return Err(error).wrap_err_with(|| {
            format!(
                "brew:{} post-install run step {} failed\nstdout tail:\n{}\nstderr tail:\n{}",
                prepared.formula, run.step_index, stdout, stderr
            )
        });
    }
    shared_writes.validate()?;
    #[cfg(unix)]
    bound_cwd
        .validate()
        .wrap_err("post-install working directory changed during execution")?;
    #[cfg(unix)]
    if let Some((parent, name, identity)) = &bound_stdin {
        validate_bound_run_file_identity(parent, name, identity)
            .wrap_err("post-install stdin changed during execution")?;
    }
    #[cfg(unix)]
    if let Some((parent, name, identity)) = &audited_executable {
        validate_bound_run_file_identity(parent, name, identity)
            .wrap_err("audited post-install helper changed during execution")?;
    }
    #[cfg(unix)]
    if let Some((name, identity)) = &audited_copy {
        validate_bound_run_file_identity(&temp_guard.parent, name, identity)
            .wrap_err("private audited post-install helper changed during execution")?;
    }
    #[cfg(unix)]
    if let Some((parent, name, identity)) = &audited_input {
        validate_bound_run_file_identity(parent, name, identity)
            .wrap_err("audited post-install source input changed during execution")?;
    }
    #[cfg(unix)]
    if let Some((name, identity)) = &audited_input_copy {
        validate_bound_run_file_identity(&temp_guard.parent, name, identity)
            .wrap_err("private audited post-install source copy changed during execution")?;
    }

    let published = shared_writes.publish()?;
    let mut required_paths = published
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    for path in shared_write_targets {
        if path_is_within_keg(&prepared.keg, &path) && symlink_metadata_if_exists(&path)?.is_some()
        {
            required_paths.push(path);
        }
    }
    Ok(StepEffects {
        required_paths,
        run_files: published.files,
        absent_patterns: published
            .deleted
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        ..Default::default()
    })
}

#[cfg(unix)]
fn open_run_stdin(
    keg: &Path,
    path: &Path,
) -> Result<(
    BoundRunSharedParent,
    std::ffi::OsString,
    BoundRunFileIdentity,
)> {
    if !path_is_within_keg(keg, path) {
        bail!("post-install stdin escapes its keg: {}", path.display())
    }
    let parent_path = path
        .parent()
        .ok_or_else(|| eyre!("post-install stdin has no parent"))?;
    let parent = BoundRunSharedParent::open_existing(parent_path)?;
    let name = path
        .file_name()
        .ok_or_else(|| eyre!("post-install stdin has no filename"))?;
    let identity = open_bound_run_file(&parent, name)?
        .ok_or_else(|| eyre!("post-install stdin is missing: {}", path.display()))?;
    Ok((parent, name.to_os_string(), identity))
}

fn lifecycle_run_sandbox(
    keg: &Path,
    read_paths: &[PathBuf],
    stdout_log: &Path,
    stderr_log: &Path,
    temp: &Path,
    stdout_temporary: Option<&Path>,
    writable_keg: bool,
) -> SandboxConfig {
    let mut allow_read = vec![keg.to_path_buf()];
    allow_read.extend(read_paths.iter().cloned());
    let mut allow_write = vec![
        stdout_log.to_path_buf(),
        stderr_log.to_path_buf(),
        temp.to_path_buf(),
    ];
    if writable_keg {
        allow_write.push(keg.to_path_buf());
    }
    if let Some(stdout_temporary) = stdout_temporary {
        allow_write.push(stdout_temporary.to_path_buf());
    }
    SandboxConfig {
        deny_read: cfg!(target_os = "linux"),
        deny_write: true,
        deny_net: true,
        deny_local_sockets: true,
        deny_env: true,
        allow_read,
        allow_write,
        deny_system_temp_write: true,
        deny_mise_data_read: cfg!(target_os = "linux"),
        require_full_filesystem_confinement: cfg!(target_os = "linux"),
        system_access_profile: if cfg!(target_os = "linux") {
            crate::sandbox::SystemAccessProfile::FormulaExecution
        } else {
            crate::sandbox::SystemAccessProfile::Compatibility
        },
        ..Default::default()
    }
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

#[cfg(unix)]
#[derive(Debug)]
struct BoundRunDirectory {
    path: PathBuf,
    fd: std::os::fd::OwnedFd,
    device: nix::libc::dev_t,
    inode: nix::libc::ino_t,
    created: bool,
    name: Option<std::ffi::OsString>,
}

#[cfg(unix)]
#[derive(Debug)]
struct BoundRunSharedParent {
    directories: Vec<BoundRunDirectory>,
}

#[cfg(unix)]
impl BoundRunSharedParent {
    fn open(parent: &Path) -> Result<Self> {
        Self::open_beneath(&prefix::prefix(), parent)
    }

    fn open_existing(parent: &Path) -> Result<Self> {
        Self::open_beneath_mode_inner(Path::new("/"), parent, nix::sys::stat::Mode::empty(), false)
    }

    fn open_beneath(root: &Path, parent: &Path) -> Result<Self> {
        Self::open_beneath_mode(
            root,
            parent,
            nix::sys::stat::Mode::S_IRWXU
                | nix::sys::stat::Mode::S_IRGRP
                | nix::sys::stat::Mode::S_IXGRP
                | nix::sys::stat::Mode::S_IROTH
                | nix::sys::stat::Mode::S_IXOTH,
        )
    }

    fn open_private_beneath(root: &Path, parent: &Path) -> Result<Self> {
        Self::open_beneath_mode(root, parent, nix::sys::stat::Mode::S_IRWXU)
    }

    fn open_beneath_mode(
        root: &Path,
        parent: &Path,
        create_mode: nix::sys::stat::Mode,
    ) -> Result<Self> {
        Self::open_beneath_mode_inner(root, parent, create_mode, true)
    }

    fn open_beneath_mode_inner(
        root: &Path,
        parent: &Path,
        create_mode: nix::sys::stat::Mode,
        allow_create: bool,
    ) -> Result<Self> {
        use nix::fcntl::{OFlag, open, openat};
        use nix::sys::stat::{Mode, SFlag, fstat};

        let shared = super::pour::lexical_normalize(root);
        let parent = super::pour::lexical_normalize(parent);
        parent.strip_prefix(&shared).wrap_err_with(|| {
            format!(
                "post-install shared output parent escapes {}: {}",
                shared.display(),
                parent.display()
            )
        })?;
        let shared_components = shared
            .strip_prefix(Path::new("/"))
            .wrap_err("post-install shared prefix is not absolute")?
            .components()
            .count();
        let components = parent
            .strip_prefix(Path::new("/"))
            .wrap_err("post-install shared output parent is not absolute")?
            .components()
            .map(|component| match component {
                std::path::Component::Normal(name) => Ok(name.to_os_string()),
                _ => bail!("post-install shared output parent has an invalid component"),
            })
            .collect::<Result<Vec<_>>>()?;
        let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let root_fd = open(Path::new("/"), flags, Mode::empty())?;
        let root_stat = fstat(&root_fd)?;
        if !SFlag::from_bits_truncate(root_stat.st_mode).contains(SFlag::S_IFDIR) {
            bail!("filesystem root is not a real directory")
        }
        let mut bound = Self {
            directories: vec![BoundRunDirectory {
                path: PathBuf::from("/"),
                fd: root_fd,
                device: root_stat.st_dev,
                inode: root_stat.st_ino,
                created: false,
                name: None,
            }],
        };
        let mut current = PathBuf::from("/");
        for (index, name) in components.iter().enumerate() {
            current.push(name);
            let parent_fd = &bound.directories.last().unwrap().fd;
            let create_missing = allow_create && index >= shared_components;
            let (fd, created) = match openat(parent_fd, name.as_os_str(), flags, Mode::empty()) {
                Ok(fd) => (fd, false),
                Err(nix::errno::Errno::ENOENT) if create_missing => {
                    let created =
                        match nix::sys::stat::mkdirat(parent_fd, name.as_os_str(), create_mode) {
                            Ok(()) => true,
                            Err(nix::errno::Errno::EEXIST) => false,
                            Err(error) => {
                                return Err(error).wrap_err_with(|| {
                                    format!(
                                        "could not create post-install shared output parent: {}",
                                        current.display()
                                    )
                                });
                            }
                        };
                    let fd = openat(parent_fd, name.as_os_str(), flags, Mode::empty())
                        .wrap_err_with(|| {
                            format!(
                                "could not bind post-install shared output parent: {}",
                                current.display()
                            )
                        })?;
                    (fd, created)
                }
                Err(error) => {
                    return Err(error).wrap_err_with(|| {
                        format!(
                            "post-install shared output parent is not a real directory: {}",
                            current.display()
                        )
                    });
                }
            };
            let stat = fstat(&fd)?;
            if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFDIR) {
                bail!(
                    "post-install shared output parent is not a real directory: {}",
                    current.display()
                )
            }
            bound.directories.push(BoundRunDirectory {
                path: current.clone(),
                fd,
                device: stat.st_dev,
                inode: stat.st_ino,
                created,
                name: Some(name.clone()),
            });
        }
        bound.validate()?;
        Ok(bound)
    }

    fn fd(&self) -> &std::os::fd::OwnedFd {
        &self.directories.last().unwrap().fd
    }

    fn final_was_created(&self) -> bool {
        self.directories
            .last()
            .is_some_and(|directory| directory.created)
    }

    fn path(&self) -> &Path {
        &self.directories.last().unwrap().path
    }

    fn cleanup_created_tree(&mut self) -> Result<()> {
        let index = self.directories.len() - 1;
        if !self.directories[index].created {
            bail!("post-install private tree was not created by this transaction")
        }
        self.validate()?;
        remove_run_tree_contents(&self.directories[index].fd)?;
        self.sync()?;
        let parent = &self.directories[index - 1];
        let name = self.directories[index]
            .name
            .as_deref()
            .ok_or_else(|| eyre!("post-install private tree has no bound name"))?;
        nix::unistd::unlinkat(&parent.fd, name, nix::unistd::UnlinkatFlags::RemoveDir)?;
        nix::unistd::fsync(&parent.fd)?;
        self.directories[index].created = false;
        Ok(())
    }

    fn sync(&self) -> Result<()> {
        nix::unistd::fsync(self.fd())?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        use nix::sys::stat::{SFlag, fstat};

        for directory in &self.directories {
            let stat = fstat(&directory.fd)?;
            if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFDIR)
                || (stat.st_dev, stat.st_ino) != (directory.device, directory.inode)
            {
                bail!(
                    "post-install shared output parent descriptor changed: {}",
                    directory.path.display()
                )
            }
            let metadata = directory.path.symlink_metadata().wrap_err_with(|| {
                format!(
                    "post-install shared output parent changed after preflight: {}",
                    directory.path.display()
                )
            })?;
            let (device, inode) = permission_device_inode(&metadata)?;
            if !metadata.file_type().is_dir()
                || metadata.file_type().is_symlink()
                || run_file_device(directory.device)? != device
                || directory.inode != inode
            {
                bail!(
                    "post-install shared output parent changed after preflight: {}",
                    directory.path.display()
                )
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(super) fn remove_run_tree_contents<Fd: std::os::fd::AsFd>(directory: Fd) -> Result<()> {
    let mount = run_directory_mount_identity(&directory)?;
    remove_run_tree_contents_on_mount(directory, mount)
}

#[cfg(target_os = "linux")]
fn run_directory_mount_identity<Fd: std::os::fd::AsFd>(directory: &Fd) -> Result<u64> {
    use std::os::fd::AsRawFd;

    let mut statx = std::mem::MaybeUninit::<nix::libc::statx>::zeroed();
    let result = unsafe {
        nix::libc::statx(
            directory.as_fd().as_raw_fd(),
            c"".as_ptr(),
            nix::libc::AT_EMPTY_PATH | nix::libc::AT_SYMLINK_NOFOLLOW,
            nix::libc::STATX_MNT_ID,
            statx.as_mut_ptr(),
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    let statx = unsafe { statx.assume_init() };
    if statx.stx_mask & nix::libc::STATX_MNT_ID == 0 {
        bail!("post-install private tree mount identity is unavailable")
    }
    Ok(statx.stx_mnt_id)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn run_directory_mount_identity<Fd: std::os::fd::AsFd>(directory: &Fd) -> Result<u64> {
    Ok(u64::try_from(nix::sys::stat::fstat(directory)?.st_dev)?)
}

#[cfg(unix)]
fn remove_run_tree_contents_on_mount<Fd: std::os::fd::AsFd>(
    directory: Fd,
    expected_mount: u64,
) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;

    let fd = nix::fcntl::openat(
        &directory,
        ".",
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    let mut entries = nix::dir::Dir::from_fd(fd)?;
    if run_directory_mount_identity(&entries)? != expected_mount {
        bail!("post-install private tree crossed an unexpected mount boundary")
    }
    let names = entries
        .iter()
        .map(|entry| entry.map(|entry| entry.file_name().to_owned()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for name in names {
        if name.as_bytes() == b"." || name.as_bytes() == b".." {
            continue;
        }
        let stat = nix::sys::stat::fstatat(
            &entries,
            std::ffi::OsStr::from_bytes(name.to_bytes()),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )?;
        let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
        if kind.contains(nix::sys::stat::SFlag::S_IFDIR) {
            let child = nix::fcntl::openat(
                &entries,
                std::ffi::OsStr::from_bytes(name.to_bytes()),
                nix::fcntl::OFlag::O_RDONLY
                    | nix::fcntl::OFlag::O_DIRECTORY
                    | nix::fcntl::OFlag::O_NOFOLLOW
                    | nix::fcntl::OFlag::O_CLOEXEC,
                nix::sys::stat::Mode::empty(),
            )?;
            if run_directory_mount_identity(&child)? != expected_mount {
                bail!(
                    "post-install private tree contains an unexpected mounted directory: {}",
                    Path::new(std::ffi::OsStr::from_bytes(name.to_bytes())).display()
                )
            }
            remove_run_tree_contents_on_mount(&child, expected_mount)?;
            nix::unistd::fsync(&child)?;
            nix::unistd::unlinkat(
                &entries,
                std::ffi::OsStr::from_bytes(name.to_bytes()),
                nix::unistd::UnlinkatFlags::RemoveDir,
            )?;
        } else {
            nix::unistd::unlinkat(
                &entries,
                std::ffi::OsStr::from_bytes(name.to_bytes()),
                nix::unistd::UnlinkatFlags::NoRemoveDir,
            )?;
        }
    }
    nix::unistd::fsync(&entries)?;
    Ok(())
}

#[cfg(unix)]
#[derive(Debug)]
struct BoundRunPrivateTree {
    parent: BoundRunSharedParent,
}

#[cfg(unix)]
impl BoundRunPrivateTree {
    fn create(base: &Path, prefix: &str) -> Result<Self> {
        let base = BoundRunSharedParent::open_private_beneath(Path::new("/"), base)?;
        loop {
            let path = base
                .path()
                .join(format!("{prefix}{}", crate::rand::random_string(16)));
            let parent = BoundRunSharedParent::open_private_beneath(base.path(), &path)?;
            if parent.final_was_created() {
                return Ok(Self { parent });
            }
        }
    }

    fn path(&self) -> &Path {
        self.parent.path()
    }
}

#[cfg(unix)]
impl Drop for BoundRunPrivateTree {
    fn drop(&mut self) {
        let _ = self.parent.cleanup_created_tree();
    }
}

#[cfg(not(unix))]
#[derive(Debug)]
struct BoundRunPrivateTree(tempfile::TempDir);

#[cfg(not(unix))]
impl BoundRunPrivateTree {
    fn create(base: &Path, prefix: &str) -> Result<Self> {
        crate::file::create_dir_all(base)?;
        Ok(Self(
            tempfile::Builder::new().prefix(prefix).tempdir_in(base)?,
        ))
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

#[cfg(unix)]
impl Drop for BoundRunSharedParent {
    fn drop(&mut self) {
        use nix::sys::stat::{SFlag, fstat, fstatat};
        use nix::unistd::{UnlinkatFlags, unlinkat};

        for index in (1..self.directories.len()).rev() {
            let child = &self.directories[index];
            if !child.created {
                continue;
            }
            let parent = &self.directories[index - 1];
            let Some(name) = child.name.as_deref() else {
                continue;
            };
            let Ok(fd_stat) = fstat(&child.fd) else {
                continue;
            };
            let Ok(path_stat) = fstatat(&parent.fd, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
            else {
                continue;
            };
            if SFlag::from_bits_truncate(fd_stat.st_mode).contains(SFlag::S_IFDIR)
                && SFlag::from_bits_truncate(path_stat.st_mode).contains(SFlag::S_IFDIR)
                && (fd_stat.st_dev, fd_stat.st_ino) == (child.device, child.inode)
                && (path_stat.st_dev, path_stat.st_ino) == (child.device, child.inode)
            {
                // Never recurse. A published output or concurrent foreign node
                // keeps the directory non-empty and therefore preserves it.
                let _ = unlinkat(&parent.fd, name, UnlinkatFlags::RemoveDir);
            }
        }
    }
}

#[cfg(unix)]
#[derive(Debug)]
struct BoundRunFileIdentity {
    file: File,
    sha256: String,
    metadata_sha256: String,
    device: nix::libc::dev_t,
    inode: nix::libc::ino_t,
    mode: nix::libc::mode_t,
    uid: nix::libc::uid_t,
    gid: nix::libc::gid_t,
}

#[cfg(unix)]
#[derive(Debug)]
enum BoundRunLeafAuthority {
    Missing,
    Existing(BoundRunFileIdentity),
}

#[cfg(unix)]
#[derive(Debug)]
struct RunSharedOutput {
    destination: PathBuf,
    staging: PathBuf,
    parent: BoundRunSharedParent,
    name: std::ffi::OsString,
    staging_parent: BoundRunSharedParent,
    staging_name: std::ffi::OsString,
    rollback_parent: BoundRunSharedParent,
    authority: BoundRunLeafAuthority,
    stdout: bool,
}

#[cfg(unix)]
impl RunSharedOutput {
    fn prepare(
        destination: &Path,
        staging: PathBuf,
        staging_root: &Path,
        stdout: bool,
        allow_existing_stdout: bool,
    ) -> Result<Self> {
        use nix::fcntl::{OFlag, openat};
        use nix::sys::stat::{Mode, SFlag, fstat, fstatat};

        let parent_path = destination.parent().ok_or_else(|| {
            eyre!(
                "post-install shared output has no parent: {}",
                destination.display()
            )
        })?;
        let parent = BoundRunSharedParent::open(parent_path)?;
        let name = destination
            .file_name()
            .ok_or_else(|| eyre!("post-install shared output has no filename"))?
            .to_os_string();
        let staging_parent_path = staging
            .parent()
            .ok_or_else(|| eyre!("post-install shared staging output has no parent"))?;
        let staging_parent = BoundRunSharedParent::open_beneath(staging_root, staging_parent_path)?;
        let staging_name = staging
            .file_name()
            .ok_or_else(|| eyre!("post-install shared staging output has no filename"))?
            .to_os_string();
        let rollback_parent = loop {
            let path = parent_path.join(format!(
                ".mise-lifecycle-rollback-{}",
                crate::rand::random_string(16)
            ));
            let candidate = BoundRunSharedParent::open_private_beneath(parent_path, &path)?;
            if candidate.final_was_created() {
                break candidate;
            }
        };
        let authority = match fstatat(
            parent.fd(),
            name.as_os_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
            Err(nix::errno::Errno::ENOENT) => BoundRunLeafAuthority::Missing,
            Err(error) => return Err(error.into()),
            Ok(stat) => {
                if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFREG) {
                    bail!(
                        "post-install shared output is not an exact regular file: {}",
                        destination.display()
                    )
                }
                if stdout && !allow_existing_stdout {
                    bail!(
                        "post-install stdout target has unproven ownership: {}",
                        destination.display()
                    )
                }
                let fd = openat(
                    parent.fd(),
                    name.as_os_str(),
                    OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )?;
                let opened = fstat(&fd)?;
                if !SFlag::from_bits_truncate(opened.st_mode).contains(SFlag::S_IFREG)
                    || (opened.st_dev, opened.st_ino) != (stat.st_dev, stat.st_ino)
                {
                    bail!(
                        "post-install shared output changed while binding: {}",
                        destination.display()
                    )
                }
                let file = File::from(fd);
                let sha256 = open_file_sha256(&file)?;
                BoundRunLeafAuthority::Existing(BoundRunFileIdentity {
                    metadata_sha256: run_file_metadata_sha256(&file)?,
                    file,
                    sha256,
                    device: stat.st_dev,
                    inode: stat.st_ino,
                    mode: stat.st_mode as nix::libc::mode_t & 0o7777,
                    uid: stat.st_uid,
                    gid: stat.st_gid,
                })
            }
        };
        if !stdout && let BoundRunLeafAuthority::Existing(identity) = &authority {
            let mut staged = create_bound_run_file(&staging_parent, &staging_name)?;
            copy_open_file_to(&identity.file, &mut staged)?;
            copy_run_file_metadata(&identity.file, &staged)?;
            staged.sync_all()?;
        }
        Ok(Self {
            destination: destination.to_path_buf(),
            staging,
            parent,
            name,
            staging_parent,
            staging_name,
            rollback_parent,
            authority,
            stdout,
        })
    }

    fn validate(&self) -> Result<()> {
        self.parent.validate()?;
        self.staging_parent.validate()?;
        self.rollback_parent.validate()?;
        validate_bound_run_leaf(self.parent.fd(), &self.name, &self.authority).wrap_err_with(|| {
            format!(
                "post-install shared output changed after preflight: {}",
                self.destination.display()
            )
        })
    }

    fn open_stdout(&self) -> Result<File> {
        create_bound_run_file(&self.staging_parent, &self.staging_name)
    }
}

#[cfg(unix)]
#[derive(Debug)]
struct RunSharedWrites {
    outputs: Vec<RunSharedOutput>,
    _staging: tempfile::TempDir,
}

#[cfg(unix)]
impl RunSharedWrites {
    fn validate(&self) -> Result<()> {
        for output in &self.outputs {
            output.validate()?;
        }
        Ok(())
    }

    fn rewrite_args(&self, args: &[String]) -> Vec<String> {
        args.iter()
            .map(|argument| {
                let path = super::pour::lexical_normalize(Path::new(argument));
                self.outputs
                    .iter()
                    .find(|output| !output.stdout && output.destination == path)
                    .map_or_else(
                        || argument.clone(),
                        |output| output.staging.display().to_string(),
                    )
            })
            .collect()
    }

    fn open_stdout(&self, destination: &Path) -> Result<File> {
        let output = self
            .outputs
            .iter()
            .find(|output| output.stdout && output.destination == destination)
            .ok_or_else(|| eyre!("post-install shared stdout was not prepared"))?;
        output.open_stdout()
    }

    fn stdout_tail(&self, destination: &Path) -> Result<String> {
        let output = self
            .outputs
            .iter()
            .find(|output| output.stdout && output.destination == destination)
            .ok_or_else(|| eyre!("post-install shared stdout was not prepared"))?;
        let Some(mut identity) = open_bound_run_file(&output.staging_parent, &output.staging_name)?
        else {
            return Ok(String::new());
        };
        log_tail_file(&mut identity.file)
    }

    fn publish(&mut self) -> Result<RunPublishResult> {
        publish_run_outputs(&self.outputs)
    }
}

#[cfg(not(unix))]
#[derive(Debug)]
struct RunSharedWrites;

#[cfg(not(unix))]
impl RunSharedWrites {
    fn validate(&self) -> Result<()> {
        Ok(())
    }

    fn rewrite_args(&self, args: &[String]) -> Vec<String> {
        args.to_vec()
    }

    fn open_stdout(&self, _destination: &Path) -> Result<File> {
        bail!("shared post-install stdout is unsupported on this platform")
    }

    fn stdout_tail(&self, _destination: &Path) -> Result<String> {
        Ok(String::new())
    }

    fn publish(&mut self) -> Result<RunPublishResult> {
        Ok(RunPublishResult::default())
    }
}

#[cfg(unix)]
fn prepare_run_shared_writes(
    keg: &Path,
    targets: &BTreeSet<PathBuf>,
    stdout: Option<&Path>,
    private_root: &Path,
) -> Result<RunSharedWrites> {
    let staging = tempfile::Builder::new()
        .prefix("shared-outputs-")
        .tempdir_in(private_root)?;
    let mut outputs = vec![];
    for target in targets {
        if path_is_within_keg(keg, target) {
            continue;
        }
        let output = RunSharedOutput::prepare(
            target,
            run_shared_staging_path(staging.path(), target)?,
            staging.path(),
            false,
            false,
        )?;
        outputs.push(output);
    }
    if let Some(stdout) = stdout {
        if outputs.iter().any(|output| output.destination == stdout) {
            bail!(
                "post-install stdout duplicates a shared argument target: {}",
                stdout.display()
            )
        }
        outputs.push(RunSharedOutput::prepare(
            stdout,
            run_shared_staging_path(staging.path(), stdout)?,
            staging.path(),
            true,
            path_is_within_keg(keg, stdout),
        )?);
    }
    Ok(RunSharedWrites {
        outputs,
        _staging: staging,
    })
}

#[cfg(not(unix))]
fn prepare_run_shared_writes(
    keg: &Path,
    targets: &BTreeSet<PathBuf>,
    stdout: Option<&Path>,
    _private_root: &Path,
) -> Result<RunSharedWrites> {
    if targets
        .iter()
        .any(|target| !path_is_within_keg(keg, target))
        || stdout.is_some_and(|path| !path_is_within_keg(keg, path))
    {
        bail!("shared post-install outputs are unsupported on this platform")
    }
    Ok(RunSharedWrites)
}

#[cfg(unix)]
fn run_shared_staging_path(staging: &Path, destination: &Path) -> Result<PathBuf> {
    let shared = super::pour::lexical_normalize(&prefix::prefix());
    let destination = super::pour::lexical_normalize(destination);
    let relative = destination.strip_prefix(&shared).wrap_err_with(|| {
        format!(
            "post-install shared output escapes {}: {}",
            shared.display(),
            destination.display()
        )
    })?;
    if relative.as_os_str().is_empty() {
        bail!("post-install shared output cannot replace the shared prefix")
    }
    Ok(staging.join("prefix").join(relative))
}

#[cfg(unix)]
fn open_file_sha256(file: &File) -> Result<String> {
    let mut file = file.try_clone()?;
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

#[cfg(unix)]
fn create_bound_run_file(parent: &BoundRunSharedParent, name: &std::ffi::OsStr) -> Result<File> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;

    let fd = openat(
        parent.fd(),
        name,
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::S_IRUSR | Mode::S_IWUSR,
    )?;
    Ok(File::from(fd))
}

#[cfg(unix)]
fn create_unique_run_log(parent: &BoundRunSharedParent, prefix: &str) -> Result<(File, PathBuf)> {
    loop {
        let name =
            std::ffi::OsString::from(format!("{prefix}{}.log", crate::rand::random_string(16)));
        match create_bound_run_file(parent, &name) {
            Ok(file) => {
                parent.sync()?;
                return Ok((file, parent.path().join(name)));
            }
            Err(error)
                if error
                    .downcast_ref::<nix::errno::Errno>()
                    .is_some_and(|error| *error == nix::errno::Errno::EEXIST) => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn copy_open_file_to(source: &File, destination: &mut File) -> Result<()> {
    let mut source = source.try_clone()?;
    source.seek(std::io::SeekFrom::Start(0))?;
    destination.seek(std::io::SeekFrom::Start(0))?;
    destination.set_len(0)?;
    std::io::copy(&mut source, destination)?;
    destination.flush()?;
    Ok(())
}

#[cfg(unix)]
fn validate_bound_run_leaf(
    parent: &std::os::fd::OwnedFd,
    name: &std::ffi::OsStr,
    expected: &BoundRunLeafAuthority,
) -> Result<()> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::{Mode, SFlag, fstat, fstatat};

    let stat = match fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => Some(stat),
        Err(nix::errno::Errno::ENOENT) => None,
        Err(error) => return Err(error.into()),
    };
    match (expected, stat) {
        (BoundRunLeafAuthority::Missing, None) => Ok(()),
        (BoundRunLeafAuthority::Missing, Some(_)) => {
            bail!("post-install shared output appeared after preflight")
        }
        (BoundRunLeafAuthority::Existing(_), None) => {
            bail!("post-install shared output disappeared after preflight")
        }
        (BoundRunLeafAuthority::Existing(expected), Some(stat)) => {
            if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFREG)
                || (stat.st_dev, stat.st_ino) != (expected.device, expected.inode)
            {
                bail!("post-install shared output identity changed after preflight")
            }
            let retained = fstat(&expected.file)?;
            if !SFlag::from_bits_truncate(retained.st_mode).contains(SFlag::S_IFREG)
                || (retained.st_dev, retained.st_ino) != (expected.device, expected.inode)
                || (retained.st_mode as nix::libc::mode_t & 0o7777) != expected.mode
                || retained.st_uid != expected.uid
                || retained.st_gid != expected.gid
                || open_file_sha256(&expected.file)? != expected.sha256
                || run_file_metadata_sha256(&expected.file)? != expected.metadata_sha256
            {
                bail!("post-install shared output contents changed after preflight")
            }
            let current = openat(
                parent,
                name,
                OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )?;
            let current = File::from(current);
            let opened = fstat(&current)?;
            if (opened.st_dev, opened.st_ino) != (expected.device, expected.inode)
                || open_file_sha256(&current)? != expected.sha256
                || run_file_metadata_sha256(&current)? != expected.metadata_sha256
            {
                bail!("post-install shared output changed while validating")
            }
            Ok(())
        }
    }
}

#[cfg(unix)]
fn open_bound_run_file(
    parent: &BoundRunSharedParent,
    name: &std::ffi::OsStr,
) -> Result<Option<BoundRunFileIdentity>> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::{Mode, SFlag, fstat, fstatat};

    let stat = match fstatat(parent.fd(), name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFREG) {
        bail!("bound post-install output is not a regular file")
    }
    let fd = openat(
        parent.fd(),
        name,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )?;
    let opened = fstat(&fd)?;
    if (opened.st_dev, opened.st_ino) != (stat.st_dev, stat.st_ino)
        || !SFlag::from_bits_truncate(opened.st_mode).contains(SFlag::S_IFREG)
    {
        bail!("bound post-install output changed while opening")
    }
    let file = File::from(fd);
    Ok(Some(BoundRunFileIdentity {
        sha256: open_file_sha256(&file)?,
        metadata_sha256: run_file_metadata_sha256(&file)?,
        file,
        device: stat.st_dev,
        inode: stat.st_ino,
        mode: stat.st_mode as nix::libc::mode_t & 0o7777,
        uid: stat.st_uid,
        gid: stat.st_gid,
    }))
}

#[cfg(unix)]
fn lifecycle_run_file(path: &Path, identity: &BoundRunFileIdentity) -> Result<LifecycleRunFile> {
    Ok(LifecycleRunFile {
        path: path.to_path_buf(),
        sha256: identity.sha256.clone(),
        metadata_sha256: Some(identity.metadata_sha256.clone()),
        device: run_file_device(identity.device)?,
        inode: identity.inode,
        mode: run_file_mode(identity.mode)?,
    })
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn run_file_device(device: nix::libc::dev_t) -> Result<u64> {
    u64::try_from(device).map_err(|_| eyre!("post-install output has an invalid device identity"))
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
fn run_file_device(device: nix::libc::dev_t) -> Result<u64> {
    Ok(device)
}

#[cfg(all(unix, target_os = "macos"))]
fn run_file_mode(mode: nix::libc::mode_t) -> Result<u32> {
    Ok(u32::from(mode))
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android", target_os = "macos"))
))]
fn run_file_mode(mode: nix::libc::mode_t) -> Result<u32> {
    u32::try_from(mode).map_err(|_| eyre!("post-install output has an invalid file mode"))
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
fn run_file_mode(mode: nix::libc::mode_t) -> Result<u32> {
    Ok(mode)
}

#[cfg(unix)]
fn validate_retained_run_file(expected: &BoundRunFileIdentity) -> Result<()> {
    use nix::sys::stat::{SFlag, fstat};

    let stat = fstat(&expected.file)?;
    if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFREG)
        || (stat.st_dev, stat.st_ino) != (expected.device, expected.inode)
        || (stat.st_mode as nix::libc::mode_t & 0o7777) != expected.mode
        || stat.st_uid != expected.uid
        || stat.st_gid != expected.gid
        || open_file_sha256(&expected.file)? != expected.sha256
        || run_file_metadata_sha256(&expected.file)? != expected.metadata_sha256
    {
        bail!("retained post-install regular-file identity changed")
    }
    Ok(())
}

#[cfg(unix)]
fn validate_bound_run_file_identity(
    parent: &BoundRunSharedParent,
    name: &std::ffi::OsStr,
    expected: &BoundRunFileIdentity,
) -> Result<()> {
    parent.validate()?;
    validate_retained_run_file(expected)?;
    let current = open_bound_run_file(parent, name)?
        .ok_or_else(|| eyre!("bound post-install regular file disappeared"))?;
    if (current.device, current.inode) != (expected.device, expected.inode)
        || current.sha256 != expected.sha256
        || current.mode != expected.mode
        || current.uid != expected.uid
        || current.gid != expected.gid
        || current.metadata_sha256 != expected.metadata_sha256
    {
        bail!("bound post-install regular-file identity changed")
    }
    Ok(())
}

#[cfg(unix)]
fn bound_run_file_is_missing(
    parent: &BoundRunSharedParent,
    name: &std::ffi::OsStr,
) -> Result<bool> {
    use nix::sys::stat::fstatat;

    parent.validate()?;
    match fstatat(parent.fd(), name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Err(nix::errno::Errno::ENOENT) => Ok(true),
        Err(error) => Err(error.into()),
        Ok(_) => Ok(false),
    }
}

#[cfg(unix)]
fn copy_run_file_xattrs(source: &File, destination: &File) -> Result<()> {
    use std::os::fd::AsRawFd;

    #[cfg(target_os = "macos")]
    let count = unsafe { nix::libc::flistxattr(source.as_raw_fd(), std::ptr::null_mut(), 0, 0) };
    #[cfg(not(target_os = "macos"))]
    let count = unsafe { nix::libc::flistxattr(source.as_raw_fd(), std::ptr::null_mut(), 0) };
    if count == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    if count == 0 {
        return Ok(());
    }
    let mut names = vec![0_u8; usize::try_from(count)?];
    #[cfg(target_os = "macos")]
    let listed = unsafe {
        nix::libc::flistxattr(
            source.as_raw_fd(),
            names.as_mut_ptr().cast(),
            names.len(),
            0,
        )
    };
    #[cfg(not(target_os = "macos"))]
    let listed = unsafe {
        nix::libc::flistxattr(source.as_raw_fd(), names.as_mut_ptr().cast(), names.len())
    };
    if listed == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    names.truncate(usize::try_from(listed)?);
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = std::ffi::CString::new(name)?;
        #[cfg(target_os = "macos")]
        let size = unsafe {
            nix::libc::fgetxattr(
                source.as_raw_fd(),
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let size = unsafe {
            nix::libc::fgetxattr(source.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0)
        };
        if size == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut value = vec![0_u8; usize::try_from(size)?];
        #[cfg(target_os = "macos")]
        let read = unsafe {
            nix::libc::fgetxattr(
                source.as_raw_fd(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let read = unsafe {
            nix::libc::fgetxattr(
                source.as_raw_fd(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        if read == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        value.truncate(usize::try_from(read)?);
        #[cfg(target_os = "macos")]
        let set = unsafe {
            nix::libc::fsetxattr(
                destination.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let set = unsafe {
            nix::libc::fsetxattr(
                destination.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        if set == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn run_file_xattrs(file: &File) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
    use std::os::fd::AsRawFd;

    #[cfg(target_os = "macos")]
    let count = unsafe { nix::libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0, 0) };
    #[cfg(not(target_os = "macos"))]
    let count = unsafe { nix::libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
    if count == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut names = vec![0_u8; usize::try_from(count)?];
    if names.is_empty() {
        return Ok(BTreeMap::new());
    }
    #[cfg(target_os = "macos")]
    let listed = unsafe {
        nix::libc::flistxattr(file.as_raw_fd(), names.as_mut_ptr().cast(), names.len(), 0)
    };
    #[cfg(not(target_os = "macos"))]
    let listed =
        unsafe { nix::libc::flistxattr(file.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
    if listed == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    names.truncate(usize::try_from(listed)?);
    let mut values = BTreeMap::new();
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let c_name = std::ffi::CString::new(name)?;
        #[cfg(target_os = "macos")]
        let size = unsafe {
            nix::libc::fgetxattr(
                file.as_raw_fd(),
                c_name.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let size = unsafe {
            nix::libc::fgetxattr(file.as_raw_fd(), c_name.as_ptr(), std::ptr::null_mut(), 0)
        };
        if size == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut value = vec![0_u8; usize::try_from(size)?];
        #[cfg(target_os = "macos")]
        let read = unsafe {
            nix::libc::fgetxattr(
                file.as_raw_fd(),
                c_name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let read = unsafe {
            nix::libc::fgetxattr(
                file.as_raw_fd(),
                c_name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        if read == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        value.truncate(usize::try_from(read)?);
        values.insert(name.to_vec(), value);
    }
    Ok(values)
}

#[cfg(unix)]
fn run_file_metadata_sha256(file: &File) -> Result<String> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata()?;
    let mut digest = Sha256::new();
    digest.update(b"mise-lifecycle-run-file-metadata-v1\0");
    digest.update((metadata.mode() & 0o7777).to_le_bytes());
    digest.update(metadata.mtime().to_le_bytes());
    digest.update(metadata.mtime_nsec().to_le_bytes());
    #[cfg(target_os = "macos")]
    digest.update(std::os::macos::fs::MetadataExt::st_flags(&metadata).to_le_bytes());
    for (name, value) in run_file_xattrs(file)? {
        digest.update(u64::try_from(name.len())?.to_le_bytes());
        digest.update(name);
        digest.update(u64::try_from(value.len())?.to_le_bytes());
        digest.update(value);
    }
    Ok(hex::encode(digest.finalize()))
}

#[cfg(unix)]
fn run_file_metadata_matches(
    left: &BoundRunFileIdentity,
    right: &BoundRunFileIdentity,
) -> Result<bool> {
    Ok(left.metadata_sha256 == right.metadata_sha256)
}

#[cfg(unix)]
fn copy_run_file_metadata(source: &File, destination: &File) -> Result<()> {
    use nix::sys::stat::{Mode, fchmod, fstat, futimens};
    use nix::sys::time::TimeSpec;
    use std::os::unix::fs::MetadataExt;

    let stat = fstat(source)?;
    // Ownership is intentionally inherited from the real destination parent.
    // The source lives under a private mirror whose parent ownership is not the
    // ownership the command would have received beside the shared target.
    fchmod(
        destination,
        Mode::from_bits_truncate(stat.st_mode as nix::libc::mode_t & 0o7777),
    )?;
    copy_run_file_xattrs(source, destination)?;
    let metadata = source.metadata()?;
    futimens(
        destination,
        &TimeSpec::new(metadata.atime(), metadata.atime_nsec()),
        &TimeSpec::new(metadata.mtime(), metadata.mtime_nsec()),
    )?;
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        let flags = std::os::macos::fs::MetadataExt::st_flags(&metadata);
        if unsafe { nix::libc::fchflags(destination.as_raw_fd(), flags) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    if run_file_metadata_sha256(source)? != run_file_metadata_sha256(destination)? {
        bail!("post-install regular-file metadata could not be preserved exactly")
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Debug)]
struct BoundAdjacentRunFile {
    parent: std::os::fd::OwnedFd,
    name: std::ffi::OsString,
    file: File,
    cleanup_identity: Option<(nix::libc::dev_t, nix::libc::ino_t)>,
}

#[cfg(unix)]
impl BoundAdjacentRunFile {
    fn create(parent: &BoundRunSharedParent) -> Result<Self> {
        use nix::fcntl::{OFlag, openat};
        use nix::sys::stat::{Mode, fstat};

        let parent_fd = nix::unistd::dup(parent.fd())?;
        loop {
            let name = std::ffi::OsString::from(format!(
                ".mise-lifecycle-run-{}",
                crate::rand::random_string(16)
            ));
            let fd = match openat(
                &parent_fd,
                name.as_os_str(),
                OFlag::O_RDWR
                    | OFlag::O_CREAT
                    | OFlag::O_EXCL
                    | OFlag::O_NOFOLLOW
                    | OFlag::O_CLOEXEC,
                Mode::S_IRUSR | Mode::S_IWUSR,
            ) {
                Ok(fd) => fd,
                Err(nix::errno::Errno::EEXIST) => continue,
                Err(error) => return Err(error.into()),
            };
            // Install the cleanup guard immediately after the creating openat.
            // Even a later fstat/copy/metadata failure cannot leak this node.
            let mut adjacent = Self {
                parent: parent_fd,
                name,
                file: File::from(fd),
                cleanup_identity: None,
            };
            let stat = fstat(&adjacent.file)?;
            adjacent.cleanup_identity = Some((stat.st_dev, stat.st_ino));
            parent.sync()?;
            return Ok(adjacent);
        }
    }

    fn identity(&self) -> Result<BoundRunFileIdentity> {
        use nix::sys::stat::fstat;

        let file = self.file.try_clone()?;
        let stat = fstat(&file)?;
        Ok(BoundRunFileIdentity {
            sha256: open_file_sha256(&file)?,
            metadata_sha256: run_file_metadata_sha256(&file)?,
            file,
            device: stat.st_dev,
            inode: stat.st_ino,
            mode: stat.st_mode as nix::libc::mode_t & 0o7777,
            uid: stat.st_uid,
            gid: stat.st_gid,
        })
    }

    fn link_existing(
        source_parent: &BoundRunSharedParent,
        source_name: &std::ffi::OsStr,
        destination_parent: &BoundRunSharedParent,
        expected: &BoundRunFileIdentity,
    ) -> Result<Self> {
        let parent_fd = nix::unistd::dup(destination_parent.fd())?;
        let file = expected.file.try_clone()?;
        validate_bound_run_file_identity(source_parent, source_name, expected)?;
        loop {
            let name = std::ffi::OsString::from(format!(
                ".mise-lifecycle-rollback-{}",
                crate::rand::random_string(16)
            ));
            match nix::unistd::linkat(
                source_parent.fd(),
                source_name,
                &parent_fd,
                name.as_os_str(),
                nix::fcntl::AtFlags::empty(),
            ) {
                Ok(()) => {
                    // Install the cleanup guard before any fallible work after
                    // link creation.
                    let rollback = Self {
                        parent: parent_fd,
                        name,
                        file,
                        cleanup_identity: Some((expected.device, expected.inode)),
                    };
                    nix::unistd::fsync(&rollback.parent)?;
                    validate_bound_run_file_identity(destination_parent, &rollback.name, expected)?;
                    return Ok(rollback);
                }
                Err(nix::errno::Errno::EEXIST) => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn set_cleanup_identity(&mut self, identity: &BoundRunFileIdentity) {
        self.cleanup_identity = Some((identity.device, identity.inode));
    }

    fn disarm(&mut self) {
        self.cleanup_identity = None;
    }

    fn cleanup(&mut self) -> Result<()> {
        let Some(identity) = self.cleanup_identity else {
            return Ok(());
        };
        unlink_bound_run_file(&self.parent, &self.name, identity)?;
        nix::unistd::fsync(&self.parent)?;
        self.cleanup_identity = None;
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for BoundAdjacentRunFile {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(unix)]
fn prepare_adjacent_run_file(
    output: &RunSharedOutput,
    staged: Option<&BoundRunFileIdentity>,
) -> Result<BoundAdjacentRunFile> {
    let mut adjacent = BoundAdjacentRunFile::create(&output.parent)?;
    if let Some(staged) = staged {
        validate_retained_run_file(staged)?;
        copy_open_file_to(&staged.file, &mut adjacent.file)?;
        copy_run_file_metadata(&staged.file, &adjacent.file)?;
        validate_retained_run_file(staged)?;
    }
    adjacent.file.sync_all()?;
    Ok(adjacent)
}

#[derive(Debug, Default)]
struct RunPublishResult {
    files: Vec<LifecycleRunFile>,
    deleted: Vec<PathBuf>,
}

#[cfg(unix)]
#[derive(Debug)]
enum RunPublishPhase {
    Prepared,
    Created,
    Swapped,
    TombstoneUnlinked,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunPublishCheckpoint {
    CreateLink,
    ReplaceExchange,
    DeleteExchange,
    DeleteUnlink,
    ParentSync,
    Validation,
    RollbackParentSync,
}

#[cfg(all(unix, test))]
thread_local! {
    static RUN_PUBLISH_FAULT: std::cell::Cell<Option<RunPublishCheckpoint>> = const {
        std::cell::Cell::new(None)
    };
}

#[cfg(unix)]
fn run_publish_checkpoint(checkpoint: RunPublishCheckpoint) -> Result<()> {
    #[cfg(test)]
    if RUN_PUBLISH_FAULT.get() == Some(checkpoint) {
        RUN_PUBLISH_FAULT.set(None);
        bail!("injected post-install publish failure at {checkpoint:?}")
    }
    #[cfg(not(test))]
    let _ = checkpoint;
    Ok(())
}

#[cfg(unix)]
#[derive(Debug)]
enum RunPublishAction {
    Absent,
    Unchanged {
        staged: BoundRunFileIdentity,
    },
    Create {
        staged: BoundRunFileIdentity,
        adjacent: BoundAdjacentRunFile,
        new: BoundRunFileIdentity,
        phase: RunPublishPhase,
    },
    Replace {
        staged: BoundRunFileIdentity,
        adjacent: BoundAdjacentRunFile,
        rollback: Option<BoundAdjacentRunFile>,
        new: BoundRunFileIdentity,
        phase: RunPublishPhase,
    },
    Delete {
        adjacent: BoundAdjacentRunFile,
        rollback: Option<BoundAdjacentRunFile>,
        tombstone: BoundRunFileIdentity,
        phase: RunPublishPhase,
    },
}

#[cfg(unix)]
#[derive(Debug)]
struct PreparedRunPublish {
    output: usize,
    action: RunPublishAction,
}

#[cfg(unix)]
impl PreparedRunPublish {
    fn prepare(output: usize, target: &RunSharedOutput) -> Result<Self> {
        target.validate()?;
        let staged = open_bound_run_file(&target.staging_parent, &target.staging_name)?;
        let action = match (&target.authority, staged) {
            (BoundRunLeafAuthority::Missing, None) => {
                if target.stdout {
                    bail!(
                        "post-install stdout did not produce its declared output: {}",
                        target.destination.display()
                    )
                }
                RunPublishAction::Absent
            }
            (BoundRunLeafAuthority::Missing, Some(staged)) => {
                let adjacent = prepare_adjacent_run_file(target, Some(&staged))?;
                let new = adjacent.identity()?;
                RunPublishAction::Create {
                    staged,
                    adjacent,
                    new,
                    phase: RunPublishPhase::Prepared,
                }
            }
            (BoundRunLeafAuthority::Existing(existing), Some(staged))
                if existing.sha256 == staged.sha256
                    && run_file_metadata_matches(existing, &staged)? =>
            {
                RunPublishAction::Unchanged { staged }
            }
            (BoundRunLeafAuthority::Existing(_), Some(staged)) => {
                let adjacent = prepare_adjacent_run_file(target, Some(&staged))?;
                let new = adjacent.identity()?;
                RunPublishAction::Replace {
                    staged,
                    adjacent,
                    rollback: None,
                    new,
                    phase: RunPublishPhase::Prepared,
                }
            }
            (BoundRunLeafAuthority::Existing(_), None) => {
                let adjacent = prepare_adjacent_run_file(target, None)?;
                let tombstone = adjacent.identity()?;
                RunPublishAction::Delete {
                    adjacent,
                    rollback: None,
                    tombstone,
                    phase: RunPublishPhase::Prepared,
                }
            }
        };
        Ok(Self { output, action })
    }

    fn prevalidate(&self, target: &RunSharedOutput) -> Result<()> {
        target.validate()?;
        match &self.action {
            RunPublishAction::Absent => {
                if !bound_run_file_is_missing(&target.staging_parent, &target.staging_name)? {
                    bail!("post-install staging output appeared after precommit")
                }
            }
            RunPublishAction::Unchanged { staged }
            | RunPublishAction::Create { staged, .. }
            | RunPublishAction::Replace { staged, .. } => {
                validate_bound_run_file_identity(
                    &target.staging_parent,
                    &target.staging_name,
                    staged,
                )?;
            }
            RunPublishAction::Delete { .. } => {
                if !bound_run_file_is_missing(&target.staging_parent, &target.staging_name)? {
                    bail!("deleted post-install staging output reappeared after precommit")
                }
            }
        }
        Ok(())
    }

    fn apply(&mut self, target: &RunSharedOutput) -> Result<()> {
        match &mut self.action {
            RunPublishAction::Absent | RunPublishAction::Unchanged { .. } => Ok(()),
            RunPublishAction::Create {
                adjacent,
                new,
                phase,
                ..
            } => {
                validate_bound_run_file_identity(&target.parent, &adjacent.name, new)?;
                validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority)?;
                nix::unistd::linkat(
                    target.parent.fd(),
                    adjacent.name.as_os_str(),
                    target.parent.fd(),
                    target.name.as_os_str(),
                    nix::fcntl::AtFlags::empty(),
                )?;
                *phase = RunPublishPhase::Created;
                run_publish_checkpoint(RunPublishCheckpoint::CreateLink)?;
                target.parent.sync()?;
                run_publish_checkpoint(RunPublishCheckpoint::ParentSync)?;
                validate_bound_run_file_identity(&target.parent, &target.name, new)?;
                run_publish_checkpoint(RunPublishCheckpoint::Validation)?;
                Ok(())
            }
            RunPublishAction::Replace {
                adjacent,
                new,
                phase,
                ..
            } => {
                validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority)?;
                validate_bound_run_file_identity(&target.parent, &adjacent.name, new)?;
                adjacent.disarm();
                if let Err(error) = rename_exchange_at(
                    target.parent.fd(),
                    adjacent.name.as_os_str(),
                    target.parent.fd(),
                    target.name.as_os_str(),
                ) {
                    if validate_bound_run_file_identity(&target.parent, &adjacent.name, new).is_ok()
                    {
                        adjacent.set_cleanup_identity(new);
                    }
                    return Err(error);
                }
                *phase = RunPublishPhase::Swapped;
                run_publish_checkpoint(RunPublishCheckpoint::ReplaceExchange)?;
                let destination_is_new =
                    validate_bound_run_file_identity(&target.parent, &target.name, new).is_ok();
                let backup_is_old =
                    validate_bound_run_leaf(target.parent.fd(), &adjacent.name, &target.authority)
                        .is_ok();
                if !destination_is_new || !backup_is_old {
                    // A rollback exchange is safe only when both sides still
                    // have the exact identities produced by our exchange.
                    // Never move an unexpected/foreign node a second time.
                    bail!(
                        "post-install replacement identities changed during atomic exchange: {}",
                        target.destination.display()
                    )
                }
                run_publish_checkpoint(RunPublishCheckpoint::Validation)?;
                let BoundRunLeafAuthority::Existing(old) = &target.authority else {
                    unreachable!()
                };
                adjacent.set_cleanup_identity(old);
                target.parent.sync()?;
                run_publish_checkpoint(RunPublishCheckpoint::ParentSync)?;
                Ok(())
            }
            RunPublishAction::Delete {
                adjacent,
                tombstone,
                phase,
                ..
            } => {
                validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority)?;
                validate_bound_run_file_identity(&target.parent, &adjacent.name, tombstone)?;
                adjacent.disarm();
                if let Err(error) = rename_exchange_at(
                    target.parent.fd(),
                    adjacent.name.as_os_str(),
                    target.parent.fd(),
                    target.name.as_os_str(),
                ) {
                    if validate_bound_run_file_identity(&target.parent, &adjacent.name, tombstone)
                        .is_ok()
                    {
                        adjacent.set_cleanup_identity(tombstone);
                    }
                    return Err(error);
                }
                *phase = RunPublishPhase::Swapped;
                run_publish_checkpoint(RunPublishCheckpoint::DeleteExchange)?;
                let destination_is_tombstone =
                    validate_bound_run_file_identity(&target.parent, &target.name, tombstone)
                        .is_ok();
                let backup_is_old =
                    validate_bound_run_leaf(target.parent.fd(), &adjacent.name, &target.authority)
                        .is_ok();
                if !destination_is_tombstone || !backup_is_old {
                    bail!(
                        "post-install deletion identities changed during atomic exchange: {}",
                        target.destination.display()
                    )
                }
                run_publish_checkpoint(RunPublishCheckpoint::Validation)?;
                let BoundRunLeafAuthority::Existing(old) = &target.authority else {
                    unreachable!()
                };
                adjacent.set_cleanup_identity(old);
                unlink_bound_run_file(
                    target.parent.fd(),
                    &target.name,
                    (tombstone.device, tombstone.inode),
                )?;
                *phase = RunPublishPhase::TombstoneUnlinked;
                run_publish_checkpoint(RunPublishCheckpoint::DeleteUnlink)?;
                target.parent.sync()?;
                run_publish_checkpoint(RunPublishCheckpoint::ParentSync)?;
                Ok(())
            }
        }
    }

    fn cleanup_private_rollback(&mut self) -> Result<()> {
        match &mut self.action {
            RunPublishAction::Replace { rollback, .. }
            | RunPublishAction::Delete { rollback, .. } => {
                if let Some(rollback) = rollback {
                    rollback.cleanup()?;
                }
                Ok(())
            }
            RunPublishAction::Absent
            | RunPublishAction::Unchanged { .. }
            | RunPublishAction::Create { .. } => Ok(()),
        }
    }

    fn validate_applied(&self, target: &RunSharedOutput) -> Result<()> {
        target.parent.validate()?;
        target.staging_parent.validate()?;
        match &self.action {
            RunPublishAction::Absent => {
                validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority)
            }
            RunPublishAction::Unchanged { staged } => {
                validate_retained_run_file(staged)?;
                validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority)
            }
            RunPublishAction::Create {
                adjacent,
                new,
                phase,
                ..
            } => {
                if !matches!(phase, RunPublishPhase::Created) {
                    bail!("post-install create was not durably applied")
                }
                validate_retained_run_file(new)?;
                validate_bound_run_file_identity(&target.parent, &target.name, new)?;
                let cleanup = adjacent.cleanup_identity.ok_or_else(|| {
                    eyre!("post-install rollback authority is missing after publish")
                })?;
                let stat = nix::sys::stat::fstatat(
                    &adjacent.parent,
                    adjacent.name.as_os_str(),
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )?;
                if (stat.st_dev, stat.st_ino) != cleanup {
                    bail!("post-install rollback node changed after publish")
                }
                Ok(())
            }
            RunPublishAction::Replace {
                adjacent,
                new,
                phase,
                ..
            } => {
                if !matches!(phase, RunPublishPhase::Swapped) {
                    bail!("post-install replacement was not durably applied")
                }
                validate_retained_run_file(new)?;
                validate_bound_run_file_identity(&target.parent, &target.name, new)?;
                let cleanup = adjacent.cleanup_identity.ok_or_else(|| {
                    eyre!("post-install rollback authority is missing after publish")
                })?;
                let stat = nix::sys::stat::fstatat(
                    &adjacent.parent,
                    adjacent.name.as_os_str(),
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )?;
                if (stat.st_dev, stat.st_ino) != cleanup {
                    bail!("post-install rollback node changed after publish")
                }
                Ok(())
            }
            RunPublishAction::Delete {
                adjacent, phase, ..
            } => {
                if !matches!(phase, RunPublishPhase::TombstoneUnlinked) {
                    bail!("post-install deletion was not durably applied")
                }
                if !bound_run_file_is_missing(&target.parent, &target.name)? {
                    bail!("post-install deletion target reappeared after publish")
                }
                let cleanup = adjacent
                    .cleanup_identity
                    .ok_or_else(|| eyre!("post-install deletion rollback authority is missing"))?;
                let stat = nix::sys::stat::fstatat(
                    &adjacent.parent,
                    adjacent.name.as_os_str(),
                    nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                )?;
                if (stat.st_dev, stat.st_ino) != cleanup {
                    bail!("post-install deletion rollback node changed after publish")
                }
                Ok(())
            }
        }
    }

    fn rollback(&mut self, target: &RunSharedOutput) -> Result<()> {
        match &mut self.action {
            RunPublishAction::Absent | RunPublishAction::Unchanged { .. } => Ok(()),
            RunPublishAction::Create {
                adjacent,
                new,
                phase,
                ..
            } if matches!(phase, RunPublishPhase::Created) => {
                validate_bound_run_file_identity(&target.parent, &target.name, new)?;
                unlink_bound_run_file(target.parent.fd(), &target.name, (new.device, new.inode))?;
                *phase = RunPublishPhase::Prepared;
                target.parent.sync()?;
                adjacent.set_cleanup_identity(new);
                Ok(())
            }
            RunPublishAction::Replace {
                adjacent,
                rollback,
                new,
                phase,
                ..
            } if matches!(phase, RunPublishPhase::Swapped) => {
                let BoundRunLeafAuthority::Existing(old) = &target.authority else {
                    unreachable!()
                };
                // Do not exchange unless both entries are still exactly ours.
                validate_bound_run_file_identity(&target.parent, &target.name, new)?;
                let (backup, backup_parent) = if let Some(rollback) = rollback.as_mut() {
                    (rollback, &target.rollback_parent)
                } else {
                    (adjacent, &target.parent)
                };
                validate_bound_run_leaf(backup_parent.fd(), &backup.name, &target.authority)?;
                backup.disarm();
                if let Err(error) = rename_exchange_at(
                    backup_parent.fd(),
                    backup.name.as_os_str(),
                    target.parent.fd(),
                    target.name.as_os_str(),
                ) {
                    if validate_bound_run_leaf(backup_parent.fd(), &backup.name, &target.authority)
                        .is_ok()
                    {
                        backup.set_cleanup_identity(old);
                    }
                    return Err(error);
                }
                *phase = RunPublishPhase::Prepared;
                let target_is_old =
                    validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority)
                        .is_ok();
                let backup_is_new =
                    validate_bound_run_file_identity(backup_parent, &backup.name, new).is_ok();
                if !target_is_old || !backup_is_new {
                    if backup_is_new {
                        backup.set_cleanup_identity(new);
                    }
                    bail!("post-install rollback identities changed during atomic exchange")
                }
                backup.set_cleanup_identity(new);
                validate_retained_run_file(old)?;
                target.parent.sync()?;
                run_publish_checkpoint(RunPublishCheckpoint::RollbackParentSync)?;
                Ok(())
            }
            RunPublishAction::Delete {
                adjacent,
                rollback,
                tombstone,
                phase,
                ..
            } if matches!(phase, RunPublishPhase::Swapped) => {
                let BoundRunLeafAuthority::Existing(old) = &target.authority else {
                    unreachable!()
                };
                let (backup, backup_parent) = if let Some(rollback) = rollback.as_mut() {
                    (rollback, &target.rollback_parent)
                } else {
                    (adjacent, &target.parent)
                };
                validate_bound_run_file_identity(&target.parent, &target.name, tombstone)?;
                validate_bound_run_leaf(backup_parent.fd(), &backup.name, &target.authority)?;
                backup.disarm();
                if let Err(error) = rename_exchange_at(
                    backup_parent.fd(),
                    backup.name.as_os_str(),
                    target.parent.fd(),
                    target.name.as_os_str(),
                ) {
                    if validate_bound_run_leaf(backup_parent.fd(), &backup.name, &target.authority)
                        .is_ok()
                    {
                        backup.set_cleanup_identity(old);
                    }
                    return Err(error);
                }
                *phase = RunPublishPhase::Prepared;
                let target_is_old =
                    validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority)
                        .is_ok();
                let backup_is_tombstone =
                    validate_bound_run_file_identity(backup_parent, &backup.name, tombstone)
                        .is_ok();
                if !target_is_old || !backup_is_tombstone {
                    if backup_is_tombstone {
                        backup.set_cleanup_identity(tombstone);
                    }
                    bail!("post-install deletion rollback identities changed during exchange")
                }
                backup.set_cleanup_identity(tombstone);
                validate_retained_run_file(old)?;
                target.parent.sync()?;
                run_publish_checkpoint(RunPublishCheckpoint::RollbackParentSync)?;
                Ok(())
            }
            RunPublishAction::Delete {
                adjacent,
                rollback,
                phase,
                ..
            } if matches!(phase, RunPublishPhase::TombstoneUnlinked) => {
                let BoundRunLeafAuthority::Existing(old) = &target.authority else {
                    unreachable!()
                };
                if !bound_run_file_is_missing(&target.parent, &target.name)? {
                    bail!("foreign node appeared at deleted post-install target")
                }
                let (backup, backup_parent) = if let Some(rollback) = rollback.as_mut() {
                    (rollback, &target.rollback_parent)
                } else {
                    (adjacent, &target.parent)
                };
                validate_bound_run_leaf(backup_parent.fd(), &backup.name, &target.authority)?;
                nix::unistd::linkat(
                    backup_parent.fd(),
                    backup.name.as_os_str(),
                    target.parent.fd(),
                    target.name.as_os_str(),
                    nix::fcntl::AtFlags::empty(),
                )?;
                *phase = RunPublishPhase::Prepared;
                validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority)?;
                backup.set_cleanup_identity(old);
                target.parent.sync()?;
                run_publish_checkpoint(RunPublishCheckpoint::RollbackParentSync)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn preserve_predecessor_after_rollback_failure(
        &mut self,
        target: &RunSharedOutput,
    ) -> Result<Option<PathBuf>> {
        let (adjacent, rollback) = match &mut self.action {
            RunPublishAction::Replace {
                adjacent, rollback, ..
            }
            | RunPublishAction::Delete {
                adjacent, rollback, ..
            } => (adjacent, rollback),
            _ => return Ok(None),
        };
        let BoundRunLeafAuthority::Existing(old) = &target.authority else {
            unreachable!()
        };
        if let Some(rollback) = rollback
            && validate_bound_run_leaf(
                target.rollback_parent.fd(),
                &rollback.name,
                &target.authority,
            )
            .is_ok()
        {
            rollback.disarm();
            return Ok(Some(target.rollback_parent.path().join(&rollback.name)));
        }
        if validate_bound_run_leaf(target.parent.fd(), &adjacent.name, &target.authority).is_ok() {
            // Disarm the last exact predecessor before any further fallible
            // work. If the private hardlink cannot be created, the adjacent
            // path remains the surfaced recovery authority instead of Drop
            // deleting it.
            adjacent.disarm();
            let adjacent_path = target.parent.path().join(&adjacent.name);
            let mut recovery = match BoundAdjacentRunFile::link_existing(
                &target.parent,
                &adjacent.name,
                &target.rollback_parent,
                old,
            ) {
                Ok(recovery) => recovery,
                Err(error) => {
                    warn!(
                        "could not move post-install predecessor into private recovery; retaining {}: {error:#}",
                        adjacent_path.display()
                    );
                    return Ok(Some(adjacent_path));
                }
            };
            let path = target.rollback_parent.path().join(&recovery.name);
            recovery.disarm();
            adjacent.set_cleanup_identity(old);
            if let Err(error) = adjacent.cleanup() {
                warn!(
                    "could not remove redundant adjacent post-install predecessor {}: {error:#}",
                    adjacent_path.display()
                );
            }
            return Ok(Some(path));
        }
        if validate_bound_run_leaf(target.parent.fd(), &target.name, &target.authority).is_ok() {
            let mut recovery = match BoundAdjacentRunFile::link_existing(
                &target.parent,
                &target.name,
                &target.rollback_parent,
                old,
            ) {
                Ok(recovery) => recovery,
                Err(error) => {
                    warn!(
                        "could not duplicate restored post-install predecessor into private recovery; retaining {}: {error:#}",
                        target.destination.display()
                    );
                    return Ok(Some(target.destination.clone()));
                }
            };
            let path = target.rollback_parent.path().join(&recovery.name);
            recovery.disarm();
            return Ok(Some(path));
        }
        bail!("no exact predecessor authority remains after rollback failure")
    }

    fn effect(&self, target: &RunSharedOutput) -> Result<Option<LifecycleRunFile>> {
        match &self.action {
            RunPublishAction::Absent | RunPublishAction::Delete { .. } => Ok(None),
            RunPublishAction::Unchanged { .. } => {
                let BoundRunLeafAuthority::Existing(existing) = &target.authority else {
                    unreachable!()
                };
                lifecycle_run_file(&target.destination, existing).map(Some)
            }
            RunPublishAction::Create { new, .. } | RunPublishAction::Replace { new, .. } => {
                lifecycle_run_file(&target.destination, new).map(Some)
            }
        }
    }

    fn cleanup(&mut self) -> Result<()> {
        match &mut self.action {
            RunPublishAction::Create { adjacent, .. }
            | RunPublishAction::Replace { adjacent, .. }
            | RunPublishAction::Delete { adjacent, .. } => adjacent.cleanup(),
            RunPublishAction::Absent | RunPublishAction::Unchanged { .. } => Ok(()),
        }
    }

    fn arm_cleanup_rollback(&mut self, target: &RunSharedOutput) -> Result<()> {
        match &mut self.action {
            RunPublishAction::Replace {
                adjacent,
                rollback,
                phase: RunPublishPhase::Swapped,
                ..
            }
            | RunPublishAction::Delete {
                adjacent,
                rollback,
                phase: RunPublishPhase::TombstoneUnlinked,
                ..
            } => {
                let BoundRunLeafAuthority::Existing(old) = &target.authority else {
                    unreachable!()
                };
                if rollback.is_none() {
                    *rollback = Some(BoundAdjacentRunFile::link_existing(
                        &target.parent,
                        &adjacent.name,
                        &target.rollback_parent,
                        old,
                    )?);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(unix)]
fn rollback_run_publishes(
    publishes: &mut [PreparedRunPublish],
    outputs: &[RunSharedOutput],
) -> Result<()> {
    let mut failures = vec![];
    for publish in publishes.iter_mut().rev() {
        if let Err(error) = publish.rollback(&outputs[publish.output]) {
            let recovery = match publish
                .preserve_predecessor_after_rollback_failure(&outputs[publish.output])
            {
                Ok(Some(path)) => format!("; predecessor retained at {}", path.display()),
                Ok(None) => String::new(),
                Err(preserve_error) => {
                    format!("; predecessor preservation also failed: {preserve_error:#}")
                }
            };
            failures.push(format!(
                "{}: {error:#}{recovery}",
                outputs[publish.output].destination.display(),
            ));
        }
    }
    if !failures.is_empty() {
        bail!(
            "failed to roll back one or more post-install outputs: {}",
            failures.join("; ")
        )
    }
    Ok(())
}

#[cfg(unix)]
fn publish_run_outputs(outputs: &[RunSharedOutput]) -> Result<RunPublishResult> {
    let mut publishes = outputs
        .iter()
        .enumerate()
        .map(|(index, output)| PreparedRunPublish::prepare(index, output))
        .collect::<Result<Vec<_>>>()?;
    for publish in &publishes {
        publish.prevalidate(&outputs[publish.output])?;
    }
    for index in 0..publishes.len() {
        let output = publishes[index].output;
        if let Err(error) = publishes[index].apply(&outputs[output]) {
            let rollback = rollback_run_publishes(&mut publishes, outputs);
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(error).wrap_err_with(|| {
                    format!("post-install output rollback also failed: {rollback_error:#}")
                }),
            };
        }
    }
    if let Err(error) = publishes
        .iter()
        .try_for_each(|publish| publish.validate_applied(&outputs[publish.output]))
    {
        let rollback = rollback_run_publishes(&mut publishes, outputs);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error).wrap_err_with(|| {
                format!("post-install output rollback also failed: {rollback_error:#}")
            }),
        };
    }
    let result = match (|| -> Result<RunPublishResult> {
        Ok(RunPublishResult {
            files: publishes
                .iter()
                .filter_map(|publish| publish.effect(&outputs[publish.output]).transpose())
                .collect::<Result<Vec<_>>>()?,
            deleted: publishes
                .iter()
                .filter(|publish| matches!(publish.action, RunPublishAction::Delete { .. }))
                .map(|publish| outputs[publish.output].destination.clone())
                .collect(),
        })
    })() {
        Ok(result) => result,
        Err(error) => {
            let rollback = rollback_run_publishes(&mut publishes, outputs);
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(error).wrap_err_with(|| {
                    format!(
                        "post-install effect validation rollback also failed: {rollback_error:#}"
                    )
                }),
            };
        }
    };
    // Preserve a second exact link to every predecessor before removing any
    // shared temporary. A later cleanup failure can therefore roll back every
    // output, including outputs whose primary backup was already removed.
    for index in 0..publishes.len() {
        let output = publishes[index].output;
        if let Err(error) = publishes[index].arm_cleanup_rollback(&outputs[output]) {
            let rollback = rollback_run_publishes(&mut publishes, outputs);
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(error).wrap_err_with(|| {
                    format!(
                        "post-install cleanup preparation rollback also failed: {rollback_error:#}"
                    )
                }),
            };
        }
    }
    for publish in &mut publishes {
        if let Err(error) = publish.cleanup() {
            let rollback = rollback_run_publishes(&mut publishes, outputs);
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(error).wrap_err_with(|| {
                    format!("post-install cleanup rollback also failed: {rollback_error:#}")
                }),
            };
        }
    }
    // Secondary authorities live in per-output 0700 directories on the
    // destination filesystem. Cleanup is explicit. If an identity-bound
    // unlink is raced or the filesystem fails after semantic commit, retain
    // that private directory as the safe recovery disposition; never report a
    // shared output failure that can no longer be rolled back atomically.
    for publish in &mut publishes {
        if let Err(error) = publish.cleanup_private_rollback() {
            warn!(
                "failed to clean private post-install rollback authority for {}: {error:#}",
                outputs[publish.output].destination.display()
            );
        }
    }
    Ok(result)
}

#[cfg(unix)]
fn unlink_bound_run_file(
    parent: &std::os::fd::OwnedFd,
    name: &std::ffi::OsStr,
    expected: (nix::libc::dev_t, nix::libc::ino_t),
) -> Result<()> {
    use nix::sys::stat::{SFlag, fstatat};

    let stat = match fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFREG)
        || (stat.st_dev, stat.st_ino) != expected
    {
        bail!("post-install temporary output changed before cleanup")
    }
    nix::unistd::unlinkat(parent, name, nix::unistd::UnlinkatFlags::NoRemoveDir)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn rename_exchange_at(
    left_parent: &std::os::fd::OwnedFd,
    left: &std::ffi::OsStr,
    right_parent: &std::os::fd::OwnedFd,
    right: &std::ffi::OsStr,
) -> Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let left = std::ffi::CString::new(left.as_bytes())?;
    let right = std::ffi::CString::new(right.as_bytes())?;
    let result = unsafe {
        nix::libc::syscall(
            nix::libc::SYS_renameat2,
            left_parent.as_raw_fd(),
            left.as_ptr(),
            right_parent.as_raw_fd(),
            right.as_ptr(),
            nix::libc::RENAME_EXCHANGE,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn rename_exchange_at(
    left_parent: &std::os::fd::OwnedFd,
    left: &std::ffi::OsStr,
    right_parent: &std::os::fd::OwnedFd,
    right: &std::ffi::OsStr,
) -> Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let left = std::ffi::CString::new(left.as_bytes())?;
    let right = std::ffi::CString::new(right.as_bytes())?;
    let result = unsafe {
        nix::libc::renameatx_np(
            left_parent.as_raw_fd(),
            left.as_ptr(),
            right_parent.as_raw_fd(),
            right.as_ptr(),
            nix::libc::RENAME_SWAP,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn rename_exchange_at(
    _left_parent: &std::os::fd::OwnedFd,
    _left: &std::ffi::OsStr,
    _right_parent: &std::os::fd::OwnedFd,
    _right: &std::ffi::OsStr,
) -> Result<()> {
    bail!("atomic shared lifecycle replacement is unsupported on this platform")
}

fn run_environment(
    prepared: &PreparedFormulaLifecycle,
    run: &PreparedRun,
    temp: &Path,
    audited_system_path: bool,
) -> Result<BTreeMap<String, String>> {
    let shared = prefix::prefix();
    let mut path_entries = vec![];
    if !audited_system_path {
        path_entries.extend([prepared.keg.join("bin"), prepared.keg.join("sbin")]);
        #[cfg(not(target_os = "linux"))]
        path_entries.extend([shared.join("bin"), shared.join("sbin")]);
    }
    path_entries.extend([
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]);
    let path = std::env::join_paths(path_entries)?;
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

#[cfg(not(unix))]
fn open_truncated(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?)
}

fn log_tail_file(file: &mut File) -> Result<String> {
    use std::io::{Read, Seek};

    let length = file.metadata()?.len();
    file.seek(std::io::SeekFrom::Start(
        length.saturating_sub(MAX_FAILURE_LOG_BYTES as u64),
    ))?;
    let mut output = Vec::with_capacity(MAX_FAILURE_LOG_BYTES);
    file.take(MAX_FAILURE_LOG_BYTES as u64)
        .read_to_end(&mut output)?;
    Ok(String::from_utf8_lossy(&output).into_owned())
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

    #[cfg(unix)]
    fn set_test_xattr(path: &Path, value: &[u8]) -> Result<()> {
        use std::os::fd::AsRawFd;

        let file = File::options().read(true).write(true).open(path)?;
        let name = c"user.mise-lifecycle-test";
        #[cfg(target_os = "macos")]
        let result = unsafe {
            nix::libc::fsetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let result = unsafe {
            nix::libc::fsetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        if result == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn formula(steps: Vec<Value>) -> Formula {
        serde_json::from_value(serde_json::json!({
            "name": "openssl@3",
            "versions": {"stable": "1"},
            "bottle": {},
            "post_install_steps": steps
        }))
        .unwrap()
    }

    #[cfg(target_os = "linux")]
    fn strict_formula_execution_unavailable(error: &eyre::Report) -> bool {
        error.chain().any(|cause| {
            cause
                .to_string()
                .contains(": strict formula execution is unavailable:")
        })
    }

    #[cfg(target_os = "linux")]
    fn prepare_when_strict_formula_execution_is_available(
        formula: &Formula,
        keg: &Path,
    ) -> Result<Option<PreparedFormulaLifecycle>> {
        match prepare(formula, keg) {
            Ok(prepared) => Ok(Some(prepared)),
            Err(error) if strict_formula_execution_unavailable(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[cfg(target_os = "macos")]
    fn ca_certificates_formula() -> Formula {
        serde_json::from_value(serde_json::json!({
            "name": "ca-certificates",
            "versions": {"stable": "2026-08-13"},
            "bottle": {},
            "ruby_source_checksum": {
                "sha256": AUDITED_CA_CERTIFICATES_FORMULA_SHA256
            },
            "post_install_steps": [{
                "command": {"base": "libexec", "path": "post-install"},
                "type": "run",
                "args": ["{{pkgshare}}/cacert.pem", "{{pkgetc}}/cert.pem"]
            }]
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
    fn log_tail_lossily_reports_binary_output() -> Result<()> {
        let mut file = tempfile::tempfile()?;
        file.write_all(&[b'o', b'k', 0xff])?;

        assert_eq!(log_tail_file(&mut file)?, "ok\u{fffd}");
        Ok(())
    }

    #[test]
    fn log_tail_lossily_reports_a_split_multibyte_boundary() -> Result<()> {
        let mut file = tempfile::tempfile()?;
        let mut bytes = "€".as_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(b'a', MAX_FAILURE_LOG_BYTES - 2));
        file.write_all(&bytes)?;

        let tail = log_tail_file(&mut file)?;

        assert!(tail.starts_with('\u{fffd}'));
        assert!(tail.ends_with('a'));
        Ok(())
    }

    #[test]
    fn log_tail_is_bounded_to_the_sampled_window() -> Result<()> {
        let mut file = tempfile::tempfile()?;
        file.write_all(&vec![b'a'; MAX_FAILURE_LOG_BYTES * 2])?;

        assert_eq!(log_tail_file(&mut file)?.len(), MAX_FAILURE_LOG_BYTES);
        Ok(())
    }

    #[test]
    fn accepts_ca_certificates_and_openssl_steps() {
        let ca: Formula = serde_json::from_value(serde_json::json!({
            "name": "ca-certificates",
            "versions": {"stable": "1"},
            "bottle": {},
            "post_install_steps": [{
                "command": {"base": "libexec", "path": "post-install"},
                "type": "run",
                "args": ["{{pkgshare}}/cacert.pem", "{{pkgetc}}/cert.pem"]
            }]
        }))
        .unwrap();
        #[cfg(target_os = "linux")]
        let ca = {
            let mut ca = ca;
            ca.post_install_steps[0]["guards"] =
                serde_json::json!([{"condition": "on", "value": "macos"}]);
            ca
        };
        #[cfg(target_os = "macos")]
        let ca = {
            let mut ca = ca;
            ca.ruby_source_checksum = Some(super::super::api::RubySourceChecksum {
                sha256: Some(AUDITED_CA_CERTIFICATES_FORMULA_SHA256.into()),
            });
            ca
        };
        let openssl = formula(vec![serde_json::json!({
            "source": {"path": "{{etc}}/ca-certificates/cert.pem"},
            "target": {"path": "{{pkgetc}}/cert.pem"},
            "force": true,
            "type": "symlink"
        })]);
        prepare(&ca, &prefix::cellar().join("ca-certificates/1")).unwrap();
        prepare(&openssl, &prefix::cellar().join("openssl@3/1")).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_rejects_generic_run_but_accepts_exact_audited_ca_shape() {
        let generic = formula(vec![serde_json::json!({
            "type": "run",
            "command": {"path": "/bin/true"}
        })]);
        let error = prepare(&generic, &prefix::cellar().join("openssl@3/1")).unwrap_err();
        assert!(format!("{error:#}").contains("generic macOS process containment"));

        let prepared = prepare(
            &ca_certificates_formula(),
            &prefix::cellar().join("ca-certificates/2026-08-13"),
        )
        .unwrap();
        assert!(matches!(
            prepared.steps.as_slice(),
            [PreparedStep::AuditedCaCertificates(_)]
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_audited_ca_rejects_wrong_bottle_snapshot_during_binding() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/ca-certificates/2026-08-13");
        let target = prefix.join("etc/ca-certificates/cert.pem");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&target, "preserve")?;
        let mut prepared = prepare(&ca_certificates_formula(), &keg)?;
        let error = prepared
            .bind_bottle_formula_snapshot_sha256("wrong-snapshot".into())
            .unwrap_err();

        assert!(format!("{error:#}").contains("bottle formula snapshot is not the audited"));
        assert_eq!(crate::file::read_to_string(&target)?, "preserve");
        assert!(keg.symlink_metadata().is_err());
        assert!(state_path(&keg).symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_audited_ca_binds_embedded_snapshot_not_live_api() -> Result<()> {
        let mut formula = ca_certificates_formula();
        formula.ruby_source_checksum = Some(super::super::api::RubySourceChecksum {
            sha256: Some("mutable-api-snapshot".into()),
        });
        let mut prepared = prepare(
            &formula,
            &prefix::cellar().join("ca-certificates/2026-08-13"),
        )?;

        prepared
            .bind_bottle_formula_snapshot_sha256(AUDITED_CA_CERTIFICATES_FORMULA_SHA256.into())?;

        assert_eq!(
            prepared.formula_snapshot_sha256.as_deref(),
            Some(AUDITED_CA_CERTIFICATES_FORMULA_SHA256)
        );
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_audited_ca_revalidates_formula_snapshot_before_output() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/ca-certificates/2026-08-13");
        let target = prefix.join("etc/ca-certificates/cert.pem");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&target, "preserve")?;
        let mut prepared = prepare(&ca_certificates_formula(), &keg)?;
        prepared.formula_snapshot_sha256 = Some("wrong-snapshot".into());

        let error = match execute_step(&prepared, &prepared.steps[0]).await {
            Err(error) => error,
            Ok(_) => panic!("wrong formula snapshot unexpectedly executed"),
        };

        assert!(format!("{error:#}").contains("formula snapshot is not the audited"));
        assert_eq!(crate::file::read_to_string(&target)?, "preserve");
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_audited_ca_rejects_wrong_helper_before_output() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/ca-certificates/2026-08-13");
        let helper = keg.join("libexec/post-install");
        let source = keg.join("share/ca-certificates/cacert.pem");
        let target = prefix.join("etc/ca-certificates/cert.pem");
        for path in [&helper, &source, &target] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        crate::file::write(&helper, "#!/bin/sh\nexit 0\n")?;
        crate::file::write(&source, "certificate-data")?;
        crate::file::write(&target, "preserve")?;
        let mut prepared = prepare(&ca_certificates_formula(), &keg)?;
        prepared
            .bind_bottle_formula_snapshot_sha256(AUDITED_CA_CERTIFICATES_FORMULA_SHA256.into())?;

        let error = match execute_step(&prepared, &prepared.steps[0]).await {
            Err(error) => error,
            Ok(_) => panic!("wrong audited helper unexpectedly executed"),
        };

        assert!(format!("{error:#}").contains("helper contents changed"));
        assert_eq!(crate::file::read_to_string(&target)?, "preserve");
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_audited_ca_uses_system_only_path_and_read_only_keg() -> Result<()> {
        let prepared = prepare(
            &ca_certificates_formula(),
            &prefix::cellar().join("ca-certificates/2026-08-13"),
        )?;
        let PreparedStep::AuditedCaCertificates(run) = &prepared.steps[0] else {
            unreachable!()
        };
        let env = run_environment(&prepared, run, Path::new("/private/temp"), true)?;
        assert_eq!(env["PATH"], "/usr/bin:/bin:/usr/sbin:/sbin");
        assert!(!env["PATH"].contains(&prefix::prefix().display().to_string()));
        let sandbox = lifecycle_run_sandbox(
            &prepared.keg,
            std::slice::from_ref(&run.executable),
            Path::new("/private/stdout.log"),
            Path::new("/private/stderr.log"),
            Path::new("/private/temp"),
            None,
            false,
        );
        assert!(!sandbox.allow_write.contains(&prepared.keg));
        assert!(sandbox.allow_read.contains(&prepared.keg));
        Ok(())
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
        prepared.bind_bottle_formula_snapshot_sha256(bottle_sha256)?;
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
            run_files: vec![],
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
            run_files: vec![],
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
            run_files: vec![],
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
            run_files: vec![],
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
            run_files: vec![],
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
            run_files: vec![],
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
        first.bind_bottle_formula_snapshot_sha256("snapshot-a".into())?;
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

    #[cfg(target_os = "linux")]
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
        let Some(prepared) = prepare_when_strict_formula_execution_is_available(
            &formula(vec![serde_json::json!({
                "type": "run",
                "command": {"path": "/bin/echo"},
                "args": ["new"],
                "stdout_path": {"base": "pkgetc", "path": "generated"}
            })]),
            &keg,
        )?
        else {
            assert_eq!(crate::file::read_to_string(&target)?, "user-owned");
            return Ok(());
        };

        assert!(execute_step(&prepared, &prepared.steps[0]).await.is_err());
        assert_eq!(crate::file::read_to_string(target)?, "user-owned");
        Ok(())
    }

    #[cfg(target_os = "linux")]
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
                ],
                "guards": [{"condition": "on", "value": "macos"}]
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
    fn run_sandbox_denies_network_and_local_socket_brokers() {
        let config = lifecycle_run_sandbox(
            Path::new("/private/keg"),
            &[PathBuf::from("/private/keg/libexec/post-install")],
            Path::new("/private/stdout.log"),
            Path::new("/private/stderr.log"),
            Path::new("/private/temp"),
            None,
            true,
        );

        assert!(config.deny_net);
        assert!(config.deny_local_sockets);
        assert!(config.deny_write);
        assert!(config.deny_env);
        #[cfg(target_os = "linux")]
        {
            assert!(config.deny_read);
            assert!(config.deny_mise_data_read);
            assert!(config.require_full_filesystem_confinement);
            assert_eq!(
                config.system_access_profile,
                crate::sandbox::SystemAccessProfile::FormulaExecution
            );
        }
        assert!(
            config
                .allow_write
                .contains(&PathBuf::from("/private/stdout.log"))
        );
        assert!(
            config
                .allow_write
                .contains(&PathBuf::from("/private/stderr.log"))
        );
        assert!(!config.allow_write.contains(&PathBuf::from("/private")));
    }

    #[cfg(unix)]
    #[test]
    fn private_run_temp_and_log_binding_reject_symlinked_ancestors() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let outside = crate::file::desymlink_path(&tmp.path().join("outside"));
        let linked = crate::file::desymlink_path(&tmp.path().join("linked"));
        crate::file::create_dir_all(&outside)?;
        crate::file::write(outside.join("sentinel"), "preserve")?;
        crate::file::make_symlink(&outside, &linked)?;

        assert!(BoundRunPrivateTree::create(&linked.join("temp"), "run-").is_err());
        assert!(
            BoundRunSharedParent::open_private_beneath(Path::new("/"), &linked.join("logs"))
                .is_err()
        );
        assert_eq!(
            crate::file::read_to_string(outside.join("sentinel"))?,
            "preserve"
        );
        assert!(outside.join("temp").symlink_metadata().is_err());
        assert!(outside.join("logs").symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn private_run_cleanup_can_distinguish_mount_boundaries() -> Result<()> {
        let root = File::open("/")?;
        let proc = File::open("/proc")?;

        assert_ne!(
            run_directory_mount_identity(&root)?,
            run_directory_mount_identity(&proc)?
        );
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn run_uses_private_mirror_for_adjacent_atomic_output() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let executable = keg.join("libexec/post-install");
        let source = keg.join("share/openssl@3/cacert.pem");
        let target = prefix.join("etc/openssl@3/cert.pem");
        let sibling = prefix.join("etc/sibling-sentinel");
        crate::file::create_dir_all(executable.parent().unwrap())?;
        crate::file::create_dir_all(source.parent().unwrap())?;
        crate::file::create_dir_all(sibling.parent().unwrap())?;
        crate::file::write(&source, "certificate-data")?;
        crate::file::write(&sibling, "preserve")?;
        crate::file::write(
            &executable,
            format!(
                r#"#!/bin/sh
set -eu
[ "$(basename "$2")" = cert.pem ] || exit 98
temporary=$(mktemp "$(dirname "$2")/.ca-certificates.XXXXXX")
cp "$1" "$temporary"
mv "$temporary" "$2"
if printf mutated > "{}" 2>/dev/null; then
  exit 97
fi
"#,
                sibling.display()
            ),
        )?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
        assert!(target.parent().unwrap().symlink_metadata().is_err());
        let Some(prepared) = prepare_when_strict_formula_execution_is_available(
            &formula(vec![serde_json::json!({
                "command": {"base": "libexec", "path": "post-install"},
                "type": "run",
                "args": ["{{pkgshare}}/cacert.pem", "{{pkgetc}}/cert.pem"]
            })]),
            &keg,
        )?
        else {
            assert!(target.parent().unwrap().symlink_metadata().is_err());
            assert_eq!(crate::file::read_to_string(&sibling)?, "preserve");
            return Ok(());
        };

        let effects = execute_step(&prepared, &prepared.steps[0]).await?;

        assert_eq!(crate::file::read_to_string(&target)?, "certificate-data");
        assert_eq!(crate::file::read_to_string(&sibling)?, "preserve");
        assert!(effects.required_paths.contains(&target));
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn lifecycle_run_cannot_read_host_secret_or_escape_process_group() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let secret = crate::file::desymlink_path(&tmp.path().join("host-secret"));
        let escaped = crate::file::desymlink_path(&tmp.path().join("escaped"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let executable = keg.join("libexec/post-install");
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(executable.parent().unwrap())?;
        crate::file::write(&secret, "host-secret")?;
        crate::file::write(
            &executable,
            format!(
                r#"#!/bin/sh
set -eu
if IFS= read -r _ < "{}" 2>/dev/null; then
  exit 91
fi
if command -v setsid >/dev/null 2>&1 && setsid /bin/sh -c 'printf escaped > "{}"'; then
  exit 92
fi
printf confined > "$1"
"#,
                secret.display(),
                escaped.display()
            ),
        )?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
        let Some(prepared) = prepare_when_strict_formula_execution_is_available(
            &formula(vec![serde_json::json!({
                "command": {"base": "libexec", "path": "post-install"},
                "type": "run",
                "args": [target]
            })]),
            &keg,
        )?
        else {
            assert_eq!(crate::file::read_to_string(&secret)?, "host-secret");
            assert!(target.symlink_metadata().is_err());
            assert!(escaped.symlink_metadata().is_err());
            return Ok(());
        };

        let effects = execute_step(&prepared, &prepared.steps[0]).await?;

        assert_eq!(crate::file::read_to_string(&target)?, "confined");
        assert_eq!(crate::file::read_to_string(&secret)?, "host-secret");
        assert!(escaped.symlink_metadata().is_err());
        assert!(effects.required_paths.contains(&target));
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn lifecycle_run_receives_nonempty_bound_stdin_from_offset_zero() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let input = keg.join("input");
        let output = keg.join("output");
        crate::file::create_dir_all(&keg)?;
        crate::file::write(&input, "nonempty-stdin")?;
        let Some(prepared) = prepare_when_strict_formula_execution_is_available(
            &formula(vec![serde_json::json!({
                "command": {"path": "/bin/cat"},
                "type": "run",
                "stdin_path": {"path": "input"},
                "stdout_path": {"path": "output"}
            })]),
            &keg,
        )?
        else {
            assert_eq!(crate::file::read_to_string(&input)?, "nonempty-stdin");
            assert!(output.symlink_metadata().is_err());
            return Ok(());
        };

        execute_step(&prepared, &prepared.steps[0]).await?;

        assert_eq!(crate::file::read_to_string(output)?, "nonempty-stdin");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn run_shared_stdout_uses_bound_parent_and_exact_publish() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        crate::file::create_dir_all(&prefix)?;
        let keg = prefix.join("Cellar/openssl@3/1");
        let target = prefix.join("etc/openssl@3/nested/generated");
        let Some(prepared) = prepare_when_strict_formula_execution_is_available(
            &formula(vec![serde_json::json!({
                "type": "run",
                "command": {"path": "/bin/echo"},
                "args": ["generated"],
                "stdout_path": {"base": "pkgetc", "path": "nested/generated"}
            })]),
            &keg,
        )?
        else {
            assert!(target.symlink_metadata().is_err());
            return Ok(());
        };

        let effects = execute_step(&prepared, &prepared.steps[0]).await?;

        assert_eq!(crate::file::read_to_string(&target)?, "generated\n");
        assert_eq!(effects.required_paths, [target]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_inside_keg_stdout_uses_the_same_bound_publish_transaction() -> Result<()> {
        use std::io::Write;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let target = keg.join("share/generated");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let mut writes =
            prepare_run_shared_writes(&keg, &BTreeSet::new(), Some(&target), &private)?;
        let mut stdout = writes.open_stdout(&target)?;
        stdout.write_all(b"new")?;
        stdout.sync_all()?;
        drop(stdout);

        let published = writes.publish()?;

        assert_eq!(crate::file::read_to_string(&target)?, "new");
        assert_eq!(published.files.len(), 1);
        assert_eq!(published.files[0].path, target);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_publish_rejects_ancestor_swap_without_touching_outside() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let outside = crate::file::desymlink_path(&tmp.path().join("outside"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        crate::file::create_dir_all(prefix.join("etc"))?;
        crate::file::create_dir_all(&outside)?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(outside.join("sentinel"), "preserve")?;
        let keg = prefix.join("Cellar/openssl@3/1");
        let target = prefix.join("etc/openssl@3/generated");
        let mut writes =
            prepare_run_shared_writes(&keg, &BTreeSet::from([target.clone()]), None, &private)?;
        crate::file::write(&writes.outputs[0].staging, "generated")?;

        fs::rename(prefix.join("etc"), prefix.join("etc-bound"))?;
        crate::file::make_symlink(&outside, &prefix.join("etc"))?;

        assert!(writes.publish().is_err());
        assert_eq!(
            crate::file::read_to_string(outside.join("sentinel"))?,
            "preserve"
        );
        assert!(
            outside
                .join("openssl@3/generated")
                .symlink_metadata()
                .is_err()
        );
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn run_shared_publish_replaces_only_bound_regular_leaf() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let mut writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        crate::file::write(&writes.outputs[0].staging, "new")?;

        let published = writes.publish()?;
        assert_eq!(published.files.len(), 1);
        assert_eq!(published.files[0].path, target);
        assert_eq!(crate::file::read_to_string(target)?, "new");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_publish_preserves_unchanged_inode_and_metadata() -> Result<()> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/unchanged");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "same")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640))?;
        let before = target.symlink_metadata()?;
        let mut writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;

        let published = writes.publish()?;

        let after = target.symlink_metadata()?;
        assert_eq!(before.ino(), after.ino());
        assert_eq!(before.permissions().mode(), after.permissions().mode());
        assert_eq!(published.files.len(), 1);
        assert_eq!(published.files[0].inode, after.ino());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_publish_preserves_updated_mode_and_xattrs() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/metadata");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let mut writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        crate::file::write(&writes.outputs[0].staging, "new")?;
        fs::set_permissions(
            &writes.outputs[0].staging,
            fs::Permissions::from_mode(0o640),
        )?;
        set_test_xattr(&writes.outputs[0].staging, b"bound-value")?;

        let published = writes.publish()?;

        let target_file = File::open(&target)?;
        assert_eq!(crate::file::read_to_string(&target)?, "new");
        assert_eq!(
            target.symlink_metadata()?.permissions().mode() & 0o7777,
            0o640
        );
        assert_eq!(
            run_file_xattrs(&target_file)?
                .get(b"user.mise-lifecycle-test".as_slice())
                .map(Vec::as_slice),
            Some(b"bound-value".as_slice())
        );
        assert_eq!(
            published.files[0].sha256,
            crate::hash::file_hash_sha256(&target, None)?
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn typed_run_file_health_requires_regular_identity_mode_and_contents() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        crate::file::create_dir_all(&prefix)?;
        let path = prefix.join("published");
        crate::file::write(&path, "expected")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
        let metadata = path.symlink_metadata()?;
        let (device, inode) = permission_device_inode(&metadata)?;
        let expected = LifecycleRunFile {
            path: path.clone(),
            sha256: crate::hash::file_hash_sha256(&path, None)?,
            metadata_sha256: None,
            device,
            inode,
            mode: metadata.permissions().mode() & 0o7777,
        };
        assert!(lifecycle_run_file_matches(&expected)?);

        crate::file::write(&path, "changed")?;
        assert!(!lifecycle_run_file_matches(&expected)?);
        fs::remove_file(&path)?;
        crate::file::write(tmp.path().join("foreign"), "expected")?;
        crate::file::make_symlink(&tmp.path().join("foreign"), &path)?;
        assert!(lifecycle_run_file_matches(&expected).is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_publish_journals_exact_deleted_file() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/deleted");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let mut writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        fs::remove_file(&writes.outputs[0].staging)?;

        let published = writes.publish()?;

        assert!(target.symlink_metadata().is_err());
        assert!(published.files.is_empty());
        assert_eq!(published.deleted, std::slice::from_ref(&target));
        drop(writes);
        assert!(fs::read_dir(target.parent().unwrap())?.all(|entry| {
            !entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".mise-lifecycle-")
        }));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn adjacent_run_file_has_immediate_identity_bound_cleanup() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let parent_path = crate::file::desymlink_path(&tmp.path().join("prefix/etc"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", parent_path.parent().unwrap());
        crate::file::create_dir_all(&parent_path)?;
        let parent = BoundRunSharedParent::open(&parent_path)?;
        let adjacent = BoundAdjacentRunFile::create(&parent)?;
        let path = parent_path.join(&adjacent.name);
        assert!(path.is_file());

        drop(adjacent);

        assert!(path.symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_publish_rejects_private_staging_ancestor_swap() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let outside = crate::file::desymlink_path(&tmp.path().join("outside"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        crate::file::create_dir_all(&prefix)?;
        crate::file::create_dir_all(&private)?;
        crate::file::create_dir_all(&outside)?;
        crate::file::write(outside.join("sentinel"), "preserve")?;
        let target = prefix.join("etc/openssl@3/generated");
        let mut writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        crate::file::write(&writes.outputs[0].staging, "generated")?;
        let staging_parent = writes.outputs[0].staging.parent().unwrap();
        let moved = staging_parent.with_extension("bound");
        fs::rename(staging_parent, &moved)?;
        crate::file::make_symlink(&outside, staging_parent)?;

        assert!(writes.publish().is_err());
        assert!(target.symlink_metadata().is_err());
        assert_eq!(
            crate::file::read_to_string(outside.join("sentinel"))?,
            "preserve"
        );
        assert!(outside.join("generated").symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_publish_preserves_concurrent_leaf_replacement() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let mut writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        crate::file::write(&writes.outputs[0].staging, "new")?;
        fs::rename(&target, target.with_extension("old"))?;
        crate::file::write(&target, "foreign")?;

        assert!(writes.publish().is_err());
        assert_eq!(crate::file::read_to_string(target)?, "foreign");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_create_rolls_back_after_every_post_link_failure() -> Result<()> {
        for checkpoint in [
            RunPublishCheckpoint::CreateLink,
            RunPublishCheckpoint::ParentSync,
            RunPublishCheckpoint::Validation,
        ] {
            let tmp = tempfile::tempdir()?;
            let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
            let private = crate::file::desymlink_path(&tmp.path().join("private"));
            let mut env = crate::test::EnvVarGuard::new();
            env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
            crate::file::create_dir_all(&prefix)?;
            crate::file::create_dir_all(&private)?;
            let target = prefix.join("etc/openssl@3/generated");
            let mut writes = prepare_run_shared_writes(
                &prefix.join("Cellar/openssl@3/1"),
                &BTreeSet::from([target.clone()]),
                None,
                &private,
            )?;
            crate::file::write(&writes.outputs[0].staging, "new")?;
            RUN_PUBLISH_FAULT.set(Some(checkpoint));

            assert!(writes.publish().is_err(), "checkpoint {checkpoint:?}");
            assert!(
                target.symlink_metadata().is_err(),
                "checkpoint {checkpoint:?}"
            );
            assert_eq!(RUN_PUBLISH_FAULT.get(), None);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_replace_rolls_back_after_every_post_exchange_failure() -> Result<()> {
        for checkpoint in [
            RunPublishCheckpoint::ReplaceExchange,
            RunPublishCheckpoint::Validation,
            RunPublishCheckpoint::ParentSync,
        ] {
            let tmp = tempfile::tempdir()?;
            let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
            let private = crate::file::desymlink_path(&tmp.path().join("private"));
            let mut env = crate::test::EnvVarGuard::new();
            env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
            crate::file::create_dir_all(&private)?;
            let target = prefix.join("etc/openssl@3/generated");
            crate::file::create_dir_all(target.parent().unwrap())?;
            crate::file::write(&target, "old")?;
            let mut writes = prepare_run_shared_writes(
                &prefix.join("Cellar/openssl@3/1"),
                &BTreeSet::from([target.clone()]),
                None,
                &private,
            )?;
            crate::file::write(&writes.outputs[0].staging, "new")?;
            RUN_PUBLISH_FAULT.set(Some(checkpoint));

            assert!(writes.publish().is_err(), "checkpoint {checkpoint:?}");
            assert_eq!(
                crate::file::read_to_string(&target)?,
                "old",
                "checkpoint {checkpoint:?}"
            );
            assert_eq!(RUN_PUBLISH_FAULT.get(), None);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_delete_rolls_back_after_every_namespace_failure() -> Result<()> {
        for checkpoint in [
            RunPublishCheckpoint::DeleteExchange,
            RunPublishCheckpoint::Validation,
            RunPublishCheckpoint::DeleteUnlink,
            RunPublishCheckpoint::ParentSync,
        ] {
            let tmp = tempfile::tempdir()?;
            let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
            let private = crate::file::desymlink_path(&tmp.path().join("private"));
            let mut env = crate::test::EnvVarGuard::new();
            env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
            crate::file::create_dir_all(&private)?;
            let target = prefix.join("etc/openssl@3/generated");
            crate::file::create_dir_all(target.parent().unwrap())?;
            crate::file::write(&target, "old")?;
            let mut writes = prepare_run_shared_writes(
                &prefix.join("Cellar/openssl@3/1"),
                &BTreeSet::from([target.clone()]),
                None,
                &private,
            )?;
            crate::file::remove_file(&writes.outputs[0].staging)?;
            RUN_PUBLISH_FAULT.set(Some(checkpoint));

            assert!(writes.publish().is_err(), "checkpoint {checkpoint:?}");
            assert_eq!(
                crate::file::read_to_string(&target)?,
                "old",
                "checkpoint {checkpoint:?}"
            );
            assert_eq!(RUN_PUBLISH_FAULT.get(), None);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_rollback_never_exchanges_a_foreign_backup_node() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        crate::file::write(&writes.outputs[0].staging, "new")?;
        let mut publish = PreparedRunPublish::prepare(0, &writes.outputs[0])?;
        publish.prevalidate(&writes.outputs[0])?;
        publish.apply(&writes.outputs[0])?;
        let adjacent_name = match &publish.action {
            RunPublishAction::Replace { adjacent, .. } => adjacent.name.clone(),
            _ => unreachable!(),
        };
        let adjacent = target.parent().unwrap().join(adjacent_name);
        let saved_old = target.parent().unwrap().join("saved-old");
        fs::rename(&adjacent, &saved_old)?;
        crate::file::write(&adjacent, "foreign")?;

        assert!(publish.rollback(&writes.outputs[0]).is_err());
        assert_eq!(crate::file::read_to_string(&target)?, "new");
        assert_eq!(crate::file::read_to_string(&adjacent)?, "foreign");
        assert_eq!(crate::file::read_to_string(&saved_old)?, "old");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn failed_rollback_retains_predecessor_in_private_recovery() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        crate::file::write(&writes.outputs[0].staging, "new")?;
        let mut publishes = vec![PreparedRunPublish::prepare(0, &writes.outputs[0])?];
        publishes[0].prevalidate(&writes.outputs[0])?;
        publishes[0].apply(&writes.outputs[0])?;
        let moved_new = target.with_extension("transaction-new");
        fs::rename(&target, &moved_new)?;
        crate::file::write(&target, "foreign")?;

        let error = rollback_run_publishes(&mut publishes, &writes.outputs).unwrap_err();
        let recovery_dir = writes.outputs[0].rollback_parent.path().to_path_buf();
        let recovery = fs::read_dir(&recovery_dir)?
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .map(|entry| entry.path())
            .find(|path| path.is_file())
            .ok_or_else(|| eyre!("predecessor recovery file is missing"))?;
        assert!(format!("{error:#}").contains(&recovery.display().to_string()));
        assert_eq!(crate::file::read_to_string(&recovery)?, "old");
        drop(publishes);
        drop(writes);

        assert_eq!(crate::file::read_to_string(&target)?, "foreign");
        assert_eq!(crate::file::read_to_string(&recovery)?, "old");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rollback_fsync_failure_retains_restored_predecessor_recovery() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        crate::file::write(&writes.outputs[0].staging, "new")?;
        let mut publishes = vec![PreparedRunPublish::prepare(0, &writes.outputs[0])?];
        publishes[0].apply(&writes.outputs[0])?;
        RUN_PUBLISH_FAULT.set(Some(RunPublishCheckpoint::RollbackParentSync));

        let error = rollback_run_publishes(&mut publishes, &writes.outputs).unwrap_err();
        let recovery_dir = writes.outputs[0].rollback_parent.path().to_path_buf();
        let recovery = fs::read_dir(&recovery_dir)?
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .map(|entry| entry.path())
            .find(|path| path.is_file())
            .ok_or_else(|| eyre!("predecessor recovery file is missing"))?;

        assert!(format!("{error:#}").contains(&recovery.display().to_string()));
        assert_eq!(crate::file::read_to_string(&target)?, "old");
        assert_eq!(crate::file::read_to_string(&recovery)?, "old");
        assert_eq!(RUN_PUBLISH_FAULT.get(), None);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn failed_private_recovery_link_retains_adjacent_predecessor() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&target, "old")?;
        let writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target.clone()]),
            None,
            &private,
        )?;
        crate::file::write(&writes.outputs[0].staging, "new")?;
        let mut publishes = vec![PreparedRunPublish::prepare(0, &writes.outputs[0])?];
        publishes[0].apply(&writes.outputs[0])?;
        let adjacent = match &publishes[0].action {
            RunPublishAction::Replace { adjacent, .. } => {
                target.parent().unwrap().join(&adjacent.name)
            }
            _ => unreachable!(),
        };
        fs::set_permissions(
            writes.outputs[0].rollback_parent.path(),
            fs::Permissions::from_mode(0o500),
        )?;
        fs::rename(&target, target.with_extension("transaction-new"))?;
        crate::file::write(&target, "foreign")?;

        let error = rollback_run_publishes(&mut publishes, &writes.outputs).unwrap_err();

        assert!(format!("{error:#}").contains(&adjacent.display().to_string()));
        drop(publishes);
        drop(writes);
        assert_eq!(crate::file::read_to_string(&target)?, "foreign");
        assert_eq!(crate::file::read_to_string(&adjacent)?, "old");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_multi_output_cleanup_failure_rolls_back_every_output() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let first = prefix.join("etc/openssl@3/first");
        let second = prefix.join("etc/openssl@3/second");
        crate::file::create_dir_all(first.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&first, "old-first")?;
        crate::file::write(&second, "old-second")?;
        let writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([first.clone(), second.clone()]),
            None,
            &private,
        )?;
        for output in &writes.outputs {
            crate::file::write(
                &output.staging,
                format!(
                    "new-{}",
                    output.destination.file_name().unwrap().to_string_lossy()
                ),
            )?;
        }
        let mut publishes = writes
            .outputs
            .iter()
            .enumerate()
            .map(|(index, output)| PreparedRunPublish::prepare(index, output))
            .collect::<Result<Vec<_>>>()?;
        for publish in &publishes {
            publish.prevalidate(&writes.outputs[publish.output])?;
        }
        for publish in &mut publishes {
            publish.apply(&writes.outputs[publish.output])?;
            publish.validate_applied(&writes.outputs[publish.output])?;
            publish.arm_cleanup_rollback(&writes.outputs[publish.output])?;
        }
        publishes[0].cleanup()?;
        let adjacent_name = match &publishes[1].action {
            RunPublishAction::Replace { adjacent, .. } => adjacent.name.clone(),
            _ => unreachable!(),
        };
        let adjacent = second.parent().unwrap().join(adjacent_name);
        let saved_old = second.parent().unwrap().join("saved-second-old");
        fs::rename(&adjacent, &saved_old)?;
        crate::file::write(&adjacent, "foreign")?;

        assert!(publishes[1].cleanup().is_err());
        rollback_run_publishes(&mut publishes, &writes.outputs)?;

        assert_eq!(crate::file::read_to_string(&first)?, "old-first");
        assert_eq!(crate::file::read_to_string(&second)?, "old-second");
        assert_eq!(crate::file::read_to_string(&adjacent)?, "foreign");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_rollback_continues_after_one_foreign_backup() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        crate::file::create_dir_all(&private)?;
        let targets =
            ["first", "middle", "last"].map(|name| prefix.join("etc/openssl@3").join(name));
        for target in &targets {
            crate::file::create_dir_all(target.parent().unwrap())?;
            crate::file::write(
                target,
                format!("old-{}", target.file_name().unwrap().to_string_lossy()),
            )?;
        }
        let writes = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &targets.iter().cloned().collect(),
            None,
            &private,
        )?;
        for output in &writes.outputs {
            crate::file::write(
                &output.staging,
                format!(
                    "new-{}",
                    output.destination.file_name().unwrap().to_string_lossy()
                ),
            )?;
        }
        let mut publishes = writes
            .outputs
            .iter()
            .enumerate()
            .map(|(index, output)| PreparedRunPublish::prepare(index, output))
            .collect::<Result<Vec<_>>>()?;
        for publish in &publishes {
            publish.prevalidate(&writes.outputs[publish.output])?;
        }
        for publish in &mut publishes {
            publish.apply(&writes.outputs[publish.output])?;
        }
        let middle_index = writes
            .outputs
            .iter()
            .position(|output| output.destination.ends_with("middle"))
            .unwrap();
        let adjacent_name = match &publishes[middle_index].action {
            RunPublishAction::Replace { adjacent, .. } => adjacent.name.clone(),
            _ => unreachable!(),
        };
        let adjacent = targets[1].parent().unwrap().join(adjacent_name);
        let saved_old = targets[1].parent().unwrap().join("saved-middle-old");
        fs::rename(&adjacent, &saved_old)?;
        crate::file::write(&adjacent, "foreign")?;

        let error = rollback_run_publishes(&mut publishes, &writes.outputs).unwrap_err();

        assert!(format!("{error:#}").contains("failed to roll back one or more"));
        assert_eq!(crate::file::read_to_string(&targets[0])?, "old-first");
        assert_eq!(crate::file::read_to_string(&targets[1])?, "new-middle");
        assert_eq!(crate::file::read_to_string(&targets[2])?, "old-last");
        assert_eq!(crate::file::read_to_string(&adjacent)?, "foreign");
        assert_eq!(crate::file::read_to_string(&saved_old)?, "old-middle");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_shared_output_rejects_non_regular_leaf() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let target = prefix.join("etc/openssl@3/generated");
        crate::file::create_dir_all(&target)?;
        crate::file::create_dir_all(&private)?;

        let error = prepare_run_shared_writes(
            &prefix.join("Cellar/openssl@3/1"),
            &BTreeSet::from([target]),
            None,
            &private,
        )
        .unwrap_err();

        assert!(error.to_string().contains("not an exact regular file"));
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
        let mut run_files = vec![];
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
            &mut run_files,
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
            &mut run_files,
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
    fn run_preflight_rejects_non_keg_cwd_and_stdin() {
        let keg = prefix::cellar().join("openssl@3/1");
        let cases = [
            (
                serde_json::json!({
                    "type": "run",
                    "command": {"base": "bin", "path": "tool"},
                    "chdir": {"path": "/etc"}
                }),
                "working directory must remain inside its keg",
            ),
            (
                serde_json::json!({
                    "type": "run",
                    "command": {"base": "bin", "path": "tool"},
                    "stdin_path": {
                        "path": prefix::prefix().join("etc/private-secret")
                    }
                }),
                "stdin must remain inside its keg",
            ),
        ];
        for (step, expected) in cases {
            let error = prepare(&formula(vec![step]), &keg).unwrap_err();
            assert!(format!("{error:#}").contains(expected));
        }
    }

    #[cfg(unix)]
    #[test]
    fn run_stdin_rejects_symlinked_leaf_and_ancestor() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = crate::file::desymlink_path(&tmp.path().join("Cellar/foo/1"));
        let outside = crate::file::desymlink_path(&tmp.path().join("outside"));
        crate::file::create_dir_all(&keg)?;
        crate::file::create_dir_all(&outside)?;
        crate::file::write(outside.join("secret"), "preserve")?;
        crate::file::make_symlink(&outside.join("secret"), &keg.join("leaf"))?;
        crate::file::make_symlink(&outside, &keg.join("parent"))?;

        assert!(open_run_stdin(&keg, &keg.join("leaf")).is_err());
        assert!(open_run_stdin(&keg, &keg.join("parent/secret")).is_err());
        assert!(open_run_stdin(&keg, &keg.join("missing/input")).is_err());
        assert!(keg.join("missing").symlink_metadata().is_err());
        assert_eq!(
            crate::file::read_to_string(outside.join("secret"))?,
            "preserve"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn audited_source_copy_is_private_and_original_mutation_is_detected() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = crate::file::desymlink_path(&tmp.path().join("Cellar/ca-certificates/1"));
        let source = keg.join("share/ca-certificates/cacert.pem");
        let private = crate::file::desymlink_path(&tmp.path().join("private"));
        crate::file::create_dir_all(source.parent().unwrap())?;
        crate::file::create_dir_all(&private)?;
        crate::file::write(&source, "original")?;
        let (parent, name, identity) = open_run_stdin(&keg, &source)?;
        let temp = BoundRunPrivateTree::create(&private, "run-")?;
        let copy_name = std::ffi::OsString::from("audited-source-input");
        let mut copy = create_bound_run_file(&temp.parent, &copy_name)?;
        copy_open_file_to(&identity.file, &mut copy)?;
        copy.sync_all()?;
        crate::file::write(&source, "mutated")?;

        assert!(validate_bound_run_file_identity(&parent, &name, &identity).is_err());
        assert_eq!(
            crate::file::read_to_string(temp.path().join(copy_name))?,
            "original"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn run_cwd_binding_rejects_symlinked_keg_subdirectory() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = crate::file::desymlink_path(&tmp.path().join("Cellar/foo/1"));
        let outside = crate::file::desymlink_path(&tmp.path().join("outside"));
        crate::file::create_dir_all(&keg)?;
        crate::file::create_dir_all(&outside)?;
        crate::file::write(outside.join("sentinel"), "preserve")?;
        crate::file::make_symlink(&outside, &keg.join("cwd"))?;

        assert!(BoundRunSharedParent::open_existing(&keg.join("cwd")).is_err());
        assert!(BoundRunSharedParent::open_existing(&keg.join("missing/cwd")).is_err());
        assert!(keg.join("missing").symlink_metadata().is_err());
        assert_eq!(
            crate::file::read_to_string(outside.join("sentinel"))?,
            "preserve"
        );
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn statically_disabled_run_does_not_require_linux_confinement() -> Result<()> {
        prepare(
            &formula(vec![serde_json::json!({
                "type": "run",
                "command": {"path": "/bin/true"},
                "guards": [{"condition": "on", "value": "macos"}]
            })]),
            &prefix::cellar().join("openssl@3/1"),
        )?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn strict_run_preflight_unavailability_is_fail_closed() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut env = crate::test::EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        let keg = prefix.join("Cellar/openssl@3/1");
        let sentinel = prefix.join("etc/openssl@3/sentinel");
        crate::file::create_dir_all(sentinel.parent().unwrap())?;
        crate::file::write(&sentinel, "preserve")?;

        let result = prepare(
            &formula(vec![serde_json::json!({
                "type": "run",
                "command": {"path": "/bin/true"}
            })]),
            &keg,
        );

        assert_eq!(crate::file::read_to_string(&sentinel)?, "preserve");
        assert!(keg.symlink_metadata().is_err());
        assert!(state_path(&keg).symlink_metadata().is_err());
        match result {
            Ok(_) => Ok(()),
            Err(error) if strict_formula_execution_unavailable(&error) => {
                assert!(format!("{error:#}").contains("no package state was changed"));
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn run_environment_contains_only_fixed_and_typed_keys() -> Result<()> {
        let formula = formula(vec![serde_json::json!({
            "type": "run",
            "command": {"base": "bin", "path": "tool"},
            "env": {"FORMULA_KEY": "formula-value"},
            "guards": [{"condition": "on", "value": "macos"}]
        })]);
        let prepared = prepare(&formula, &prefix::cellar().join("openssl@3/1"))?;
        let PreparedStep::Run(run) = &prepared.steps[0] else {
            panic!("expected prepared run")
        };
        let env = run_environment(
            &prepared,
            run,
            Path::new("/private/tmp/mise-private"),
            false,
        )?;
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
            run_files: vec![],
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
            run_files: vec![],
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
