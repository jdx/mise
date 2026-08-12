//! Persistent formula state and typed post-install operations.

use std::collections::BTreeMap;
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
    steps: Vec<PreparedStep>,
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
    symlinks: Vec<LifecycleSymlink>,
    #[serde(default)]
    required_paths: Vec<PathBuf>,
    #[serde(default)]
    absent_patterns: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
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

pub(super) fn needs_repair(keg: &Path) -> bool {
    let state_path = state_path(keg);
    if state_path.exists() {
        let Ok(contents) = crate::file::read_to_string(&state_path) else {
            return true;
        };
        let Ok(state) = serde_json::from_str::<LifecycleState>(&contents) else {
            return true;
        };
        if !state.complete
            || state
                .symlinks
                .iter()
                .any(|link| resolved_symlink_target(&link.target).as_ref() != Some(&link.source))
            || state.required_paths.iter().any(|path| !path.exists())
            || state.absent_patterns.iter().any(|pattern| {
                glob::glob(pattern)
                    .ok()
                    .into_iter()
                    .flatten()
                    .any(|path| path.is_ok())
            })
        {
            return true;
        }
    }
    ["etc", "var"].into_iter().any(|root| {
        let source = keg.join(".bottle").join(root);
        source.exists() && shared_tree_missing(&source, &prefix::prefix().join(root))
    })
}

pub(super) async fn install(prepared: &PreparedFormulaLifecycle) -> Result<()> {
    let keg = &prepared.keg;
    let state_path = state_path(keg);
    write_state(
        &state_path,
        &LifecycleState {
            complete: false,
            phase: LifecyclePhase::Initial,
            symlinks: vec![],
            required_paths: vec![],
            absent_patterns: vec![],
        },
    )?;
    let mut symlinks = vec![];
    let mut required_paths = vec![];
    let mut absent_patterns = vec![];
    let result: Result<()> = async {
        for root in ["etc", "var"] {
            required_paths.extend(install_shared_tree(
                &prepared.formula,
                keg,
                root,
                &keg.join(".bottle").join(root),
                &prefix::prefix().join(root),
            )?);
        }
        write_state(
            &state_path,
            &LifecycleState {
                complete: false,
                phase: LifecyclePhase::SharedState,
                symlinks: vec![],
                required_paths: required_paths.clone(),
                absent_patterns: vec![],
            },
        )?;
        for step in &prepared.steps {
            let effects = execute_step(prepared, step).await?;
            symlinks.extend(effects.symlinks);
            required_paths.extend(effects.required_paths);
            absent_patterns.extend(effects.absent_patterns);
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
                symlinks,
                required_paths,
                absent_patterns,
            },
        )?;
    }
    result
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

pub(super) fn remove_owned_state(keg: &Path) -> Result<()> {
    let path = state_path(keg);
    if path.exists() {
        let state: LifecycleState = serde_json::from_str(&crate::file::read_to_string(&path)?)?;
        remove_lifecycle_symlinks(&state)?;
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
    keg: &Path,
    root: &str,
    source_root: &Path,
    destination_root: &Path,
) -> Result<Vec<PathBuf>> {
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
        let destination =
            install_destination(formula, keg, root, entry.path(), relative, &destination)?;
        atomic_copy(entry.path(), &destination)?;
        installed_paths.push(destination);
    }
    Ok(installed_paths)
}

fn install_destination(
    formula: &str,
    keg: &Path,
    root: &str,
    source: &Path,
    relative: &Path,
    destination: &Path,
) -> Result<PathBuf> {
    if destination.symlink_metadata().is_err() || files_equal(source, destination) {
        return Ok(destination.to_path_buf());
    }
    let rack = keg
        .parent()
        .ok_or_else(|| eyre!("keg has no formula rack"))?;
    for old_keg in crate::file::ls(rack).unwrap_or_default() {
        if old_keg == keg || !old_keg.is_dir() {
            continue;
        }
        let old_default = old_keg.join(".bottle").join(root).join(relative);
        if old_default.symlink_metadata().is_ok() && files_equal(&old_default, destination) {
            return Ok(destination.to_path_buf());
        }
    }
    let default = PathBuf::from(format!("{}.default", destination.display()));
    debug!(
        "brew:{} preserving modified {}; writing new default to {}",
        formula,
        destination.display(),
        default.display()
    );
    Ok(default)
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

fn shared_tree_missing(source_root: &Path, destination_root: &Path) -> bool {
    walkdir::WalkDir::new(source_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| !entry.file_type().is_dir())
        .any(|entry| {
            entry
                .path()
                .strip_prefix(source_root)
                .ok()
                .is_some_and(|relative| destination_root.join(relative).symlink_metadata().is_err())
        })
}

#[derive(Default)]
struct StepEffects {
    symlinks: Vec<LifecycleSymlink>,
    required_paths: Vec<PathBuf>,
    absent_patterns: Vec<String>,
}

async fn execute_step(
    prepared: &PreparedFormulaLifecycle,
    step: &PreparedStep,
) -> Result<StepEffects> {
    let guards = match step {
        PreparedStep::Mkdir { guards, .. }
        | PreparedStep::Remove { guards, .. }
        | PreparedStep::Copy { guards, .. }
        | PreparedStep::Symlink { guards, .. } => guards,
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
            for pattern in paths {
                absent_patterns.extend(pattern.patterns.clone());
                for path in expand_pattern(pattern)? {
                    ensure_runtime_write_path(&path, false)?;
                    remove_prepared_node(&path, *recursive, symlink_target_contains.as_deref())?;
                }
            }
            Ok(StepEffects {
                absent_patterns,
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
                let relative = super::pour::relative_target(&source, &destination);
                crate::file::make_symlink(&relative, &destination)?;
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
        PreparedStep::Run(run) => execute_run(prepared, run).await,
    }
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
            &keg,
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "user");
        let default = PathBuf::from(format!("{}.default", destination.display()));
        assert_eq!(crate::file::read_to_string(&default)?, "new");
        assert_eq!(installed, vec![default]);
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
            &keg,
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "new");
        assert_eq!(installed, vec![destination.clone()]);
        assert!(!PathBuf::from(format!("{}.default", destination.display())).exists());
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
            &keg,
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "user");
        assert_eq!(
            installed,
            vec![PathBuf::from(format!("{}.default", destination.display()))]
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
                &keg,
                root,
                &keg.join(".bottle").join(root),
                &tmp.path().join(root),
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
                &keg,
                root,
                &keg.join(".bottle").join(root),
                &tmp.path().join(root),
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
            symlinks: vec![LifecycleSymlink {
                source: source.clone(),
                target: target.clone(),
            }],
            required_paths: vec![],
            absent_patterns: vec![],
        };
        remove_lifecycle_symlinks(&state)?;
        assert!(!target.exists());

        crate::file::make_symlink(&replacement, &target)?;
        remove_lifecycle_symlinks(&state)?;
        assert_eq!(fs::read_link(target)?, replacement);
        Ok(())
    }
}
