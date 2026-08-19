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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
enum PreparedSources {
    One(PathBuf),
    Glob(PreparedPattern),
}

#[derive(Debug)]
struct PreparedPattern {
    patterns: Vec<String>,
}

#[derive(Debug)]
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
    Symlink {
        source: PathBuf,
        target: PathBuf,
    },
    SetPermissions {
        path: PathBuf,
        permission: LifecyclePermissionKind,
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

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LifecyclePermission {
    path: PathBuf,
    permission: LifecyclePermissionKind,
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

fn read_receipt_identity(keg: &Path) -> Result<LifecycleReceiptIdentity> {
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
    serde_json::from_slice(&std::fs::read(&path)?)
        .wrap_err_with(|| format!("lifecycle install receipt is malformed: {}", path.display()))
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
    crate::file::write(identity_marker_path(&prepared.keg), &incarnation)?;
    Ok(LifecycleInstallIdentity {
        formula,
        receipt,
        formula_snapshot_sha256,
        incarnation,
    })
}

fn validate_install_identity(keg: &Path, state: &LifecycleState) -> Result<()> {
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
    let path = state_path(keg);
    if path.symlink_metadata().is_err() {
        return LifecycleInstallProgress::Absent;
    }
    match crate::file::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<LifecycleState>(&contents).ok())
    {
        Some(state)
            if validate_install_identity(keg, &state).is_ok()
                && validate_shared_state(keg, &state).is_ok()
                && state.complete
                && state.phase == LifecyclePhase::Complete =>
        {
            LifecycleInstallProgress::Complete
        }
        Some(_) | None => LifecycleInstallProgress::Incomplete,
    }
}

/// Classify formula lifecycle health without fetching metadata or mutating state.
/// A missing private state file is accepted for native Homebrew state only when
/// the observable shared defaults are present. Mise-owned legacy state remains
/// actionable because old mise versions never ran this lifecycle at all.
pub(super) fn health(keg: &Path, mise_owned: bool) -> LifecycleHealth {
    let shared_missing = shared_missing_paths(keg);
    let path = state_path(keg);
    if path.symlink_metadata().is_err() {
        if mise_owned {
            let mut reasons = vec![
                "lifecycle state absent; install_etc_var and post-install were not recorded"
                    .to_string(),
            ];
            reasons.extend(
                shared_missing.into_iter().map(|(_, target)| {
                    format!("shared lifecycle path missing: {}", target.display())
                }),
            );
            return LifecycleHealth::Repairable(reasons);
        }
        return native_health(shared_missing);
    }
    let state = match crate::file::read_to_string(&path)
        .ok()
        .and_then(|contents| serde_json::from_str::<LifecycleState>(&contents).ok())
    {
        Some(state) => state,
        None if !mise_owned => return native_health(shared_missing),
        None => {
            return LifecycleHealth::ReinstallRequired(vec![format!(
                "lifecycle state is unreadable: {}",
                path.display()
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
        if !node_exists(&mapping.target) {
            repairable.insert(format!(
                "shared lifecycle path missing: {}",
                mapping.target.display()
            ));
        }
    }
    for link in &state.symlinks {
        if !node_exists(&link.source) {
            reinstall.insert(format!(
                "post-install symlink source is missing: {}",
                link.source.display()
            ));
        } else if resolved_symlink_target(&link.target).as_ref() != Some(&link.source) {
            if link.target.symlink_metadata().is_err() {
                repairable.insert(format!(
                    "post-install symlink is missing: {}",
                    link.target.display()
                ));
            } else {
                reinstall.insert(format!(
                    "post-install target has ambiguous ownership: {}",
                    link.target.display()
                ));
            }
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
        if !node_exists(required) {
            let is_shared_default = ["etc", "var"].into_iter().any(|root| {
                required
                    .strip_prefix(prefix::prefix().join(root))
                    .ok()
                    .is_some_and(|relative| {
                        keg.join(".bottle")
                            .join(root)
                            .join(relative)
                            .symlink_metadata()
                            .is_ok()
                    })
            });
            if is_shared_default {
                repairable.insert(format!(
                    "shared lifecycle path missing: {}",
                    required.display()
                ));
            } else {
                reinstall.insert(format!(
                    "post-install output is missing and cannot be replayed safely: {}",
                    required.display()
                ));
            }
        }
    }
    for permission in &state.permissions {
        if permission_satisfied(&permission.path, permission.permission) {
            continue;
        }
        if node_exists(&permission.path) {
            repairable.insert(format!(
                "post-install permission is missing at {}",
                permission.path.display()
            ));
        } else {
            reinstall.insert(format!(
                "post-install permission target is missing: {}",
                permission.path.display()
            ));
        }
    }
    for pattern in &state.absent_patterns {
        if glob::glob(pattern)
            .ok()
            .into_iter()
            .flatten()
            .any(|path| path.is_ok())
        {
            reinstall.insert(format!(
                "post-install removal invariant no longer holds: {pattern}"
            ));
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

fn node_exists(path: &Path) -> bool {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            resolved_symlink_target(path).is_some_and(|target| target.exists())
        }
        Ok(_) => true,
        Err(_) => false,
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

fn shared_sources(keg: &Path) -> Vec<PathBuf> {
    let mut sources = vec![];
    for root in ["etc", "var"] {
        let source_root = keg.join(".bottle").join(root);
        if !source_root.is_dir() {
            continue;
        }
        sources.extend(
            walkdir::WalkDir::new(source_root)
                .follow_links(false)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| !entry.file_type().is_dir())
                .map(|entry| entry.into_path()),
        );
    }
    sources.sort();
    sources
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
                mapping.target == target || mapping.target == default
            })
    })
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
        if !recorded_sources.insert(mapping.source.clone())
            || !recorded_targets.insert(mapping.target.clone())
        {
            bail!("duplicate shared-state source or target mapping")
        }
    }
    let current_sources = shared_sources(keg).into_iter().collect::<BTreeSet<_>>();
    if recorded_sources != current_sources {
        bail!("recorded shared-state sources do not match current keg")
    }
    Ok(())
}

fn shared_missing_paths(keg: &Path) -> Vec<(PathBuf, PathBuf)> {
    shared_sources(keg)
        .into_iter()
        .filter_map(|source| {
            ["etc", "var"].into_iter().find_map(|root| {
                let relative = source.strip_prefix(keg.join(".bottle").join(root)).ok()?;
                let target = prefix::prefix().join(root).join(relative);
                (!node_exists(&target)).then_some((source.clone(), target))
            })
        })
        .collect()
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
    if path.symlink_metadata().is_err() {
        validate_legacy_formula_snapshot(prepared)?;
        install(prepared, None).await?;
        return Ok(true);
    }
    let mut state: LifecycleState = serde_json::from_str(&crate::file::read_to_string(&path)?)?;
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
    let mut journal = state
        .repair
        .take()
        .unwrap_or_else(|| LifecycleRepairJournal {
            effects: repair_effects(&state),
            next: 0,
        });
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
    if path.symlink_metadata().is_err() {
        validate_legacy_formula_snapshot(prepared)?;
        return Ok(true);
    }
    let state: LifecycleState = serde_json::from_str(&crate::file::read_to_string(&path)?)?;
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
    let journal = state
        .repair
        .clone()
        .unwrap_or_else(|| LifecycleRepairJournal {
            effects: repair_effects(&state),
            next: 0,
        });
    validate_repair_journal(keg, &state, &journal)?;
    preflight_repair_effects(&journal.effects)?;
    Ok(true)
}

pub(super) fn requires_legacy_snapshot_evidence(prepared: &PreparedFormulaLifecycle) -> bool {
    state_path(&prepared.keg).symlink_metadata().is_err()
}

fn validate_legacy_formula_snapshot(prepared: &PreparedFormulaLifecycle) -> Result<()> {
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

fn repair_effects(state: &LifecycleState) -> Vec<LifecycleRepairEffect> {
    let mut effects = state
        .shared_state
        .iter()
        .filter(|mapping| !node_exists(&mapping.target))
        .map(|mapping| LifecycleRepairEffect::Copy {
            source: mapping.source.clone(),
            target: mapping.target.clone(),
        })
        .collect::<Vec<_>>();
    effects.extend(
        state
            .symlinks
            .iter()
            .filter(|link| resolved_symlink_target(&link.target).as_ref() != Some(&link.source))
            .map(|link| LifecycleRepairEffect::Symlink {
                source: link.source.clone(),
                target: link.target.clone(),
            }),
    );
    effects.extend(
        state
            .permissions
            .iter()
            .filter(|permission| !permission_satisfied(&permission.path, permission.permission))
            .map(|permission| LifecycleRepairEffect::SetPermissions {
                path: permission.path.clone(),
                permission: permission.permission,
            }),
    );
    effects
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
                let mapping_matches = state
                    .shared_state
                    .iter()
                    .any(|mapping| mapping.source == *source && mapping.target == *target);
                if !mapping_matches
                    || !validate_shared_mapping(
                        keg,
                        &LifecycleSharedState {
                            source: source.clone(),
                            target: target.clone(),
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
            LifecycleRepairEffect::SetPermissions { path, permission } => {
                if !state
                    .permissions
                    .iter()
                    .any(|expected| expected.path == *path && expected.permission == *permission)
                {
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
                if source.symlink_metadata().is_err() {
                    bail!("lifecycle repair source is missing: {}", source.display())
                }
                ensure_runtime_write_path(target, false)?;
                if target.symlink_metadata().is_ok() && !files_equal(source, target) {
                    bail!(
                        "lifecycle repair target has ambiguous ownership: {}",
                        target.display()
                    )
                }
            }
            LifecycleRepairEffect::Symlink { source, target } => {
                if !node_exists(source) {
                    bail!(
                        "lifecycle repair symlink source is missing: {}",
                        source.display()
                    )
                }
                ensure_runtime_write_path(target, false)?;
                if target.symlink_metadata().is_ok()
                    && resolved_symlink_target(target).as_ref() != Some(source)
                {
                    bail!(
                        "lifecycle repair target has ambiguous ownership: {}",
                        target.display()
                    )
                }
            }
            LifecycleRepairEffect::SetPermissions { path, .. } => {
                permission_target(path).wrap_err_with(|| {
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
            if target.symlink_metadata().is_err() {
                atomic_copy(source, target)?;
            }
        }
        LifecycleRepairEffect::Symlink { source, target } => {
            if target.symlink_metadata().is_err() {
                crate::file::create_dir_all(target.parent().unwrap())?;
                crate::file::make_symlink(source, target)?;
            }
        }
        LifecycleRepairEffect::SetPermissions { path, permission } => {
            apply_permission(path, *permission)?;
        }
    }
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

#[cfg(test)]
pub(super) fn test_state_path(keg: &Path) -> PathBuf {
    state_path(keg)
}

pub(super) fn remove_owned_state(keg: &Path) -> Result<()> {
    let path = state_path(keg);
    if path.exists() {
        let state: LifecycleState = serde_json::from_str(&crate::file::read_to_string(&path)?)?;
        if validate_install_identity(keg, &state).is_ok() {
            remove_lifecycle_symlinks(&state)?;
            let marker = identity_marker_path(keg);
            if marker.symlink_metadata().is_ok() {
                crate::file::remove_file(marker)?;
            }
        }
        crate::file::remove_file(path)?;
    }
    Ok(())
}

fn remove_lifecycle_symlinks(state: &LifecycleState) -> Result<()> {
    for link in &state.symlinks {
        if resolved_symlink_target(&link.target).as_ref() == Some(&link.source) {
            crate::file::remove_file(&link.target)?;
        }
    }
    Ok(())
}

fn write_state(path: &Path, state: &LifecycleState) -> Result<()> {
    crate::file::create_dir_all(path.parent().unwrap())?;
    crate::file::write(path, serde_json::to_string_pretty(state)?)
}

fn install_shared_tree(
    formula: &str,
    root: &str,
    source_root: &Path,
    destination_root: &Path,
    predecessor_keg: Option<&Path>,
) -> Result<Vec<LifecycleSharedState>> {
    if !source_root.is_dir() {
        return Ok(vec![]);
    }
    let mut installed_paths = vec![];
    for entry in walkdir::WalkDir::new(source_root).follow_links(false) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source_root)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = destination_root.join(relative);
        if entry.file_type().is_dir() {
            crate::file::create_dir_all(&destination)?;
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
        atomic_copy(entry.path(), &destination)?;
        installed_paths.push(LifecycleSharedState {
            source: entry.path().to_path_buf(),
            target: destination,
        });
    }
    Ok(installed_paths)
}

fn install_destination(
    formula: &str,
    root: &str,
    source: &Path,
    relative: &Path,
    destination: &Path,
    predecessor_keg: Option<&Path>,
) -> Result<PathBuf> {
    if destination.symlink_metadata().is_err() || files_equal(source, destination) {
        return Ok(destination.to_path_buf());
    }
    if let Some(predecessor_keg) = predecessor_keg {
        let old_default = predecessor_keg.join(".bottle").join(root).join(relative);
        if old_default.symlink_metadata().is_ok() && files_equal(&old_default, destination) {
            return Ok(destination.to_path_buf());
        }
    }
    let default = shared_default_path(destination);
    debug!(
        "brew:{} preserving modified {}; writing new default to {}",
        formula,
        destination.display(),
        default.display()
    );
    Ok(default)
}

fn shared_default_path(destination: &Path) -> PathBuf {
    let mut default = destination.as_os_str().to_os_string();
    default.push(".default");
    default.into()
}

pub(super) fn atomic_copy(source: &Path, destination: &Path) -> Result<()> {
    crate::file::create_dir_all(destination.parent().unwrap())?;
    let temp = destination.with_file_name(format!(
        ".{}.mise-new",
        destination.file_name().unwrap().to_string_lossy()
    ));
    if temp.symlink_metadata().is_ok() {
        crate::file::remove_file(&temp)?;
    }
    let metadata = source.symlink_metadata()?;
    if metadata.file_type().is_symlink() {
        crate::file::make_symlink(&fs::read_link(source)?, &temp)?;
    } else {
        fs::copy(source, &temp)?;
        fs::set_permissions(&temp, metadata.permissions())?;
    }
    if destination.symlink_metadata().is_ok() {
        crate::file::remove_file(destination)?;
    }
    crate::file::rename(&temp, destination)?;
    Ok(())
}

fn files_equal(left: &Path, right: &Path) -> bool {
    match (left.symlink_metadata(), right.symlink_metadata()) {
        (Ok(a), Ok(b)) if a.file_type().is_symlink() && b.file_type().is_symlink() => {
            fs::read_link(left).ok() == fs::read_link(right).ok()
        }
        (Ok(a), Ok(b)) if a.is_file() && b.is_file() && a.len() == b.len() => {
            fs::read(left).ok() == fs::read(right).ok()
        }
        _ => false,
    }
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
        if !permissions.contains(&permission) {
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
            ensure_runtime_write_path(target, target.is_dir())?;
            let directory_target = sources.len() > 1 || target.is_dir();
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
                if *recursive {
                    required_paths.extend(copy_recursive(&source, &destination)?);
                } else {
                    atomic_copy(&source, &destination)?;
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
                crate::file::create_dir_all(destination.parent().unwrap())?;
                if destination.symlink_metadata().is_ok() {
                    if !force && resolved_symlink_target(&destination).as_ref() != Some(&source) {
                        bail!(
                            "post-install target already exists: {}",
                            destination.display()
                        );
                    }
                    crate::file::remove_file(&destination)?;
                }
                let source = super::pour::lexical_normalize(&source);
                // Formula DSL `ln_s`/`ln_sf` receives the resolved source
                // path. These lifecycle links are absolute; only keg/public
                // topology uses Homebrew's relative-link convention.
                crate::file::make_symlink(&source, &destination)?;
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
fn permission_satisfied(path: &Path, permission: LifecyclePermissionKind) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata().is_ok_and(|metadata| match permission {
        LifecyclePermissionKind::UserWrite => metadata.permissions().mode() & 0o200 != 0,
    })
}

#[cfg(not(unix))]
fn permission_satisfied(_path: &Path, _permission: LifecyclePermissionKind) -> bool {
    false
}

fn remove_prepared_node(
    path: &Path,
    recursive: bool,
    symlink_target_contains: Option<&str>,
) -> Result<()> {
    let Ok(metadata) = path.symlink_metadata() else {
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
            PreparedGuard::IfExists(pattern) => expand_pattern(pattern)?
                .iter()
                .any(|path| path.symlink_metadata().is_ok()),
            PreparedGuard::UnlessExists(pattern) => expand_pattern(pattern)?
                .iter()
                .all(|path| path.symlink_metadata().is_err()),
            PreparedGuard::Platform(matches) => *matches,
        };
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
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

    let stdout = match &run.stdout_path {
        Some(path) => {
            ensure_runtime_write_path(path, false)?;
            crate::file::create_dir_all(path.parent().unwrap())?;
            open_truncated(path)?
        }
        None => open_truncated(&stdout_log)?,
    };
    let stderr = open_truncated(&stderr_log)?;
    let stdin = match &run.stdin_path {
        Some(path) => Stdio::from(File::open(path)?),
        None => Stdio::null(),
    };

    let shared = prefix::prefix();
    let env = run_environment(prepared, run, &temp)?;
    let mut allow_write = vec![
        prepared.keg.clone(),
        shared.join("etc"),
        shared.join("var"),
        run.log_dir.clone(),
        temp.clone(),
    ];
    if let Some(path) = &run.stdout_path {
        allow_write.push(path.clone());
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
        let stdout = log_tail(run.stdout_path.as_deref().unwrap_or(&stdout_log))?;
        let stderr = log_tail(&stderr_log)?;
        return Err(error).wrap_err_with(|| {
            format!(
                "brew:{} post-install run step {} failed\nstdout tail:\n{}\nstderr tail:\n{}",
                prepared.formula, run.step_index, stdout, stderr
            )
        });
    }

    let mut required_paths = run
        .args
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.starts_with(&shared) && path.symlink_metadata().is_ok())
        .collect::<Vec<_>>();
    if let Some(path) = &run.stdout_path {
        required_paths.push(path.clone());
    }
    Ok(StepEffects {
        required_paths,
        ..Default::default()
    })
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

fn copy_recursive(source: &Path, target: &Path) -> Result<Vec<PathBuf>> {
    let destination = target.to_path_buf();
    if let Ok(metadata) = destination.symlink_metadata() {
        if metadata.file_type().is_symlink() {
            crate::file::remove_file(&destination)?;
        } else {
            crate::file::remove_all(&destination)?;
        }
    }
    let mut outputs = vec![destination.clone()];
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let output = destination.join(relative);
        if entry.file_type().is_dir() {
            crate::file::create_dir_all(&output)?;
        } else {
            atomic_copy(entry.path(), &output)?;
            outputs.push(output);
        }
    }
    Ok(outputs)
}

fn resolved_symlink_target(path: &Path) -> Option<PathBuf> {
    let target = fs::read_link(path).ok()?;
    let target = if target.is_absolute() {
        target
    } else {
        path.parent()?.join(target)
    };
    Some(super::pour::lexical_normalize(&target))
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
        ));
        apply_permission_unchecked(&path, LifecyclePermissionKind::UserWrite)?;
        apply_permission_unchecked(&path, LifecyclePermissionKind::UserWrite)?;
        assert!(permission_satisfied(
            &path,
            LifecyclePermissionKind::UserWrite
        ));
        assert_eq!(path.metadata()?.permissions().mode() & 0o777, 0o644);
        Ok(())
    }

    #[test]
    fn legacy_bottle_snapshot_can_differ_from_current_tap_source() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = tmp.path().join("Cellar/openssl@3/1");
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
        let prefix = tmp.path().join("prefix");
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
            }],
            symlinks: vec![LifecycleSymlink {
                source: link_source.clone(),
                target: link_target.clone(),
            }],
            required_paths: vec![shared_target],
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

        // Real Homebrew replaced this exact rack/version from another receipt
        // while the external mise state and incarnation marker survived.
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
        remove_owned_state(&keg)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn repair_restores_exact_default_mapping_without_repouring() -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
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
        assert_eq!(
            installed,
            vec![LifecycleSharedState {
                source,
                target: default,
            }]
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
        assert_eq!(
            installed,
            vec![LifecycleSharedState {
                source,
                target: destination.clone(),
            }]
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
        assert_eq!(
            installed,
            vec![LifecycleSharedState {
                source,
                target: default,
            }]
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
        assert_eq!(
            installed,
            vec![LifecycleSharedState {
                source,
                target: PathBuf::from(format!("{}.default", destination.display())),
            }]
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
        let source = tmp.path().join("source");
        let replacement = tmp.path().join("replacement");
        let target = tmp.path().join("target");
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
}
